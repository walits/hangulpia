#!/bin/bash
#
# dist.sh — 배포용 zip 패키지 생성
#
# 사용법: ./dist.sh
# 결과:   dist/HangulJapaneseIME-v0.1.3-arm64.zip
#

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
VERSION="0.1.3"
DIST_DIR="$SCRIPT_DIR/dist"
PKG_NAME="HangulJapaneseIME-v${VERSION}-arm64"

echo "📦 배포 패키지 생성: $PKG_NAME"
echo ""

# 1. Build
echo "🔨 빌드..."
"$SCRIPT_DIR/build.sh" build

# 2. Copy .app to dist
echo "📋 패키지 구성..."
rm -rf "$DIST_DIR/$PKG_NAME"
mkdir -p "$DIST_DIR/$PKG_NAME"

cp -R "$SCRIPT_DIR/build/HangulJapaneseIME.app" "$DIST_DIR/$PKG_NAME/"
cp "$DIST_DIR/install.sh" "$DIST_DIR/$PKG_NAME/"
cp "$DIST_DIR/uninstall.sh" "$DIST_DIR/$PKG_NAME/"
chmod +x "$DIST_DIR/$PKG_NAME/install.sh" "$DIST_DIR/$PKG_NAME/uninstall.sh"

# 3. Create README
cat > "$DIST_DIR/$PKG_NAME/README.txt" << 'EOF'
═══════════════════════════════════════════════════════════
  한글일본어입력기 (HangulJapaneseIME) v0.1.3
  한글 자판으로 일본어를 입력하는 macOS 입력기
═══════════════════════════════════════════════════════════

  ■ 요구 사항
    - macOS 13 (Ventura) 이상
    - Apple Silicon (M1/M2/M3/M4)

  ■ 설치 방법
    1. 터미널 열기 (Spotlight → "터미널" 검색)
    2. 이 폴더로 이동:
       cd ~/Downloads/HangulJapaneseIME-v0.1.3-arm64
    3. 설치 실행:
       bash install.sh
    4. 로그아웃 → 재로그인
    5. 메뉴바 입력 소스 아이콘 클릭 → HangulJapaneseIME 선택

  ■ 사용 방법
    한글 두벌식 자판 배열로 일본어를 입력합니다.
    입력한 한글이 실시간으로 히라가나로 변환됩니다.

    키보드 매핑 (두벌식 기준):
      d→ㅇ k→ㅏ  →  あ (아)
      f→ㄹ l→ㅣ  →  り (리)
      r→ㄱ k→ㅏ  →  が (가) / か
      x→ㅌ h→ㅗ  →  と (토)

    입력 예시:
      dkflrkxh          (아리가토)    → ありがと
      dkflrkxhndhdwkdlaktn (아리가토우고자이마스) → ありがとうございます
      sksksk            (나나나)      → ななな
      dhgkdhdndhwkdlaktn (오하요우고자이마스)   → おはようございます

    조작:
      스페이스 / 엔터  → 변환 확정
      백스페이스       → 한 글자 삭제
      Esc             → 입력 취소
      숫자 1-9        → 후보 직접 선택

  ■ 제거 방법
      bash uninstall.sh

  ■ 문제 해결
    Q: 입력 소스 목록에 안 보여요
    A: 로그아웃 → 재로그인 해보세요.
       그래도 안 되면: bash install.sh 다시 실행

    Q: 입력이 영어로만 돼요
    A: 메뉴바에서 입력 소스가 HangulJapaneseIME로
       선택되었는지 확인하세요.

  ■ 라이선스: MIT
═══════════════════════════════════════════════════════════
EOF

# 4. Create zip
echo "🗜  ZIP 생성..."
cd "$DIST_DIR"
rm -f "${PKG_NAME}.zip"
zip -r -q "${PKG_NAME}.zip" "$PKG_NAME/"

# 5. Cleanup temp directory
rm -rf "$PKG_NAME"

SIZE=$(ls -lh "${PKG_NAME}.zip" | awk '{print $5}')

echo ""
echo "✅ 완료: dist/${PKG_NAME}.zip ($SIZE)"
echo ""
echo "이 파일을 테스터에게 전달하세요."
echo "테스터는 zip 풀고 'bash install.sh' 만 실행하면 됩니다."
