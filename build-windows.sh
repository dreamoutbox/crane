#!/bin/sh

# ==================================================
# THIS SCRIPT CURRENTLY NOT WORKING, NEED TO FIX 
# ==================================================

set -e

# Default to x86_64-pc-windows-msvc
TARGET=${1:-"x86_64-pc-windows-msvc"}

echo "Building crane for Windows ($TARGET)"

# Ensure docker is installed
if ! command -v docker >/dev/null 2>&1; then
    echo "Error: 'docker' command not found. Please install Docker first."
    exit 1
fi

# Build binary
echo "Running cargo-xwin build inside Docker..."
docker run --rm -i \
    -u "$(id -u):$(id -g)" \
    -v "$(pwd)":/io \
    -w /io \
    -e CARGO_HOME=/io/target/.cargo \
    -e XWIN_CACHE_DIR=/io/target/.xwin-cache \
    messense/cargo-xwin \
    cargo xwin build --release --locked --target "$TARGET"

# Package
BIN="target/$TARGET/release/crane.exe"
if [ ! -f "$BIN" ]; then
    echo "Error: Binary not found at $BIN"
    exit 1
fi

echo "Windows binary successfully built"

echo "Output binary path: ${BIN}"
