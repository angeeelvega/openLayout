#!/bin/sh
set -eu
cd "$(dirname "$0")/.."

./scripts/build-app.sh

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
volume="openLayout"
image="OpenLayout-${version}.dmg"
stage=$(mktemp -d)
rw="$stage/rw.dmg"
mount="/Volumes/$volume"

cleanup() {
    hdiutil detach "$mount" >/dev/null 2>&1 || true
    rm -rf "$stage"
}
trap cleanup EXIT

bytes=$(du -sk OpenLayout.app | awk '{print $1}')
hdiutil create -size "$((bytes + 20480))k" -fs HFS+ -volname "$volume" "$rw" >/dev/null
if [ -d "$mount" ]; then
    hdiutil detach "$mount" >/dev/null 2>&1 || true
fi
hdiutil attach "$rw" -mountpoint "$mount" >/dev/null
sleep 1

cp -R OpenLayout.app "$mount/OpenLayout.app"
ln -s /Applications "$mount/Applications"

osascript <<EOF || echo "Finder layout skipped; the disk image still contains the app and Applications."
tell application "Finder"
    tell disk "$volume"
        open
        set current view of container window to icon view
        set toolbar visible of container window to false
        set statusbar visible of container window to false
        set the bounds of container window to {200, 120, 760, 480}
        set viewOptions to the icon view options of container window
        set arrangement of viewOptions to not arranged
        set icon size of viewOptions to 96
        set position of item "OpenLayout.app" of container window to {140, 180}
        set position of item "Applications" of container window to {420, 180}
        update without registering applications
        delay 1
        close
    end tell
end tell
EOF

sync
hdiutil detach "$mount" >/dev/null
rm -f "$image"
hdiutil convert "$rw" -format UDZO -imagekey zlib-level=9 -o "$image" >/dev/null
echo "Built $image"
