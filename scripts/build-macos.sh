#!/usr/bin/env bash
# build-macos.sh — Build the Squashbox macOS app + FSKit extension.
#
# This script:
# 1. Builds the Rust static libraries (squashbox-core, squashbox-macos, macos-fskit)
# 2. Compiles the Swift FSKit extension, linking the Rust static libs
# 3. Compiles the Swift host app
# 4. Assembles the .app bundle with embedded .appex
#
# Usage:
#   ./scripts/build-macos.sh [--release]
#
# Output:
#   build/macos/Squashbox.app/
#   └── Contents/
#       ├── MacOS/Squashbox
#       ├── Info.plist
#       └── PlugIns/
#           └── SquashboxFS.appex/
#               ├── Contents/
#               │   ├── MacOS/SquashboxFS
#               │   └── Info.plist

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
BUILD_DIR="$ROOT_DIR/build/macos"

# Parse args
PROFILE="debug"
CARGO_FLAGS=""
if [[ "${1:-}" == "--release" ]]; then
    PROFILE="release"
    CARGO_FLAGS="--release"
fi

RUST_TARGET_DIR="$ROOT_DIR/target/$PROFILE"
ARCH="$(uname -m)"  # arm64 or x86_64

echo "╔══════════════════════════════════════════════════╗"
echo "║       Squashbox macOS Build                      ║"
echo "╚══════════════════════════════════════════════════╝"
echo ""
echo "  Profile:    $PROFILE"
echo "  Arch:       $ARCH"
echo "  Output:     $BUILD_DIR/Squashbox.app"
echo ""

# ── Step 1: Build Rust static libraries ──────────────────────────
echo "▸ Building Rust static libraries..."
cd "$ROOT_DIR"
cargo build -p squashbox-macos -p macos-fskit -p squashbox-core $CARGO_FLAGS

# Find the static libraries
LIB_SQUASHBOX_MACOS="$RUST_TARGET_DIR/libsquashbox_macos.a"
LIB_SQUASHBOX_CORE="$RUST_TARGET_DIR/libsquashbox_core.a"

if [[ ! -f "$LIB_SQUASHBOX_MACOS" ]]; then
    echo "ERROR: $LIB_SQUASHBOX_MACOS not found"
    echo "  Checking for available libraries..."
    ls -la "$RUST_TARGET_DIR"/lib*.a 2>/dev/null || echo "  No .a files found"
    exit 1
fi

echo "  ✓ libsquashbox_macos.a"
echo "  ✓ libsquashbox_core.a"

# ── Step 2: Prepare output directories ───────────────────────────
echo "▸ Preparing bundle directories..."
APPEX_DIR="$BUILD_DIR/Squashbox.app/Contents/PlugIns/SquashboxFS.appex/Contents"
APP_DIR="$BUILD_DIR/Squashbox.app/Contents"

rm -rf "$BUILD_DIR/Squashbox.app"
mkdir -p "$APPEX_DIR/MacOS"
mkdir -p "$APP_DIR/MacOS"

# ── Step 3: Compile FSKit extension ──────────────────────────────
echo "▸ Compiling FSKit extension (Swift)..."

SWIFT_FLAGS=(
    -O
    -target "${ARCH}-apple-macos15.4"
    -sdk "$(xcrun --show-sdk-path)"
    -I "$ROOT_DIR/macos/include"         # module map + C header
    -L "$RUST_TARGET_DIR"                # Rust static libs
    -lsquashbox_macos                    # our adapter + FFI
    -lsquashbox_core                     # core provider
    -framework FSKit
    -framework Foundation
    -framework ExtensionFoundation
    -framework ExtensionKit
    -parse-as-library
    -module-name SquashboxFS
    -emit-executable
    -o "$APPEX_DIR/MacOS/SquashboxFS"
)

swiftc "${SWIFT_FLAGS[@]}" \
    "$ROOT_DIR/macos/SquashboxFS/SquashboxFSExtension.swift"

echo "  ✓ SquashboxFS extension compiled"

# Copy extension Info.plist
cp "$ROOT_DIR/macos/SquashboxFS/Info.plist" "$APPEX_DIR/Info.plist"
echo "  ✓ Extension Info.plist"

# ── Step 4: Compile host app ─────────────────────────────────────
echo "▸ Compiling host app (Swift)..."

swiftc \
    -O \
    -target "${ARCH}-apple-macos15.4" \
    -sdk "$(xcrun --show-sdk-path)" \
    -framework SwiftUI \
    -framework AppKit \
    -parse-as-library \
    -module-name Squashbox \
    -emit-executable \
    -o "$APP_DIR/MacOS/Squashbox" \
    "$ROOT_DIR/macos/Squashbox/SquashboxApp.swift"

echo "  ✓ Squashbox host app compiled"

# Copy host app Info.plist
cp "$ROOT_DIR/macos/Squashbox/Info.plist" "$APP_DIR/Info.plist"
echo "  ✓ Host app Info.plist"

# ── Step 5: Code sign ────────────────────────────────────────────
echo "▸ Code signing..."

# Find available signing identity (prefer Apple Development)
SIGNING_IDENTITY=$(security find-identity -v -p codesigning | grep -E "Apple Development|Mac Developer|Developer ID Application" | head -n 1 | awk -F'"' '{print $2}')
if [[ -z "$SIGNING_IDENTITY" ]]; then
    SIGNING_IDENTITY="-"
    echo "  ⚠ No Apple Developer identity found. Using ad-hoc signature (-)"
else
    echo "  ✓ Found signing identity: $SIGNING_IDENTITY"
fi

# Sign the extension first (inner → outer)
codesign --force --sign "$SIGNING_IDENTITY" \
    --entitlements "$ROOT_DIR/macos/SquashboxFS/SquashboxFS.entitlements" \
    "$BUILD_DIR/Squashbox.app/Contents/PlugIns/SquashboxFS.appex" \
    2>/dev/null || echo "  ⚠ Extension code signing failed"

# Sign the host app
codesign --force --sign "$SIGNING_IDENTITY" \
    --entitlements "$ROOT_DIR/macos/Squashbox/Squashbox.entitlements" \
    "$BUILD_DIR/Squashbox.app" \
    2>/dev/null || echo "  ⚠ App code signing failed"

echo "  ✓ Code signed with: $SIGNING_IDENTITY"

# ── Done ─────────────────────────────────────────────────────────
echo ""
echo "╔══════════════════════════════════════════════════╗"
echo "║              Build Complete ✓                    ║"
echo "╚══════════════════════════════════════════════════╝"
echo ""
echo "  Output: $BUILD_DIR/Squashbox.app"
echo ""
echo "  To install:"
echo "    sqb install"
echo "    — or —"
echo "    sudo cp -R $BUILD_DIR/Squashbox.app /Applications/"
echo ""
echo "  Then enable in:"
echo "    System Settings → General → Login Items → File System Extensions"
