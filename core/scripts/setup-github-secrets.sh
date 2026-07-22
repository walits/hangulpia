#!/usr/bin/env bash
set -euo pipefail

echo "=== GitHub Secrets 설정 ==="
echo ""

# GitHub CLI 확인
if ! command -v gh &>/dev/null; then
    echo "❌ GitHub CLI(gh)가 설치되어 있지 않습니다."
    echo "   설치: https://cli.github.com/"
    exit 1
fi

# 로그인 확인
if ! gh auth status &>/dev/null; then
    echo "❌ GitHub에 로그인되어 있지 않습니다."
    echo "   실행: gh auth login"
    exit 1
fi

REPO=$(gh repo view --json nameWithOwner -q .nameWithOwner 2>/dev/null || echo "")
if [ -z "$REPO" ]; then
    echo "❌ GitHub 레포가 연결되어 있지 않습니다."
    echo "   먼저 'git remote add origin <url>' 후 다시 실행하세요."
    exit 1
fi

echo "레포: $REPO"
echo ""

# ──────────────────────────────────────────────
# 필요한 시크릿 등록
# ──────────────────────────────────────────────

# Android signing (선택사항)
echo "📱 Android 서명 키 설정 (선택사항)"
read -rp "Android keystore base64를 등록하시겠습니까? (y/N): " SETUP_ANDROID
if [[ "$SETUP_ANDROID" =~ ^[Yy]$ ]]; then
    read -rp "Keystore 파일 경로: " KEYSTORE_PATH
    if [ -f "$KEYSTORE_PATH" ]; then
        KEYSTORE_B64=$(base64 < "$KEYSTORE_PATH")
        gh secret set ANDROID_KEYSTORE --body "$KEYSTORE_B64" --repo "$REPO"
        read -rsp "Keystore 비밀번호: " KS_PASS; echo
        gh secret set ANDROID_KEYSTORE_PASSWORD --body "$KS_PASS" --repo "$REPO"
        read -rp "Key alias: " KEY_ALIAS
        gh secret set ANDROID_KEY_ALIAS --body "$KEY_ALIAS" --repo "$REPO"
        read -rsp "Key 비밀번호: " KEY_PASS; echo
        gh secret set ANDROID_KEY_PASSWORD --body "$KEY_PASS" --repo "$REPO"
        echo "  ✅ Android 서명 시크릿 등록 완료"
    else
        echo "  ⚠️  파일을 찾을 수 없습니다: $KEYSTORE_PATH"
    fi
fi

# macOS signing (선택사항)
echo ""
echo "🍎 macOS 코드 서명 설정 (선택사항)"
read -rp "macOS Developer ID를 등록하시겠습니까? (y/N): " SETUP_MACOS
if [[ "$SETUP_MACOS" =~ ^[Yy]$ ]]; then
    read -rp "Apple Developer Team ID: " TEAM_ID
    gh secret set APPLE_TEAM_ID --body "$TEAM_ID" --repo "$REPO"
    read -rp "Certificate base64 (p12): " CERT_PATH
    if [ -f "$CERT_PATH" ]; then
        CERT_B64=$(base64 < "$CERT_PATH")
        gh secret set APPLE_CERTIFICATE --body "$CERT_B64" --repo "$REPO"
        read -rsp "Certificate 비밀번호: " CERT_PASS; echo
        gh secret set APPLE_CERTIFICATE_PASSWORD --body "$CERT_PASS" --repo "$REPO"
    fi
    echo "  ✅ macOS 서명 시크릿 등록 완료"
fi

echo ""
echo "🎉 GitHub Secrets 설정 완료!"
