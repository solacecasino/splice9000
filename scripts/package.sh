#!/bin/sh
# Builds dist/splice9000-<version>-macos-universal.zip: splice9000.app with
# its icon, for Apple Silicon and Intel Macs, ad-hoc signed.
# Usage: scripts/package.sh
set -e
cd "$(dirname "$0")/.."
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
APP=target/splice9000.app

rustup target add aarch64-apple-darwin x86_64-apple-darwin
cargo build --release --target aarch64-apple-darwin
cargo build --release --target x86_64-apple-darwin
mkdir -p target/release
lipo -create -output target/release/splice9000-universal \
  target/aarch64-apple-darwin/release/splice9000 target/x86_64-apple-darwin/release/splice9000

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/splice9000-universal "$APP/Contents/MacOS/splice9000"

# The 1024px PNG as an .icns with every size macOS asks for.
ICONSET=target/splice9000.iconset
rm -rf "$ICONSET" && mkdir "$ICONSET"
for s in 16 32 128 256 512; do
  sips -z $s $s assets/icon.png --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
  sips -z $((s*2)) $((s*2)) assets/icon.png --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/splice9000.icns"
rm -rf "$ICONSET"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>splice9000</string>
  <key>CFBundleDisplayName</key><string>splice9000</string>
  <key>CFBundleIdentifier</key><string>app.splice9000.editor</string>
  <key>CFBundleExecutable</key><string>splice9000</string>
  <key>CFBundleIconFile</key><string>splice9000</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.video</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSHumanReadableCopyright</key><string>© 2026 Vishal Mohan · MIT License</string>
</dict>
</plist>
PLIST

codesign --force --deep --sign - "$APP"
mkdir -p dist
rm -f "dist/splice9000-$VERSION-macos-universal.zip"
# ditto keeps the signature, permissions and icon intact.
ditto -c -k --sequesterRsrc --keepParent "$APP" "dist/splice9000-$VERSION-macos-universal.zip"
ls -la dist
