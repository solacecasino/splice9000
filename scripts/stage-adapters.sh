#!/bin/sh
# Puts the built-in adapters, built for this system, in <dest>: the folder
# splice9000 looks in beside its program (inside the app on macOS). They're
# this repo's own (adapters/) and the catalog's listed in
# adapters/bundled.txt, built from a checkout of the catalog at
# $SPLICE9000_CATALOG (default: ../splice9000-adapters).
# Usage: scripts/stage-adapters.sh <dest> [rust targets...]
#   With targets (macOS: aarch64-apple-darwin x86_64-apple-darwin), the
#   programs are built for each and joined into one universal program.
set -e
cd "$(dirname "$0")/.."
dest=$1
shift
catalog=${SPLICE9000_CATALOG:-../splice9000-adapters}
rm -rf "$dest"
mkdir -p "$dest"

# One adapter folder: copied as it is, or, with a Cargo.toml, its
# adapter.toml and its program (named for the folder), built.
stage() {
  src=$1
  name=$(basename "$src")
  if [ ! -f "$src/Cargo.toml" ]; then
    cp -R "$src" "$dest/"
    return
  fi
  mkdir -p "$dest/$name"
  cp "$src/adapter.toml" "$dest/$name/"
  if [ $# -gt 1 ]; then
    shift
    outs=""
    for t in "$@"; do
      cargo build --release --manifest-path "$src/Cargo.toml" --target "$t"
      outs="$outs $src/target/$t/release/$name"
    done
    # shellcheck disable=SC2086
    lipo -create -output "$dest/$name/$name" $outs
    codesign --force --sign - "$dest/$name/$name"
  else
    cargo build --release --manifest-path "$src/Cargo.toml"
    for f in "$name" "$name.exe"; do
      if [ -f "$src/target/release/$f" ]; then cp "$src/target/release/$f" "$dest/$name/"; fi
    done
  fi
}

stage adapters/export-presets "$@"
stage adapters/frame-grab "$@"
for name in $(grep -v '^#' adapters/bundled.txt | grep -v '^catalog ' | tr -d '\r'); do
  [ -f "$catalog/$name/adapter.toml" ] || { echo "No $name in the catalog at $catalog" >&2; exit 1; }
  stage "$catalog/$name" "$@"
done
ls -R "$dest"
