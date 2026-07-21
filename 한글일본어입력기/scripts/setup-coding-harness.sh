#!/usr/bin/env bash
set -euo pipefail

echo "=== 한글일본어입력기 - 개발환경 하네스 설정 ==="
echo ""

# ──────────────────────────────────────────────
# 1. Rust toolchain 확인
# ──────────────────────────────────────────────
echo "🔧 Rust toolchain 확인..."
if ! command -v rustup &>/dev/null; then
    echo "  rustup이 설치되어 있지 않습니다. 설치 중..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
fi

rustup update stable
rustup component add rustfmt clippy
echo "  ✅ Rust $(rustc --version)"

# ──────────────────────────────────────────────
# 2. Android 크로스컴파일 타겟 추가
# ──────────────────────────────────────────────
echo ""
echo "📱 Android 크로스컴파일 타겟 추가..."
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
echo "  ✅ Android targets added"

# ──────────────────────────────────────────────
# 3. cargo-watch, cargo-nextest 설치 (개발 편의)
# ──────────────────────────────────────────────
echo ""
echo "📦 개발 도구 설치..."
if ! command -v cargo-watch &>/dev/null; then
    cargo install cargo-watch
fi
if ! command -v cargo-nextest &>/dev/null; then
    cargo install cargo-nextest --locked
fi
echo "  ✅ cargo-watch, cargo-nextest"

# ──────────────────────────────────────────────
# 4. Git hooks (pre-commit) 설정
# ──────────────────────────────────────────────
echo ""
echo "🪝 Git pre-commit hook 설정..."
HOOK_DIR="$(git rev-parse --show-toplevel)/.git/hooks"
mkdir -p "$HOOK_DIR"

cat > "$HOOK_DIR/pre-commit" << 'HOOK'
#!/usr/bin/env bash
set -euo pipefail

echo "🔍 Pre-commit checks..."

# Format check
echo "  [1/3] rustfmt..."
cargo fmt --all -- --check || {
    echo "❌ Formatting issues found. Run 'cargo fmt --all' to fix."
    exit 1
}

# Clippy
echo "  [2/3] clippy..."
cargo clippy --workspace --all-targets -- -D warnings || {
    echo "❌ Clippy warnings found. Please fix before committing."
    exit 1
}

# Tests (core crates only for speed)
echo "  [3/3] tests (core)..."
cargo test -p ime-core -p ime-hangul -p ime-db --quiet || {
    echo "❌ Tests failed. Please fix before committing."
    exit 1
}

echo "✅ All pre-commit checks passed!"
HOOK

chmod +x "$HOOK_DIR/pre-commit"
echo "  ✅ pre-commit hook installed"

# ──────────────────────────────────────────────
# 5. 빌드 확인
# ──────────────────────────────────────────────
echo ""
echo "🔨 빌드 확인..."
cargo build --workspace 2>&1 || {
    echo "⚠️  빌드 실패. Cargo.toml 및 의존성을 확인하세요."
    exit 1
}
echo "  ✅ Workspace build succeeded"

echo ""
echo "🎉 개발환경 하네스 설정 완료!"
echo ""
echo "사용 가능한 명령어:"
echo "  cargo fmt --all          # 코드 포맷팅"
echo "  cargo clippy --workspace # 린트 검사"
echo "  cargo test --workspace   # 전체 테스트"
echo "  cargo watch -x test      # 파일 변경시 자동 테스트"
echo "  cargo nextest run        # 빠른 테스트 실행"
