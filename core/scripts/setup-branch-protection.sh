#!/usr/bin/env bash
set -euo pipefail

echo "=== 브랜치 보호 규칙 설정 ==="
echo ""

# GitHub CLI 확인
if ! command -v gh &>/dev/null; then
    echo "❌ GitHub CLI(gh)가 필요합니다."
    exit 1
fi

REPO=$(gh repo view --json nameWithOwner -q .nameWithOwner 2>/dev/null || echo "")
if [ -z "$REPO" ]; then
    echo "❌ GitHub 레포가 연결되어 있지 않습니다."
    exit 1
fi

echo "레포: $REPO"
echo ""

# ──────────────────────────────────────────────
# main 브랜치 보호 규칙
# ──────────────────────────────────────────────
echo "🔒 main 브랜치 보호 규칙 설정..."

gh api repos/"$REPO"/branches/main/protection \
    --method PUT \
    --input - << 'JSON'
{
    "required_status_checks": {
        "strict": true,
        "contexts": ["Check & Lint"]
    },
    "enforce_admins": false,
    "required_pull_request_reviews": {
        "required_approving_review_count": 1,
        "dismiss_stale_reviews": true
    },
    "restrictions": null,
    "required_linear_history": true,
    "allow_force_pushes": false,
    "allow_deletions": false
}
JSON

echo "  ✅ main 브랜치 보호 규칙 적용 완료"
echo ""
echo "적용된 규칙:"
echo "  - PR 필수 (최소 1명 승인)"
echo "  - CI 통과 필수 (Check & Lint job)"
echo "  - Stale 리뷰 자동 해제"
echo "  - Linear history 강제 (rebase merge only)"
echo "  - Force push / 삭제 금지"
