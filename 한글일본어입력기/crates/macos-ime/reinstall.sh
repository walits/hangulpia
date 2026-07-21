#!/bin/bash
set -euo pipefail

APP_NAME="HangulJapaneseIME"
INSTALL_DIR="$HOME/Library/Input Methods"

echo "🧹 1. Cleaning old installation..."
killall "$APP_NAME" 2>/dev/null || true
sleep 1
rm -rf "$INSTALL_DIR/$APP_NAME.app"

echo "🧹 2. Removing old defaults entry..."
# Remove our IME from AppleEnabledInputSources
python3 -c "
import plistlib, os, sys

plist_path = os.path.expanduser('~/Library/Preferences/com.apple.HIToolbox.plist')
try:
    with open(plist_path, 'rb') as f:
        data = plistlib.load(f)
    sources = data.get('AppleEnabledInputSources', [])
    filtered = [s for s in sources if s.get('Bundle ID', '') != 'com.hkd.inputmethod.HangulJapanese']
    if len(filtered) < len(sources):
        data['AppleEnabledInputSources'] = filtered
        with open(plist_path, 'wb') as f:
            plistlib.dump(data, f)
        print('   Removed old entry')
    else:
        print('   No old entry found')
except Exception as e:
    print(f'   Skip: {e}')
"

echo "🔨 3. Building..."
./build.sh build

echo "📦 4. Installing..."
cp -R build/$APP_NAME.app "$INSTALL_DIR/"

echo "🔏 5. Code signing..."
codesign --force --deep --sign - "$INSTALL_DIR/$APP_NAME.app"

echo "🚀 6. Launching..."
open "$INSTALL_DIR/$APP_NAME.app"
sleep 2

echo "🔍 7. Checking TIS registration..."
swift enable_ime.swift

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "위 결과에서 FOUND + Enabled: YES 가 나오면"
echo "메뉴바 입력 소스 아이콘 클릭해서 확인하세요."
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
