#!/bin/sh

# ==================================================
# THIS SCRIPT CURRENTLY NOT WORKING, NEED TO FIX 
# ==================================================

set -e

# Default to x86_64-pc-windows-msvc
TARGET=${1:-"x86_64-pc-windows-msvc"}

echo "Building crane for Windows ($TARGET)"

# Ensure cross is installed
if ! command -v cross >/dev/null 2>&1; then
    echo "Error: 'cross' command not found. Please run ./devsetup.sh first."
    exit 1
fi

# Build binary
echo "Running cross build..."
cross build --release --locked --target "$TARGET"

# Package
BIN="target/$TARGET/release/crane.exe"
if [ ! -f "$BIN" ]; then
    echo "Error: Binary not found at $BIN"
    exit 1
fi

# Retrieve version from git or Cargo.toml
VERSION=$(git describe --tags --always 2>/dev/null || cargo metadata --no-deps --format-version 1 | grep -o '"version":"[^"]*"' | head -n 1 | cut -d'"' -f4)
ARCHIVE="crane-$VERSION-$TARGET.zip"

echo "Packaging $BIN into $ARCHIVE..."
if command -v zip >/dev/null 2>&1; then
    # -j options junk paths, so only the file is added to the zip
    zip -j "$ARCHIVE" "$BIN"
else
    echo "Warning: 'zip' command not found, fallback to creating a tarball (.tar.gz)."
    ARCHIVE="crane-$VERSION-$TARGET.tar.gz"
    tar -czf "$ARCHIVE" -C "$(dirname "$BIN")" "$(basename "$BIN")"
fi

echo "Windows binary successfully built and packaged at $ARCHIVE"
