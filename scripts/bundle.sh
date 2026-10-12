#!/usr/bin/env bash
# Сборка target/DesktopSpider.app без cargo-bundle.
#
# Использование:
#   scripts/bundle.sh            # release-сборка + .app + ad-hoc подпись
#   open target/DesktopSpider.app
#
# Пресеты копируются в Contents/Resources/presets (config.rs ищет <exe>/../Resources/presets).
set -euo pipefail

cd "$(dirname "$0")/.."

BIN=desktop_spider
APP=target/DesktopSpider.app
VERSION=$(sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -1)

cargo build --release

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "target/release/$BIN" "$APP/Contents/MacOS/$BIN"

if [ -d presets ]; then
    cp -R presets "$APP/Contents/Resources/presets"
else
    echo "warning: ./presets не найдена, бандл без пресетов" >&2
fi

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key><string>com.alexander.desktopspider</string>
    <key>CFBundleName</key><string>Desktop Spider</string>
    <key>CFBundleDisplayName</key><string>Desktop Spider</string>
    <key>CFBundleExecutable</key><string>$BIN</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
    <key>CFBundleShortVersionString</key><string>$VERSION</string>
    <key>CFBundleVersion</key><string>$VERSION</string>
    <key>LSMinimumSystemVersion</key><string>13.0</string>
    <key>LSUIElement</key><true/>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

plutil -lint "$APP/Contents/Info.plist" >/dev/null
codesign -s - --force --deep "$APP"
echo "готово: $APP"
