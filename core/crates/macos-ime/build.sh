#!/bin/bash
#
# build.sh — Build the HangulJapaneseIME macOS input method
#
# This script:
#   1. Builds the Rust engine as a static library (libhj_engine.a)
#   2. Compiles the Swift InputMethodKit app
#   3. Links them together into an .app bundle
#   4. Optionally installs to ~/Library/Input Methods/
#
# Usage:
#   ./build.sh              # Build only
#   ./build.sh install      # Build + install
#   ./build.sh clean        # Clean build artifacts
#

set -euo pipefail

# ── Ensure cargo is in PATH ─────────────────────────────────
if ! command -v cargo &>/dev/null; then
    if [ -f "$HOME/.cargo/env" ]; then
        source "$HOME/.cargo/env"
    elif [ -d "$HOME/.cargo/bin" ]; then
        export PATH="$HOME/.cargo/bin:$PATH"
    else
        echo "❌ cargo not found. Install Rust first: https://rustup.rs"
        exit 1
    fi
fi

# ── Configuration ────────────────────────────────────────────
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
CRATE_DIR="$SCRIPT_DIR"
APP_DIR="$CRATE_DIR/HangulJapaneseIME"
BUILD_DIR="$CRATE_DIR/build"
APP_NAME="HangulJapaneseIME"
BUNDLE_ID="com.hkd.inputmethod.HangulJapanese"

# Detect architecture
ARCH=$(uname -m)
if [ "$ARCH" = "arm64" ]; then
    RUST_TARGET="aarch64-apple-darwin"
else
    RUST_TARGET="x86_64-apple-darwin"
fi

# ── Functions ────────────────────────────────────────────────

generate_icon() {
    local RESOURCES_DIR="$1"
    swift - "$RESOURCES_DIR" <<'ICON_SWIFT'
import Cocoa
import Foundation

let resourcesDir = CommandLine.arguments[1]

func createMenuBarIcon(size: CGFloat, label: String) -> NSImage {
    let img = NSImage(size: NSSize(width: size, height: size))
    img.lockFocus()

    let fontSize = size * 0.48
    let font = NSFont(name: "HiraginoSans-W6", size: fontSize)
        ?? NSFont.systemFont(ofSize: fontSize, weight: .semibold)
    let attrs: [NSAttributedString.Key: Any] = [
        .font: font,
        .foregroundColor: NSColor.black,
    ]
    let str = label as NSString
    let textSize = str.size(withAttributes: attrs)
    let x = (size - textSize.width) / 2
    let y = (size - textSize.height) / 2
    str.draw(at: NSPoint(x: x, y: y), withAttributes: attrs)

    img.unlockFocus()
    return img
}

// Menu bar icon: "한あ"
let icon = createMenuBarIcon(size: 16, label: "한あ")
if let data = icon.tiffRepresentation {
    try? data.write(to: URL(fileURLWithPath: "\(resourcesDir)/ime_icon.tiff"))
}

// App icon (larger)
let appIcon = createMenuBarIcon(size: 128, label: "한あ")
if let data = appIcon.tiffRepresentation {
    try? data.write(to: URL(fileURLWithPath: "\(resourcesDir)/AppIcon.tiff"))
}
ICON_SWIFT
}

clean() {
    echo "🧹 Cleaning..."
    rm -rf "$BUILD_DIR"
    echo "   Done."
}

build_rust() {
    echo "🦀 Building Rust engine (release, $RUST_TARGET)..."
    cd "$PROJECT_ROOT"
    cargo build --release -p ime-macos --target "$RUST_TARGET"

    local LIB_PATH="$PROJECT_ROOT/target/$RUST_TARGET/release/libhj_engine.a"
    if [ ! -f "$LIB_PATH" ]; then
        echo "❌ Static library not found at $LIB_PATH"
        exit 1
    fi
    echo "   ✅ libhj_engine.a built"
}

build_swift() {
    echo "🍎 Building Swift InputMethodKit app..."

    mkdir -p "$BUILD_DIR/$APP_NAME.app/Contents/MacOS"
    mkdir -p "$BUILD_DIR/$APP_NAME.app/Contents/Resources"

    # Copy Info.plist
    cp "$APP_DIR/Resources/Info.plist" "$BUILD_DIR/$APP_NAME.app/Contents/"

    # Generate menu bar icon (あ badge)
    echo "   🎨 Generating menu bar icon..."
    generate_icon "$BUILD_DIR/$APP_NAME.app/Contents/Resources"

    local LIB_PATH="$PROJECT_ROOT/target/$RUST_TARGET/release/libhj_engine.a"
    local HEADER_DIR="$CRATE_DIR/include"
    local SOURCES_DIR="$APP_DIR/Sources"

    # Find Swift source files
    local SWIFT_FILES=(
        "$SOURCES_DIR/main.swift"
        "$SOURCES_DIR/AppDelegate.swift"
        "$SOURCES_DIR/HJEngine.swift"
        "$SOURCES_DIR/HJInputController.swift"
        "$SOURCES_DIR/CandidateWindowController.swift"
    )

    # Compile Swift → executable, linking with Rust static library
    swiftc \
        -target "${ARCH}-apple-macosx13.0" \
        -sdk "$(xcrun --show-sdk-path)" \
        -import-objc-header "$SOURCES_DIR/BridgingHeader.h" \
        -I "$HEADER_DIR" \
        -L "$(dirname "$LIB_PATH")" \
        -lhj_engine \
        -framework Cocoa \
        -framework Carbon \
        -framework InputMethodKit \
        -framework Security \
        "${SWIFT_FILES[@]}" \
        -o "$BUILD_DIR/$APP_NAME.app/Contents/MacOS/$APP_NAME"

    echo "   ✅ $APP_NAME.app built"
}

install_ime() {
    local INSTALL_DIR="$HOME/Library/Input Methods"
    echo "📦 Installing to $INSTALL_DIR..."

    # Kill existing instance
    killall "$APP_NAME" 2>/dev/null || true
    sleep 1

    # Remove old version
    rm -rf "$INSTALL_DIR/$APP_NAME.app"

    # Copy new version
    cp -R "$BUILD_DIR/$APP_NAME.app" "$INSTALL_DIR/"

    # Ad-hoc code sign (required for macOS to trust the IME)
    echo "🔏 Code signing..."
    codesign --force --deep --sign - "$INSTALL_DIR/$APP_NAME.app" 2>/dev/null || true

    # Clear TIS input source cache so macOS re-scans
    echo "🔄 Clearing input source cache..."
    killall SystemUIServer 2>/dev/null || true

    # Launch the IME process so macOS can detect it
    echo "🚀 Launching IME process..."
    open "$INSTALL_DIR/$APP_NAME.app"

    echo ""
    echo "   ✅ Installed & launched!"
    echo ""
    echo "   ┌──────────────────────────────────────────────────────┐"
    echo "   │  다음 단계:                                           │"
    echo "   │                                                       │"
    echo "   │  1. 시스템 설정 → 키보드 → 입력 소스 편집 → + 클릭    │"
    echo "   │  2. 왼쪽 목록에서 '일본어' 카테고리 확인              │"
    echo "   │  3. '한글일본어입력기' 선택하여 추가                   │"
    echo "   │  4. 메뉴바에서 입력 소스를 전환하여 사용               │"
    echo "   │                                                       │"
    echo "   │  ⚠️  안 보이면 로그아웃 후 재로그인 해보세요           │"
    echo "   └──────────────────────────────────────────────────────┘"
}

# ── Main ─────────────────────────────────────────────────────

case "${1:-build}" in
    clean)
        clean
        ;;
    build)
        build_rust
        build_swift
        echo ""
        echo "✅ Build complete: $BUILD_DIR/$APP_NAME.app"
        echo "   Run './build.sh install' to install."
        ;;
    install)
        build_rust
        build_swift
        install_ime
        ;;
    *)
        echo "Usage: $0 {build|install|clean}"
        exit 1
        ;;
esac
