#!/bin/bash
#
#  한글일본어입력기 (HangulJapaneseIME) 설치 스크립트
#
#  사용법: bash install.sh
#
#  Rust, Xcode 등 개발 도구 불필요. 이 스크립트만 실행하면 됩니다.
#

set -euo pipefail

APP_NAME="HangulJapaneseIME"
INSTALL_DIR="$HOME/Library/Input Methods"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
APP_BUNDLE="$SCRIPT_DIR/$APP_NAME.app"

echo ""
echo "  ╔═══════════════════════════════════════════╗"
echo "  ║   한글일본어입력기 설치                     ║"
echo "  ║   한글 자판으로 일본어를 입력합니다          ║"
echo "  ╚═══════════════════════════════════════════╝"
echo ""

# Check app bundle exists
if [ ! -d "$APP_BUNDLE" ]; then
    echo "  ❌ $APP_NAME.app 을 찾을 수 없습니다."
    echo "     이 스크립트와 같은 폴더에 $APP_NAME.app 이 있어야 합니다."
    exit 1
fi

# Check macOS
if [ "$(uname)" != "Darwin" ]; then
    echo "  ❌ macOS에서만 설치할 수 있습니다."
    exit 1
fi

# Check architecture
ARCH=$(uname -m)
echo "  🖥  시스템: macOS $(sw_vers -productVersion) ($ARCH)"

if [ "$ARCH" != "arm64" ]; then
    echo "  ⚠️  이 빌드는 Apple Silicon (arm64) 전용입니다."
    echo "     Intel Mac에서는 소스 빌드가 필요합니다."
    exit 1
fi

# Kill existing instance
echo "  🧹 기존 버전 정리..."
killall "$APP_NAME" 2>/dev/null || true
sleep 1

# Create install directory
mkdir -p "$INSTALL_DIR"

# Remove old version
rm -rf "$INSTALL_DIR/$APP_NAME.app"

# Copy
echo "  📦 설치 중..."
cp -R "$APP_BUNDLE" "$INSTALL_DIR/"

# Code sign (ad-hoc)
echo "  🔏 코드 서명..."
codesign --force --deep --sign - "$INSTALL_DIR/$APP_NAME.app" 2>/dev/null || true

# Launch
echo "  🚀 입력기 시작..."
open "$INSTALL_DIR/$APP_NAME.app"
sleep 2

# Register with TIS
echo "  📋 입력 소스 등록..."
swift - <<'SWIFT_EOF' 2>/dev/null || true
import Carbon
import Foundation
guard let sources = TISCreateInputSourceList(nil, true)?.takeRetainedValue() as? [TISInputSource] else { exit(0) }
for source in sources {
    guard let rawID = TISGetInputSourceProperty(source, kTISPropertyInputSourceID) else { continue }
    let sourceID = Unmanaged<CFString>.fromOpaque(rawID).takeUnretainedValue() as String
    if sourceID.contains("hkd.inputmethod.HangulJapanese") {
        if let rawEnabled = TISGetInputSourceProperty(source, kTISPropertyInputSourceIsEnabled) {
            let enabled = CFBooleanGetValue(Unmanaged<CFBoolean>.fromOpaque(rawEnabled).takeUnretainedValue())
            if !enabled { TISEnableInputSource(source) }
        }
        TISSelectInputSource(source)
        break
    }
}
SWIFT_EOF

echo ""
echo "  ✅ 설치 완료!"
echo ""
echo "  ┌───────────────────────────────────────────────────────┐"
echo "  │                                                        │"
echo "  │  🔄 로그아웃 후 재로그인 해주세요!                      │"
echo "  │                                                        │"
echo "  │  재로그인 후:                                           │"
echo "  │  1. 메뉴바 입력 소스 아이콘 클릭 (한/A 표시)            │"
echo "  │  2. 'HangulJapaneseIME' 선택                           │"
echo "  │  3. 한글 두벌식 자판으로 일본어 입력!                    │"
echo "  │                                                        │"
echo "  │  예시:                                                  │"
echo "  │   dkflrkxh  (아리가토)  → ありがと                      │"
echo "  │   dkflrkxhndhdwkdlaktn → ありがとうございます           │"
echo "  │   sksksk    (나나나)    → ななな                         │"
echo "  │                                                        │"
echo "  │  제거: bash uninstall.sh                                │"
echo "  │                                                        │"
echo "  └───────────────────────────────────────────────────────┘"
echo ""
