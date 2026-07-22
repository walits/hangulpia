#!/bin/bash
#
#  한글일본어입력기 제거 스크립트
#
#  사용법: bash uninstall.sh
#

set -euo pipefail

APP_NAME="HangulJapaneseIME"
INSTALL_DIR="$HOME/Library/Input Methods"

echo ""
echo "  한글일본어입력기 제거"
echo ""

# Kill process
echo "  🛑 프로세스 종료..."
killall "$APP_NAME" 2>/dev/null || true
sleep 1

# Remove app
if [ -d "$INSTALL_DIR/$APP_NAME.app" ]; then
    rm -rf "$INSTALL_DIR/$APP_NAME.app"
    echo "  🧹 앱 제거 완료"
else
    echo "  ℹ️  설치된 앱이 없습니다"
fi

# Remove app support data
APP_SUPPORT="$HOME/Library/Application Support/HangulJapaneseIME"
if [ -d "$APP_SUPPORT" ]; then
    rm -rf "$APP_SUPPORT"
    echo "  🧹 데이터 제거 완료"
fi

echo ""
echo "  ✅ 제거 완료. 로그아웃 후 재로그인하면 입력 소스 목록에서도 사라집니다."
echo ""
