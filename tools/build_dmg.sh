#!/bin/bash
# Builds MicDeck.app and a drag-to-install .dmg for Apple Silicon. Run from the repo root: tools/build_dmg.sh
# Output: dist/MicDeck-<version>-arm64.dmg. The app is only ad-hoc signed (no paid Apple account), so macOS asks
# users to confirm once; the .dmg background and the README explain how.
set -euo pipefail
export PATH=/opt/homebrew/opt/rustup/bin:$PATH

VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
APP=dist/MicDeck.app
DMG=dist/MicDeck-$VERSION-arm64.dmg
VOL=MicDeck

echo "== Building the Rust library and the app"
cargo build --release -p micdeck-ffi
(cd app && swift build -c release --arch arm64)
BIN=$(cd app && swift build -c release --arch arm64 --show-bin-path)

echo "== Assembling $APP"
rm -rf dist && mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN/MicDeck" "$APP/Contents/MacOS/MicDeck"
cp -R "$BIN/MicDeck_MicDeck.bundle" "$APP/Contents/Resources/"   # the menu-bar icon lives in this resource bundle
cp app/Resources/AppIcon.icns "$APP/Contents/Resources/"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>MicDeck</string>
  <key>CFBundleDisplayName</key><string>MicDeck</string>
  <key>CFBundleIdentifier</key><string>io.github.ikarthikng.micdeck</string>
  <key>CFBundleExecutable</key><string>MicDeck</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>LSUIElement</key><true/>
  <key>NSMicrophoneUsageDescription</key><string>MicDeck listens to your microphone so Meeting mode can clean the sound.</string>
</dict></plist>
PLIST
codesign --force --deep --sign - "$APP"

echo "== Building $DMG"
STAGE=dist/stage
mkdir -p "$STAGE/.background"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
# Finder shows a background at its pixel size, so a sharp (2x) picture must be combined with a normal-size copy
# in one TIFF; Finder then picks the right one and scales it to the window.
sips -z 400 660 app/Resources/dmg-background.png --out dist/bg1x.png >/dev/null
cp app/Resources/dmg-background.png dist/bg2x.png
tiffutil -cathidpicheck dist/bg1x.png dist/bg2x.png -out "$STAGE/.background/background.tiff"
rm dist/bg1x.png dist/bg2x.png
cp app/Resources/AppIcon.icns "$STAGE/.VolumeIcon.icns"

# A writable disk image first, so Finder can be told how the window should look; then it is compressed and locked.
hdiutil detach "/Volumes/$VOL" >/dev/null 2>&1 || true
hdiutil create -quiet -srcfolder "$STAGE" -volname "$VOL" -fs HFS+ -format UDRW -ov dist/rw.dmg
hdiutil attach -quiet -readwrite -noverify -noautoopen dist/rw.dmg
SetFile -a C "/Volumes/$VOL" 2>/dev/null || echo "(no SetFile: the disk keeps a plain icon)"

# Window size and icon positions match dmg-background.png (660x400 points; the app sits at x=165, Applications at x=495, both at y=220).
osascript <<OSA || echo "(could not style the window; allow this terminal to control Finder and run again)"
tell application "Finder"
  tell disk "$VOL"
    open
    set current view of container window to icon view
    set toolbar visible of container window to false
    set statusbar visible of container window to false
    set the bounds of container window to {200, 120, 860, 542}
    set opts to the icon view options of container window
    set arrangement of opts to not arranged
    set icon size of opts to 96
    set background picture of opts to file ".background:background.tiff"
    set position of item "MicDeck.app" of container window to {165, 220}
    set position of item "Applications" of container window to {495, 220}
    close
    open
    update without registering applications
    delay 2
    close
  end tell
end tell
OSA
sync
hdiutil detach -quiet "/Volumes/$VOL"
hdiutil convert -quiet dist/rw.dmg -format UDZO -imagekey zlib-level=9 -o "$DMG"
rm -rf dist/rw.dmg "$STAGE"
echo "== Done: $DMG ($(du -h "$DMG" | cut -f1))"
