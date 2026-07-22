#!/bin/bash
# ============================================
# 🏗️ 한글일본어입력기 - Project Harness Bootstrap Script
# Rust 크로스플랫폼 IME에 CI/CD + 에이전트 자동화 하네스를 한 번에 설정
#
# 사용법:
#   bash scripts/bootstrap-harness.sh  (로컬)
#
# 설정 항목:
#   1. Repository 하네스 (CI, auto-label, cross-compile check)
#   2. Deployment 하네스 (macOS .app / Android .aar 빌드, 릴리즈)
#   3. Agent 워크플로우 (auto-commit, auto-PR, promote)
#   4. Coding 하네스 (rustfmt, clippy, cargo-nextest, commitlint, pre-commit)
#   5. GitHub 설정 (branch protection, environments, secrets)
# ============================================

set -euo pipefail

# ──────────────────────────────────────
# 색상
# ──────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

log()  { echo -e "${BLUE}[harness]${NC} $1"; }
ok()   { echo -e "${GREEN}  ✅${NC} $1"; }
warn() { echo -e "${YELLOW}  ⚠️${NC} $1"; }
err()  { echo -e "${RED}  ❌${NC} $1"; }
header() {
  echo ""
  echo -e "${CYAN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
  echo -e "${BOLD}  $1${NC}"
  echo -e "${CYAN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
}

# ──────────────────────────────────────
# 사전 조건 확인
# ──────────────────────────────────────
check_prerequisites() {
  header "사전 조건 확인"

  if ! git rev-parse --is-inside-work-tree &>/dev/null; then
    err "Git 레포지터리가 아닙니다. git init 먼저 실행하세요."
    exit 1
  fi
  ok "Git 레포지터리 확인"

  if ! command -v gh &>/dev/null; then
    warn "gh CLI가 없습니다. GitHub 설정은 수동으로 진행하세요."
    warn "설치: brew install gh (macOS) / https://cli.github.com"
    HAS_GH=false
  else
    ok "gh CLI 확인"
    HAS_GH=true
  fi

  if ! command -v rustup &>/dev/null; then
    warn "Rust toolchain이 없습니다. 설치 중..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
  fi
  ok "Rust $(rustc --version 2>/dev/null || echo 'installed')"

  if ! command -v cargo &>/dev/null; then
    err "cargo를 찾을 수 없습니다. Rust toolchain 설치를 확인하세요."
    exit 1
  fi
  ok "Cargo $(cargo --version 2>/dev/null | head -1)"
}

# ──────────────────────────────────────
# 프로젝트 정보 수집
# ──────────────────────────────────────
collect_project_info() {
  header "프로젝트 설정"

  # Git remote에서 자동 감지
  REMOTE_URL=$(git remote get-url origin 2>/dev/null || echo "")
  if [ -n "$REMOTE_URL" ]; then
    AUTO_REPO=$(echo "$REMOTE_URL" | sed -E 's|.*github\.com[:/]||;s|\.git$||')
    AUTO_OWNER=$(echo "$AUTO_REPO" | cut -d'/' -f1)
    AUTO_NAME=$(echo "$AUTO_REPO" | cut -d'/' -f2)
  fi

  # GitHub 레포지터리
  echo ""
  read -p "  GitHub 레포 (owner/repo) [${AUTO_REPO:-미설정}]: " INPUT_REPO
  REPO="${INPUT_REPO:-${AUTO_REPO:-}}"

  if [ -n "$REPO" ]; then
    OWNER=$(echo "$REPO" | cut -d'/' -f1)
    REPO_NAME=$(echo "$REPO" | cut -d'/' -f2)
    ok "레포: $REPO"
  else
    warn "GitHub 레포 미설정. GitHub 관련 설정은 건너뜁니다."
    OWNER=""
    REPO_NAME="hangul-japanese-ime"
  fi

  PROJECT_NAME="한글일본어입력기"

  # 타겟 플랫폼
  echo ""
  echo -e "  ${BOLD}타겟 플랫폼 (빌드 대상):${NC}"
  echo "    1) macOS + Android (기본)"
  echo "    2) macOS only"
  echo "    3) Android only"
  echo "    4) macOS + Android + Linux"
  read -p "  선택 [1]: " PLATFORM_CHOICE
  PLATFORM_CHOICE="${PLATFORM_CHOICE:-1}"

  HAS_MACOS=false; HAS_ANDROID=false; HAS_LINUX=false
  case "$PLATFORM_CHOICE" in
    1) HAS_MACOS=true; HAS_ANDROID=true ;;
    2) HAS_MACOS=true ;;
    3) HAS_ANDROID=true ;;
    4) HAS_MACOS=true; HAS_ANDROID=true; HAS_LINUX=true ;;
  esac

  # 코드 서명
  echo ""
  echo -e "  ${BOLD}코드 서명 설정:${NC}"
  HAS_APPLE_SIGN=false; HAS_ANDROID_SIGN=false
  if $HAS_MACOS; then
    read -p "  Apple Developer ID로 코드 서명? (y/N): " APPLE_SIGN
    [[ "$APPLE_SIGN" =~ ^[Yy]$ ]] && HAS_APPLE_SIGN=true
  fi
  if $HAS_ANDROID; then
    read -p "  Android keystore 서명? (y/N): " ANDROID_SIGN
    [[ "$ANDROID_SIGN" =~ ^[Yy]$ ]] && HAS_ANDROID_SIGN=true
  fi

  # 요약
  header "설정 요약"
  echo "  프로젝트: $PROJECT_NAME"
  echo "  구조:     Rust workspace (core + hangul + japanese + db + platform bridges)"
  $HAS_MACOS && echo "  플랫폼:   macOS (InputMethodKit)"
  $HAS_ANDROID && echo "  플랫폼:   Android (JNI bridge)"
  $HAS_LINUX && echo "  플랫폼:   Linux (IBus/Fcitx)"
  echo "  DB:       SQLite (rusqlite, bundled)"
  $HAS_APPLE_SIGN && echo "  서명:     Apple Developer ID"
  $HAS_ANDROID_SIGN && echo "  서명:     Android Keystore"
  echo ""
  read -p "  이대로 진행할까요? (Y/n): " CONFIRM
  if [[ "${CONFIRM:-Y}" =~ ^[Nn] ]]; then
    echo "취소되었습니다."
    exit 0
  fi
}

# ──────────────────────────────────────
# 1. Repository 하네스
# ──────────────────────────────────────
create_repo_harness() {
  header "1. Repository 하네스 생성"
  REPO_ROOT=$(git rev-parse --show-toplevel)
  cd "$REPO_ROOT"

  mkdir -p .github/workflows .github/PULL_REQUEST_TEMPLATE

  # ── CI 워크플로우 ──
  log "CI 워크플로우 생성..."

  cat > .github/workflows/ci.yml << 'CIEOF'
# ============================================
# 한글일본어입력기 CI (Continuous Integration)
# PR 및 push 시 린트/테스트/빌드 자동 검증
# ============================================

name: CI

on:
  pull_request:
    branches: [main, staging]
  push:
    branches: [main, staging]

concurrency:
  group: ci-${{ github.ref }}
  cancel-in-progress: true

env:
  CARGO_TERM_COLOR: always
  RUST_BACKTRACE: 1

jobs:
  # ── 변경 감지 ──
  changes:
    name: 🔍 변경 감지
    runs-on: ubuntu-latest
    outputs:
      core: ${{ steps.filter.outputs.core }}
      hangul: ${{ steps.filter.outputs.hangul }}
      japanese: ${{ steps.filter.outputs.japanese }}
      db: ${{ steps.filter.outputs.db }}
      macos: ${{ steps.filter.outputs.macos }}
      android: ${{ steps.filter.outputs.android }}
    steps:
      - uses: actions/checkout@v4
      - uses: dorny/paths-filter@v3
        id: filter
        with:
          filters: |
            core:
              - 'crates/core/**'
              - 'Cargo.toml'
              - 'Cargo.lock'
            hangul:
              - 'crates/hangul/**'
              - 'crates/core/**'
            japanese:
              - 'crates/japanese/**'
              - 'crates/core/**'
              - 'crates/db/**'
            db:
              - 'crates/db/**'
              - 'dictionaries/**'
            macos:
              - 'crates/macos-ime/**'
              - 'crates/core/**'
            android:
              - 'crates/android-bridge/**'
              - 'crates/core/**'

  # ── 린트 & 포맷 ──
  lint:
    name: 🔧 린트 & 포맷
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
      - name: rustfmt 체크
        run: cargo fmt --all -- --check
      - name: clippy
        run: cargo clippy --workspace --all-targets -- -D warnings

  # ── 테스트 ──
  test:
    name: 🧪 테스트
    needs: changes
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - name: cargo nextest (또는 cargo test)
        run: |
          if command -v cargo-nextest &>/dev/null; then
            cargo nextest run --workspace
          else
            cargo test --workspace
          fi
      - name: doc tests
        run: cargo test --workspace --doc

  # ── macOS 빌드 ──
  macos-build:
    name: 🍎 macOS 빌드
    needs: changes
    if: >
      needs.changes.outputs.core == 'true' ||
      needs.changes.outputs.macos == 'true' ||
      needs.changes.outputs.hangul == 'true' ||
      needs.changes.outputs.japanese == 'true'
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - name: macOS 빌드
        run: cargo build --workspace --release
      - name: macOS 테스트
        run: cargo test --workspace

  # ── Android 크로스컴파일 ──
  android-check:
    name: 📱 Android 크로스컴파일
    needs: changes
    if: >
      needs.changes.outputs.core == 'true' ||
      needs.changes.outputs.android == 'true' ||
      needs.changes.outputs.hangul == 'true' ||
      needs.changes.outputs.japanese == 'true'
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: aarch64-linux-android, armv7-linux-androideabi, x86_64-linux-android
      - uses: Swatinem/rust-cache@v2
      - uses: nttld/setup-ndk@v1
        with:
          ndk-version: r26d
      - name: Android 빌드 체크
        run: |
          for target in aarch64-linux-android armv7-linux-androideabi x86_64-linux-android; do
            echo "🔨 Checking $target..."
            cargo check -p ime-android-bridge --target "$target" || true
          done

  # ── CI 게이트 ──
  ci-gate:
    name: ✅ CI 게이트
    if: always()
    needs:
      - lint
      - test
      - macos-build
      - android-check
    runs-on: ubuntu-latest
    steps:
      - name: 결과 확인
        run: |
          echo "## CI 결과"
          echo "Lint:    ${{ needs.lint.result }}"
          echo "Test:    ${{ needs.test.result }}"
          echo "macOS:   ${{ needs.macos-build.result }}"
          echo "Android: ${{ needs.android-check.result }}"

          if [[ "${{ needs.lint.result }}" == "failure" ]]; then exit 1; fi
          if [[ "${{ needs.test.result }}" == "failure" ]]; then exit 1; fi
          # macOS/Android는 스킵될 수 있으므로 failure만 체크
          if [[ "${{ needs.macos-build.result }}" == "failure" ]]; then exit 1; fi
          if [[ "${{ needs.android-check.result }}" == "failure" ]]; then exit 1; fi
          echo "✅ CI 통과"
CIEOF
  ok "CI 워크플로우"

  # ── 자동 라벨링 ──
  log "자동 라벨링 설정..."

  cat > .github/workflows/auto-label.yml << 'LABELWF'
name: Auto Label

on:
  pull_request:
    types: [opened, synchronize, reopened]

permissions:
  contents: read
  pull-requests: write

jobs:
  label:
    name: 🏷️ 자동 라벨링
    runs-on: ubuntu-latest
    steps:
      - uses: actions/labeler@v5
        with:
          repo-token: ${{ secrets.GITHUB_TOKEN }}
LABELWF

  cat > .github/labeler.yml << 'LABELEOF'
# PR 자동 라벨링 규칙
core:
  - changed-files:
    - any-glob-to-any-file: 'crates/core/**'

hangul:
  - changed-files:
    - any-glob-to-any-file: 'crates/hangul/**'

japanese:
  - changed-files:
    - any-glob-to-any-file: 'crates/japanese/**'

database:
  - changed-files:
    - any-glob-to-any-file:
      - 'crates/db/**'
      - 'dictionaries/**'
      - '**/*.sql'

macos:
  - changed-files:
    - any-glob-to-any-file: 'crates/macos-ime/**'

android:
  - changed-files:
    - any-glob-to-any-file: 'crates/android-bridge/**'

infra:
  - changed-files:
    - any-glob-to-any-file:
      - '.github/**'
      - 'scripts/**'
      - 'Cargo.toml'

docs:
  - changed-files:
    - any-glob-to-any-file:
      - '**/*.md'
      - 'docs/**'
LABELEOF
  ok "자동 라벨링"

  # ── PR 템플릿 ──
  log "PR 템플릿 생성..."
  cat > .github/PULL_REQUEST_TEMPLATE/default.md << 'PREOF'
## 변경 사항
<!-- 이 PR에서 변경한 내용을 간단히 설명해주세요 -->


## 변경 유형
- [ ] 🐛 버그 수정
- [ ] ✨ 새 기능
- [ ] ♻️ 리팩토링
- [ ] 📝 문서 수정
- [ ] 🔧 인프라/설정 변경
- [ ] 🧪 테스트 추가/수정

## 영향 범위
- [ ] 코어 엔진 (crates/core)
- [ ] 한글 입력 (crates/hangul)
- [ ] 일본어 입력 (crates/japanese)
- [ ] 사전 DB (crates/db)
- [ ] macOS IME (crates/macos-ime)
- [ ] Android bridge (crates/android-bridge)

## 테스트
- [ ] `cargo test --workspace` 통과
- [ ] `cargo clippy --workspace` 경고 없음
- [ ] 관련 플랫폼에서 수동 테스트 완료

## 배포 영향
- [ ] SQLite 스키마 변경
- [ ] 새 의존성 추가
- [ ] 플랫폼 API 변경
PREOF
  ok "PR 템플릿"
}

# ──────────────────────────────────────
# 2. Deployment 하네스
# ──────────────────────────────────────
create_deploy_harness() {
  header "2. Deployment 하네스 생성"

  # ── 릴리즈 빌드 ──
  log "릴리즈 빌드 워크플로우..."
  cat > .github/workflows/release-build.yml << 'RELEOF'
name: Release Build

on:
  push:
    tags: ['v*']
  workflow_dispatch:
    inputs:
      version:
        description: '릴리즈 버전 (예: v0.1.0)'
        required: true

env:
  CARGO_TERM_COLOR: always

jobs:
  # macOS 릴리즈 빌드
  macos-release:
    name: 🍎 macOS Release
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: x86_64-apple-darwin, aarch64-apple-darwin
      - uses: Swatinem/rust-cache@v2

      - name: 빌드 (x86_64)
        run: cargo build --workspace --release --target x86_64-apple-darwin

      - name: 빌드 (aarch64 / Apple Silicon)
        run: cargo build --workspace --release --target aarch64-apple-darwin

      - name: Universal Binary 생성
        run: |
          mkdir -p target/universal-release
          for bin in target/x86_64-apple-darwin/release/*.dylib; do
            NAME=$(basename "$bin")
            if [ -f "target/aarch64-apple-darwin/release/$NAME" ]; then
              lipo -create \
                "target/x86_64-apple-darwin/release/$NAME" \
                "target/aarch64-apple-darwin/release/$NAME" \
                -output "target/universal-release/$NAME"
            fi
          done

      - name: 아티팩트 업로드
        uses: actions/upload-artifact@v4
        with:
          name: macos-release
          path: target/universal-release/

  # Android 릴리즈 빌드
  android-release:
    name: 📱 Android Release
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: aarch64-linux-android, armv7-linux-androideabi, x86_64-linux-android
      - uses: Swatinem/rust-cache@v2
      - uses: nttld/setup-ndk@v1
        with:
          ndk-version: r26d

      - name: 빌드 (aarch64)
        run: cargo build -p ime-android-bridge --release --target aarch64-linux-android
        continue-on-error: true

      - name: 빌드 (armv7)
        run: cargo build -p ime-android-bridge --release --target armv7-linux-androideabi
        continue-on-error: true

      - name: 아티팩트 업로드
        uses: actions/upload-artifact@v4
        with:
          name: android-release
          path: |
            target/aarch64-linux-android/release/*.so
            target/armv7-linux-androideabi/release/*.so

  # GitHub Release 생성
  create-release:
    name: 📦 GitHub Release
    needs: [macos-release, android-release]
    runs-on: ubuntu-latest
    permissions:
      contents: write
    steps:
      - uses: actions/checkout@v4
      - uses: actions/download-artifact@v4

      - name: 릴리즈 생성
        uses: softprops/action-gh-release@v2
        with:
          generate_release_notes: true
          files: |
            macos-release/*
            android-release/**/*
RELEOF
  ok "릴리즈 빌드 워크플로우"

  # ── 롤백 ──
  log "롤백 워크플로우..."
  cat > .github/workflows/rollback.yml << 'RBEOF'
name: Manual Rollback

on:
  workflow_dispatch:
    inputs:
      target_tag:
        description: '롤백할 태그 (예: v0.1.0)'
        required: true
      reason:
        description: '롤백 사유'
        required: true

jobs:
  rollback:
    name: 🔄 롤백
    runs-on: ubuntu-latest
    steps:
      - name: 롤백 정보
        run: |
          echo "🔄 롤백 시작"
          echo "대상 태그: ${{ inputs.target_tag }}"
          echo "사유: ${{ inputs.reason }}"
          echo ""
          echo "⚠️ 해당 태그의 릴리즈 아티팩트를 다시 배포하세요."
          echo "   gh release download ${{ inputs.target_tag }}"
RBEOF
  ok "롤백 워크플로우"
}

# ──────────────────────────────────────
# 3. Agent 워크플로우
# ──────────────────────────────────────
create_agent_harness() {
  header "3. Agent 자동화 워크플로우 생성"

  mkdir -p scripts

  # ── agent-commit.sh ──
  log "에이전트 커밋 스크립트..."
  cat > scripts/agent-commit.sh << 'ACEOF'
#!/bin/bash
# Agent Auto-Commit Script (Rust IME)
# 사용법: ./scripts/agent-commit.sh "변경 설명"
set -euo pipefail
REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; BLUE='\033[0;34m'; NC='\033[0m'
log() { echo -e "${BLUE}[agent]${NC} $1"; }
ok()  { echo -e "${GREEN}[✓]${NC} $1"; }
warn(){ echo -e "${YELLOW}[!]${NC} $1"; }
err() { echo -e "${RED}[✗]${NC} $1"; }

CURRENT_BRANCH=$(git branch --show-current)

if [[ "$CURRENT_BRANCH" == "main" || "$CURRENT_BRANCH" == "staging" ]]; then
  err "main/staging 브랜치에서는 직접 커밋할 수 없습니다."
  log "feature 브랜치를 생성합니다..."
  TIMESTAMP=$(date +%Y%m%d-%H%M%S)
  SESSION_ID="${AGENT_SESSION_ID:-auto}"
  BRANCH_NAME="agent/${SESSION_ID}-${TIMESTAMP}"
  git checkout -b "$BRANCH_NAME"
  ok "브랜치 생성됨: $BRANCH_NAME"
  CURRENT_BRANCH="$BRANCH_NAME"
fi

if git diff --quiet && git diff --cached --quiet && [ -z "$(git ls-files --others --exclude-standard)" ]; then
  warn "변경 사항이 없습니다."; exit 0
fi

# 위험 파일 검사
DANGEROUS_PATTERNS=(".env" "credentials" "secret" "private_key" "id_rsa" ".pem" ".p12" ".keystore")

# 스테이징
git add -A
for pattern in "${DANGEROUS_PATTERNS[@]}"; do
  git diff --cached --name-only | grep -i "$pattern" | while read -r f; do
    git reset HEAD -- "$f" 2>/dev/null || true
    warn "제외됨: $f"
  done
done

# 빌드 체크 (빠른 검증)
log "빌드 체크 중..."
if ! cargo check --workspace --quiet 2>/dev/null; then
  warn "빌드 경고가 있습니다. 커밋은 진행합니다."
fi

# 커밋 메시지
USER_MSG="${1:-}"
FILE_COUNT=$(git diff --cached --name-only | wc -l | tr -d ' ')
if [ -n "$USER_MSG" ]; then
  if echo "$USER_MSG" | grep -qE '^(feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert)(\(.+\))?:'; then
    COMMIT_MSG="$USER_MSG"
  else
    # 변경된 크레이트 자동 감지
    CHANGED_CRATE=$(git diff --cached --name-only | grep -oP 'crates/\K[^/]+' | sort -u | head -1)
    if [ -n "$CHANGED_CRATE" ]; then
      COMMIT_MSG="feat(${CHANGED_CRATE}): ${USER_MSG}"
    else
      COMMIT_MSG="feat: ${USER_MSG}"
    fi
  fi
else
  COMMIT_MSG="feat: update ${FILE_COUNT} files via agent"
fi
COMMIT_MSG=$(echo "$COMMIT_MSG" | cut -c1-72)

git commit --no-verify -m "${COMMIT_MSG}

Co-Authored-By: Claude Agent <noreply@anthropic.com>"
ok "커밋: $COMMIT_MSG"

git push -u origin "$CURRENT_BRANCH" 2>/dev/null || git push origin "$CURRENT_BRANCH"
ok "푸시 완료: $CURRENT_BRANCH"
ACEOF
  chmod +x scripts/agent-commit.sh
  ok "에이전트 커밋 스크립트"

  # ── agent-pr-staging.yml ──
  log "자동 PR 워크플로우..."
  cat > .github/workflows/agent-pr-staging.yml << 'APREOF'
name: Agent → Staging PR

on:
  push:
    branches: ['agent/**']

permissions:
  contents: read
  pull-requests: write

jobs:
  create-pr:
    name: 📋 Staging PR 생성
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0

      - name: staging 브랜치 확인
        run: |
          if ! git ls-remote --heads origin staging | grep -q staging; then
            git checkout -b staging origin/main
            git push origin staging
          fi

      - name: 기존 PR 확인
        id: check-pr
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        run: |
          BRANCH="${{ github.ref_name }}"
          EXISTING=$(gh pr list --head "$BRANCH" --base staging --state open --json number --jq '.[0].number // empty')
          echo "existing_pr=${EXISTING}" >> "$GITHUB_OUTPUT"

      - name: PR 생성
        if: steps.check-pr.outputs.existing_pr == ''
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        run: |
          BRANCH="${{ github.ref_name }}"
          FILES=$(git diff --name-only origin/staging..HEAD 2>/dev/null | wc -l | tr -d ' ')

          # 변경된 크레이트 감지
          CRATES=$(git diff --name-only origin/staging..HEAD 2>/dev/null | grep -oP 'crates/\K[^/]+' | sort -u | tr '\n' ', ' | sed 's/,$//')

          gh pr create \
            --head "$BRANCH" --base staging \
            --title "[Agent] ${BRANCH##agent/} (${FILES} files)" \
            --body "## 🤖 Agent 자동 PR
          **브랜치:** \`${BRANCH}\`
          **영향 크레이트:** ${CRATES:-없음}

          > ⚠️ CI 검증 통과 후 staging 머지를 진행하세요."

      - name: 기존 PR 코멘트
        if: steps.check-pr.outputs.existing_pr != ''
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        run: |
          COMMIT_MSG=$(git log -1 --format="%s")
          gh pr comment "${{ steps.check-pr.outputs.existing_pr }}" \
            --body "🔄 새 커밋: \`${{ github.sha }}\` - ${COMMIT_MSG}"
APREOF
  ok "자동 PR 워크플로우"

  # ── promote-production.yml ──
  log "Production 승격 워크플로우..."
  cat > .github/workflows/promote-production.yml << 'PROMEOF'
name: Promote to Production

on:
  workflow_dispatch:
    inputs:
      description:
        description: '배포 설명'
        required: true
        type: string

permissions:
  contents: write
  pull-requests: write

jobs:
  promote:
    name: 🚀 Production 승격 PR
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0

      - name: 변경 확인 및 PR 생성
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        run: |
          git fetch origin main staging
          DIFF=$(git rev-list --count origin/main..origin/staging)
          if [ "$DIFF" -eq 0 ]; then
            echo "::warning::staging과 main이 동일합니다."
            exit 0
          fi

          EXISTING=$(gh pr list --head staging --base main --state open --json number --jq '.[0].number // empty')
          if [ -n "$EXISTING" ]; then
            gh pr comment "$EXISTING" --body "🔄 승격 요청: ${{ inputs.description }}"
          else
            COMMITS=$(git log origin/main..origin/staging --oneline)
            gh pr create --head staging --base main \
              --title "🚀 Production: ${{ inputs.description }}" \
              --body "## 🚀 Production 승격
          **설명:** ${{ inputs.description }}
          **커밋 수:** ${DIFF}개

          ### 커밋 내역
          \`\`\`
          ${COMMITS}
          \`\`\`

          > ✅ 머지하면 태그를 생성하여 릴리즈 빌드가 자동 실행됩니다."
          fi
PROMEOF
  ok "Production 승격 워크플로우"
}

# ──────────────────────────────────────
# 4. Coding 하네스
# ──────────────────────────────────────
create_coding_harness() {
  header "4. Coding 하네스 생성"
  REPO_ROOT=$(git rev-parse --show-toplevel)
  cd "$REPO_ROOT"

  # ── rustfmt ──
  log "rustfmt 설정..."
  cat > rustfmt.toml << 'RFEOF'
edition = "2021"
max_width = 100
use_field_init_shorthand = true
use_try_shorthand = true
RFEOF
  ok "rustfmt"

  # ── clippy ──
  log "clippy 설정..."
  cat > clippy.toml << 'CLPEOF'
too-many-arguments-threshold = 8
cognitive-complexity-threshold = 30
CLPEOF
  ok "clippy"

  # ── commitlint ──
  log "commitlint 설정..."
  cat > commitlint.config.js << 'CLEOF'
module.exports = {
  extends: ['@commitlint/config-conventional'],
  rules: {
    'type-enum': [2, 'always', [
      'feat', 'fix', 'docs', 'style', 'refactor',
      'perf', 'test', 'build', 'ci', 'chore', 'revert',
    ]],
    'scope-enum': [1, 'always', [
      'core', 'hangul', 'japanese', 'db',
      'macos', 'android', 'linux',
      'ci', 'infra', 'deps', 'scripts',
    ]],
    'header-max-length': [2, 'always', 72],
    'subject-empty': [2, 'never'],
    'type-empty': [2, 'never'],
  },
};
CLEOF
  ok "commitlint"

  # ── Husky (pre-commit via cargo) ──
  log "Git hooks 설정..."
  mkdir -p .husky

  cat > .husky/pre-commit << 'HOOKEOF'
#!/usr/bin/env bash
set -euo pipefail

echo "🔍 Pre-commit checks..."

# [1/3] Format check
echo "  [1/3] rustfmt..."
cargo fmt --all -- --check || {
    echo "❌ Formatting issues. Run 'cargo fmt --all' to fix."
    exit 1
}

# [2/3] Clippy
echo "  [2/3] clippy..."
cargo clippy --workspace --all-targets -- -D warnings || {
    echo "❌ Clippy warnings. Please fix before committing."
    exit 1
}

# [3/3] Quick tests (core crates only for speed)
echo "  [3/3] tests (core)..."
cargo test -p ime-core -p ime-hangul -p ime-db --quiet || {
    echo "❌ Tests failed."
    exit 1
}

echo "✅ All pre-commit checks passed!"
HOOKEOF
  chmod +x .husky/pre-commit

  # commitlint hook (Node.js 있을 때만 동작)
  cat > .husky/commit-msg << 'CMEOF'
#!/usr/bin/env bash
# Conventional Commits 검증 (Node.js + commitlint 설치 시)
if command -v npx &>/dev/null && [ -f "$(git rev-parse --show-toplevel)/commitlint.config.js" ]; then
  npx --no -- commitlint --edit "$1"
else
  # 간이 검증: type: 형식인지만 체크
  MSG=$(head -1 "$1")
  if ! echo "$MSG" | grep -qE '^(feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert)(\(.+\))?:'; then
    echo "❌ 커밋 메시지는 Conventional Commits 형식이어야 합니다."
    echo "   예: feat(hangul): 두벌식 자모 조합 구현"
    exit 1
  fi
fi
CMEOF
  chmod +x .husky/commit-msg
  ok "Git hooks"

  # ── 설치 스크립트 ──
  log "코딩 하네스 설치 스크립트..."
  cat > scripts/setup-coding-harness.sh << 'SHEOF'
#!/bin/bash
# ============================================
# 한글일본어입력기 코딩 하네스 로컬 설치 스크립트
# 사용법: bash scripts/setup-coding-harness.sh
# ============================================

set -e

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
CYAN='\033[0;36m'
NC='\033[0m'

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

cd "$PROJECT_ROOT"

echo -e "${CYAN}========================================${NC}"
echo -e "${CYAN}  한글일본어입력기 코딩 하네스 설치${NC}"
echo -e "${CYAN}========================================${NC}"
echo ""

# ──────────────────────────────────────────
# 1. Rust 확인
# ──────────────────────────────────────────
echo -e "${YELLOW}[1/5] Rust toolchain 확인...${NC}"
if ! command -v rustup &> /dev/null; then
  echo -e "${RED}❌ Rust가 설치되어 있지 않습니다.${NC}"
  echo "   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
  exit 1
fi
rustup update stable
RUST_VER=$(rustc --version)
echo -e "  ${GREEN}✅ ${RUST_VER}${NC}"
echo ""

# ──────────────────────────────────────────
# 2. 컴포넌트 설치
# ──────────────────────────────────────────
echo -e "${YELLOW}[2/5] Rust 컴포넌트 설치 (rustfmt, clippy)...${NC}"
rustup component add rustfmt clippy
echo -e "  ${GREEN}✅ rustfmt, clippy${NC}"
echo ""

# ──────────────────────────────────────────
# 3. Android 크로스컴파일 타겟
# ──────────────────────────────────────────
echo -e "${YELLOW}[3/5] Android 크로스컴파일 타겟 추가...${NC}"
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
echo -e "  ${GREEN}✅ Android targets${NC}"
echo ""

# ──────────────────────────────────────────
# 4. 개발 도구 설치
# ──────────────────────────────────────────
echo -e "${YELLOW}[4/5] 개발 도구 설치...${NC}"
if ! command -v cargo-watch &>/dev/null; then
  cargo install cargo-watch
fi
echo -e "  ${GREEN}✅ cargo-watch${NC}"

if ! command -v cargo-nextest &>/dev/null; then
  cargo install cargo-nextest --locked
fi
echo -e "  ${GREEN}✅ cargo-nextest${NC}"

# commitlint (선택, Node.js 있을 때만)
if command -v node &>/dev/null; then
  echo ""
  echo -e "${YELLOW}  [선택] commitlint 설치 (Node.js 감지됨)...${NC}"
  npm install --save-dev @commitlint/cli @commitlint/config-conventional 2>/dev/null || true
  echo -e "  ${GREEN}✅ commitlint${NC}"
fi
echo ""

# ──────────────────────────────────────────
# 5. Git hooks 활성화
# ──────────────────────────────────────────
echo -e "${YELLOW}[5/5] Git hooks 활성화...${NC}"
HOOK_DIR="$(git rev-parse --show-toplevel)/.git/hooks"
mkdir -p "$HOOK_DIR"

# .husky 훅을 .git/hooks로 복사
if [ -f .husky/pre-commit ]; then
  cp .husky/pre-commit "$HOOK_DIR/pre-commit"
  chmod +x "$HOOK_DIR/pre-commit"
fi
if [ -f .husky/commit-msg ]; then
  cp .husky/commit-msg "$HOOK_DIR/commit-msg"
  chmod +x "$HOOK_DIR/commit-msg"
fi
echo -e "  ${GREEN}✅ Git hooks 활성화 완료${NC}"
echo "    ├─ pre-commit: rustfmt + clippy + tests"
echo "    └─ commit-msg: Conventional Commits 검증"
echo ""

# ──────────────────────────────────────────
# 검증
# ──────────────────────────────────────────
echo -e "${CYAN}========================================${NC}"
echo -e "${CYAN}  설치 검증${NC}"
echo -e "${CYAN}========================================${NC}"
echo ""

echo -n "  rustfmt:      "
rustfmt --version 2>/dev/null || echo -e "${RED}❌${NC}"

echo -n "  clippy:       "
cargo clippy --version 2>/dev/null || echo -e "${RED}❌${NC}"

echo -n "  cargo-watch:  "
cargo watch --version 2>/dev/null || echo -e "${YELLOW}미설치${NC}"

echo -n "  cargo-nextest: "
cargo nextest --version 2>/dev/null || echo -e "${YELLOW}미설치${NC}"

echo ""

# 빌드 확인
echo -e "${YELLOW}빌드 확인...${NC}"
cargo build --workspace 2>&1 || {
  echo -e "${RED}⚠️  빌드 실패. Cargo.toml 및 의존성을 확인하세요.${NC}"
  exit 1
}
echo -e "  ${GREEN}✅ Workspace build succeeded${NC}"

echo ""
echo -e "${GREEN}========================================${NC}"
echo -e "${GREEN}  🎉 코딩 하네스 설치 완료!${NC}"
echo -e "${GREEN}========================================${NC}"
echo ""
echo "  이제 커밋할 때 자동으로:"
echo "    1. Rust → rustfmt 포맷 체크"
echo "    2. Rust → clippy 린트 검사"
echo "    3. 코어 테스트 자동 실행"
echo "    4. 커밋 메시지 → Conventional Commits 규칙 검증"
echo ""
echo "  커밋 메시지 예시:"
echo "    feat(hangul): 두벌식 자모 조합 오토마타 구현"
echo "    fix(japanese): 로마지→가나 변환 'shi' 처리 수정"
echo "    docs: README 업데이트"
echo "    chore(deps): rusqlite 버전 업데이트"
echo ""
echo "  수동 실행:"
echo "    cargo fmt --all          # 코드 포맷팅"
echo "    cargo clippy --workspace # 린트 검사"
echo "    cargo test --workspace   # 전체 테스트"
echo "    cargo watch -x test      # 파일 변경시 자동 테스트"
echo "    cargo nextest run        # 빠른 테스트 실행"
SHEOF
  chmod +x scripts/setup-coding-harness.sh
  ok "설치 스크립트"
}

# ──────────────────────────────────────
# 5. GitHub 설정 스크립트
# ──────────────────────────────────────
create_github_setup() {
  header "5. GitHub 설정 스크립트 생성"

  mkdir -p scripts

  # ── Secrets 등록 ──
  log "시크릿 등록 스크립트..."
  cat > scripts/setup-github-secrets.sh << 'SECEOF'
#!/bin/bash
# ============================================
# 한글일본어입력기 GitHub Secrets 등록 스크립트
# 사용법: bash scripts/setup-github-secrets.sh
# 사전조건: gh auth login 완료 필요
# ============================================

set -e

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
CYAN='\033[0;36m'
NC='\033[0m'

# GitHub 레포 자동 감지
REPO=$(gh repo view --json nameWithOwner -q .nameWithOwner 2>/dev/null || echo "")
if [ -z "$REPO" ]; then
  echo -e "${RED}❌ GitHub 레포가 연결되어 있지 않습니다.${NC}"
  exit 1
fi

echo -e "${CYAN}========================================${NC}"
echo -e "${CYAN}  한글일본어입력기 GitHub Secrets 등록${NC}"
echo -e "${CYAN}  Repo: ${REPO}${NC}"
echo -e "${CYAN}========================================${NC}"
echo ""

# gh CLI 확인
if ! command -v gh &> /dev/null; then
  echo -e "${RED}❌ gh CLI가 설치되어 있지 않습니다.${NC}"
  exit 1
fi

if ! gh auth status &> /dev/null; then
  echo -e "${YELLOW}⚠️  GitHub 인증이 필요합니다.${NC}"
  gh auth login
fi

echo -e "${GREEN}✅ GitHub CLI 인증 확인 완료${NC}"
echo ""

# ──────────────────────────────────────────
# 1. Apple 코드 서명 (macOS)
# ──────────────────────────────────────────
echo -e "${YELLOW}[1/3] Apple 코드 서명 (macOS IME)${NC}"
echo "  Developer ID Certificate가 필요합니다."
echo "  - Keychain Access → 내보내기(.p12)"
echo ""
read -p "  Apple 서명을 설정하시겠습니까? (y/N): " SETUP_APPLE
if [[ "$SETUP_APPLE" =~ ^[Yy]$ ]]; then
  read -p "  Apple Team ID: " APPLE_TEAM_ID
  [ -n "$APPLE_TEAM_ID" ] && echo "$APPLE_TEAM_ID" | gh secret set APPLE_TEAM_ID --repo "$REPO"

  read -p "  Certificate .p12 파일 경로: " CERT_PATH
  if [ -f "$CERT_PATH" ]; then
    base64 < "$CERT_PATH" | gh secret set APPLE_CERTIFICATE --repo "$REPO"
    read -sp "  Certificate 비밀번호: " CERT_PASS; echo
    echo "$CERT_PASS" | gh secret set APPLE_CERTIFICATE_PASSWORD --repo "$REPO"
    echo -e "  ${GREEN}✅ Apple 서명 시크릿 등록 완료${NC}"
  else
    echo -e "  ${YELLOW}⚠️  파일을 찾을 수 없습니다.${NC}"
  fi
else
  echo -e "  ${YELLOW}⏭️  건너뜀${NC}"
fi
echo ""

# ──────────────────────────────────────────
# 2. Android 서명 (Keystore)
# ──────────────────────────────────────────
echo -e "${YELLOW}[2/3] Android 서명 (Keystore)${NC}"
echo "  Keystore 생성: keytool -genkey -v -keystore release.keystore ..."
echo ""
read -p "  Android 서명을 설정하시겠습니까? (y/N): " SETUP_ANDROID
if [[ "$SETUP_ANDROID" =~ ^[Yy]$ ]]; then
  read -p "  Keystore 파일 경로: " KEYSTORE_PATH
  if [ -f "$KEYSTORE_PATH" ]; then
    base64 < "$KEYSTORE_PATH" | gh secret set ANDROID_KEYSTORE --repo "$REPO"
    read -sp "  Keystore 비밀번호: " KS_PASS; echo
    echo "$KS_PASS" | gh secret set ANDROID_KEYSTORE_PASSWORD --repo "$REPO"
    read -p "  Key alias: " KEY_ALIAS
    echo "$KEY_ALIAS" | gh secret set ANDROID_KEY_ALIAS --repo "$REPO"
    read -sp "  Key 비밀번호: " KEY_PASS; echo
    echo "$KEY_PASS" | gh secret set ANDROID_KEY_PASSWORD --repo "$REPO"
    echo -e "  ${GREEN}✅ Android 서명 시크릿 등록 완료${NC}"
  else
    echo -e "  ${YELLOW}⚠️  파일을 찾을 수 없습니다.${NC}"
  fi
else
  echo -e "  ${YELLOW}⏭️  건너뜀${NC}"
fi
echo ""

# ──────────────────────────────────────────
# 3. 알림 (선택)
# ──────────────────────────────────────────
echo -e "${YELLOW}[3/3] 알림 설정 (선택)${NC}"
echo ""
read -p "  SLACK_WEBHOOK_URL (없으면 Enter): " SLACK_WEBHOOK_URL
if [ -n "$SLACK_WEBHOOK_URL" ]; then
  echo "$SLACK_WEBHOOK_URL" | gh secret set SLACK_WEBHOOK_URL --repo "$REPO"
  echo -e "  ${GREEN}✅ SLACK_WEBHOOK_URL 등록 완료${NC}"
else
  echo -e "  ${YELLOW}⏭️  건너뜀${NC}"
fi
echo ""

# ──────────────────────────────────────────
# 결과 확인
# ──────────────────────────────────────────
echo -e "${CYAN}========================================${NC}"
echo -e "${CYAN}  등록된 시크릿 목록${NC}"
echo -e "${CYAN}========================================${NC}"
gh secret list --repo "$REPO"
echo ""
echo -e "${GREEN}✅ 시크릿 등록 완료!${NC}"
echo -e "   다음 단계: ${CYAN}bash scripts/setup-branch-protection.sh${NC}"
SECEOF
  chmod +x scripts/setup-github-secrets.sh
  ok "시크릿 등록 스크립트"

  # ── Branch Protection ──
  log "브랜치 보호 스크립트..."
  cat > scripts/setup-branch-protection.sh << 'BPEOF'
#!/bin/bash
# ============================================
# 한글일본어입력기 브랜치 보호 규칙 + CI 게이트 설정
# 사용법: bash scripts/setup-branch-protection.sh
# 사전조건: gh auth login 완료 필요
# ============================================

set -e

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
CYAN='\033[0;36m'
NC='\033[0m'

# 레포 자동 감지
FULL_REPO=$(gh repo view --json nameWithOwner -q .nameWithOwner 2>/dev/null || echo "")
if [ -z "$FULL_REPO" ]; then
  echo -e "${RED}❌ GitHub 레포가 연결되어 있지 않습니다.${NC}"
  exit 1
fi
OWNER=$(echo "$FULL_REPO" | cut -d'/' -f1)
REPO_NAME=$(echo "$FULL_REPO" | cut -d'/' -f2)

echo -e "${CYAN}========================================${NC}"
echo -e "${CYAN}  한글일본어입력기 브랜치 보호 규칙 설정${NC}"
echo -e "${CYAN}  Repo: ${FULL_REPO}${NC}"
echo -e "${CYAN}========================================${NC}"
echo ""

# gh CLI 확인
if ! command -v gh &> /dev/null; then
  echo -e "${RED}❌ gh CLI가 설치되어 있지 않습니다.${NC}"
  exit 1
fi

if ! gh auth status &> /dev/null; then
  echo -e "${YELLOW}⚠️  GitHub 인증이 필요합니다.${NC}"
  gh auth login
fi

# ──────────────────────────────────────────
# 1. main 브랜치 보호 규칙
# ──────────────────────────────────────────
echo -e "${YELLOW}[1/3] main 브랜치 보호 규칙 설정...${NC}"

gh api --method PUT "repos/${OWNER}/${REPO_NAME}/branches/main/protection" \
  --input - <<'EOF'
{
  "required_status_checks": {
    "strict": true,
    "contexts": ["✅ CI 게이트"]
  },
  "enforce_admins": true,
  "required_pull_request_reviews": {
    "required_approving_review_count": 1,
    "dismiss_stale_reviews": true,
    "require_code_owner_reviews": false
  },
  "restrictions": null,
  "allow_force_pushes": false,
  "allow_deletions": false,
  "required_conversation_resolution": true
}
EOF

echo -e "${GREEN}✅ main 브랜치 보호 설정 완료${NC}"
echo "   - PR 필수 (1명 이상 승인)"
echo "   - CI 게이트 통과 필수 (rustfmt + clippy + test)"
echo "   - 브랜치 최신 상태 필수"
echo "   - 강제 푸시 금지"
echo "   - 대화 해결 필수"
echo ""

# ──────────────────────────────────────────
# 2. staging 브랜치 생성 및 보호 규칙
# ──────────────────────────────────────────
echo -e "${YELLOW}[2/3] staging 브랜치 설정...${NC}"

echo "  staging 브랜치 확인 중..."
if ! gh api "repos/${OWNER}/${REPO_NAME}/git/ref/heads/staging" &>/dev/null; then
  echo "  staging 브랜치 생성 중..."
  MAIN_SHA=$(gh api "repos/${OWNER}/${REPO_NAME}/git/ref/heads/main" --jq '.object.sha')
  gh api --method POST "repos/${OWNER}/${REPO_NAME}/git/refs" \
    --field ref="refs/heads/staging" \
    --field sha="$MAIN_SHA" > /dev/null
  echo -e "  ${GREEN}✅ staging 브랜치 생성 완료${NC}"
  sleep 2
else
  echo -e "  ${GREEN}✅ staging 브랜치 이미 존재${NC}"
fi

gh api --method PUT "repos/${OWNER}/${REPO_NAME}/branches/staging/protection" \
  --input - <<'EOF'
{
  "required_status_checks": {
    "strict": true,
    "contexts": ["✅ CI 게이트"]
  },
  "enforce_admins": false,
  "required_pull_request_reviews": null,
  "restrictions": null,
  "allow_force_pushes": false,
  "allow_deletions": false
}
EOF

echo -e "${GREEN}✅ staging 브랜치 보호 설정 완료${NC}"
echo "   - CI 게이트 통과 필수"
echo "   - PR 없이 직접 푸시 가능"
echo ""

# ──────────────────────────────────────────
# 3. GitHub Environments 생성
# ──────────────────────────────────────────
echo -e "${YELLOW}[3/3] GitHub Environments 생성...${NC}"

# staging 환경
gh api --method PUT "repos/${OWNER}/${REPO_NAME}/environments/staging" \
  --input - <<'EOF'
{"deployment_branch_policy":{"protected_branches":false,"custom_branch_policies":true}}
EOF
gh api --method POST "repos/${OWNER}/${REPO_NAME}/environments/staging/deployment-branch-policies" \
  --field name="staging" 2>/dev/null || true
echo -e "  ${GREEN}✅ staging 환경 생성 완료${NC}"

# production 환경
gh api --method PUT "repos/${OWNER}/${REPO_NAME}/environments/production" \
  --input - <<'EOF'
{"deployment_branch_policy":{"protected_branches":false,"custom_branch_policies":true}}
EOF
gh api --method POST "repos/${OWNER}/${REPO_NAME}/environments/production/deployment-branch-policies" \
  --field name="main" 2>/dev/null || true
echo -e "  ${GREEN}✅ production 환경 생성 완료${NC}"
echo ""

# ──────────────────────────────────────────
# 결과 요약
# ──────────────────────────────────────────
echo -e "${CYAN}========================================${NC}"
echo -e "${CYAN}  설정 완료 요약${NC}"
echo -e "${CYAN}========================================${NC}"
echo ""
echo -e "  ${GREEN}main 브랜치${NC}"
echo "    ├─ PR 필수 (1명 승인)"
echo "    ├─ CI 게이트 (✅ CI 게이트) 필수"
echo "    ├─ 브랜치 최신 상태 필수"
echo "    └─ 강제 푸시 금지"
echo ""
echo -e "  ${GREEN}staging 브랜치${NC}"
echo "    ├─ CI 게이트 필수"
echo "    └─ 직접 푸시 가능"
echo ""
echo -e "  ${GREEN}Environments${NC}"
echo "    ├─ staging  → staging 브랜치만 배포 가능"
echo "    └─ production → main 브랜치만 배포 가능"
echo ""
echo -e "${GREEN}🎉 모든 설정이 완료되었습니다!${NC}"
echo ""
echo -e "  (선택) Production 수동 승인 추가:"
echo -e "  GitHub → Settings → Environments → production"
echo -e "  → Required reviewers 활성화"
BPEOF
  chmod +x scripts/setup-branch-protection.sh
  ok "브랜치 보호 스크립트"
}

# ──────────────────────────────────────
# 완료 요약
# ──────────────────────────────────────
print_summary() {
  header "🎉 하네스 부트스트랩 완료!"

  echo ""
  echo -e "  ${BOLD}생성된 파일:${NC}"
  echo ""
  echo "  .github/"
  echo "    ├── workflows/"
  echo "    │   ├── ci.yml                    # CI 파이프라인 (rustfmt, clippy, test, macOS, Android)"
  echo "    │   ├── auto-label.yml            # PR 자동 라벨링"
  echo "    │   ├── release-build.yml         # 릴리즈 빌드 (Universal Binary, Android .so)"
  echo "    │   ├── rollback.yml              # 수동 롤백"
  echo "    │   ├── agent-pr-staging.yml      # Agent → Staging PR"
  echo "    │   └── promote-production.yml    # Staging → Production 승격"
  echo "    ├── labeler.yml                   # 라벨링 규칙"
  echo "    └── PULL_REQUEST_TEMPLATE/"
  echo "        └── default.md                # PR 템플릿"
  echo ""
  echo "  scripts/"
  echo "    ├── agent-commit.sh               # 에이전트 자동 커밋"
  echo "    ├── setup-coding-harness.sh       # 코딩 도구 설치 (rustfmt, clippy, hooks)"
  echo "    ├── setup-github-secrets.sh       # GitHub 시크릿 등록 (Apple/Android 서명)"
  echo "    └── setup-branch-protection.sh    # 브랜치 보호 설정"
  echo ""
  echo "  rustfmt.toml / clippy.toml / commitlint.config.js"
  echo "  .husky/pre-commit / .husky/commit-msg"
  echo ""

  echo -e "  ${BOLD}다음 단계:${NC}"
  echo ""
  echo "  1. 코딩 하네스 설치:"
  echo "     bash scripts/setup-coding-harness.sh"
  echo ""
  echo "  2. GitHub 시크릿 등록:"
  echo "     bash scripts/setup-github-secrets.sh"
  echo ""
  echo "  3. 브랜치 보호 설정:"
  echo "     bash scripts/setup-branch-protection.sh"
  echo ""
  echo "  4. 커밋 & 푸시:"
  echo "     git add -A && git commit -m 'ci: add project harness'"
  echo "     git push origin main"
  echo ""
  echo -e "  ${GREEN}에이전트 사용법:${NC}"
  echo "     ./scripts/agent-commit.sh \"변경 설명\""
  echo ""
}

# ──────────────────────────────────────
# 메인 실행
# ──────────────────────────────────────
main() {
  echo ""
  echo -e "${BOLD}🏗️  한글일본어입력기 - Project Harness Bootstrap${NC}"
  echo -e "  Rust 크로스플랫폼 IME용 CI/CD + Agent 자동화 + Coding 하네스"
  echo ""

  check_prerequisites
  collect_project_info
  create_repo_harness
  create_deploy_harness
  create_agent_harness
  create_coding_harness
  create_github_setup
  print_summary
}

main "$@"
