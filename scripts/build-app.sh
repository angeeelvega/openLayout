#!/bin/sh
set -eu
cd "$(dirname "$0")/.."

cargo build --release

target_dir="${CARGO_TARGET_DIR:-target}"
app="OpenLayout.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS"
cp "$target_dir/release/openlayout" "$app/Contents/MacOS/openlayout"
cp Packaging/Info.plist "$app/Contents/Info.plist"
chmod +x "$app/Contents/MacOS/openlayout"
codesign --force --sign - "$app"
echo "Built $app"
