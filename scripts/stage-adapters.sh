#!/bin/sh
# Puts the built-in adapters, built for this system, in <dest>: the folder
# splice9000 looks in beside its program (inside the app on macOS).
# Usage: scripts/stage-adapters.sh <dest> [rust targets...]
#   With targets (macOS: aarch64-apple-darwin x86_64-apple-darwin), the
#   programs are built for each and joined into one universal program.
set -e
cd "$(dirname "$0")/.."
dest=$1
shift
rm -rf "$dest"
mkdir -p "$dest"

# Presets only: nothing to build.
cp -R adapters/export-presets "$dest/"

# Frame Grab: a small Rust program.
manifest=adapters/frame-grab/Cargo.toml
mkdir -p "$dest/frame-grab"
cp adapters/frame-grab/adapter.toml "$dest/frame-grab/"
if [ $# -gt 0 ]; then
  outs=""
  for t in "$@"; do
    cargo build --release --manifest-path "$manifest" --target "$t"
    outs="$outs adapters/frame-grab/target/$t/release/frame-grab"
  done
  # shellcheck disable=SC2086
  lipo -create -output "$dest/frame-grab/frame-grab" $outs
  codesign --force --sign - "$dest/frame-grab/frame-grab"
else
  cargo build --release --manifest-path "$manifest"
  for f in frame-grab frame-grab.exe; do
    [ -f "adapters/frame-grab/target/release/$f" ] && cp "adapters/frame-grab/target/release/$f" "$dest/frame-grab/"
  done
fi
ls -R "$dest"
