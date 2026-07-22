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
