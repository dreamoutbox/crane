#!/bin/sh
set -e

REPO="dreamoutbox/crane"

# Detect OS
OS="$(uname -s)"
case "$OS" in
    Linux)
        OS_TYPE="linux"
        ;;
    Darwin)
        OS_TYPE="darwin"
        ;;
    *)
        echo "Error: Unsupported operating system: $OS" >&2
        echo "If you are on Windows, please download the zip release from:" >&2
        echo "https://github.com/$REPO/releases" >&2
        exit 1
        ;;
esac

# Detect Architecture
ARCH="$(uname -m)"
case "$ARCH" in
    x86_64|amd64)
        ARCH_TYPE="x86_64"
        ;;
    aarch64|arm64)
        ARCH_TYPE="aarch64"
        ;;
    *)
        echo "Error: Unsupported architecture: $ARCH" >&2
        exit 1
        ;;
esac

# Resolve target triple
if [ "$OS_TYPE" = "darwin" ]; then
    TARGET="${ARCH_TYPE}-apple-darwin"
elif [ "$OS_TYPE" = "linux" ]; then
    if [ "$ARCH_TYPE" = "aarch64" ]; then
        TARGET="aarch64-unknown-linux-gnu"
    elif [ "$ARCH_TYPE" = "x86_64" ]; then
        if ldd --version 2>&1 | grep -qi "musl"; then
            TARGET="x86_64-unknown-linux-musl"
        else
            TARGET="x86_64-unknown-linux-gnu"
        fi
    fi
fi

# Resolve version / tag
if [ -n "$CRANE_VERSION" ]; then
    TAG="v${CRANE_VERSION#v}"
else
    # Fetch latest release tag from GitHub API, falling back to redirect header
    TAG="$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" 2>/dev/null | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/' || true)"
    if [ -z "$TAG" ]; then
        TAG="$(curl -fsSLI -o /dev/null -w "%{url_effective}" "https://github.com/$REPO/releases/latest" 2>/dev/null | sed -E 's#.*/tag/([^/]+).*#\1#' || true)"
    fi
fi

if [ -z "$TAG" ]; then
    echo "Error: Unable to determine the latest release version for $REPO." >&2
    echo "Please specify CRANE_VERSION (e.g. CRANE_VERSION=0.5.4) and retry." >&2
    exit 1
fi

ARCHIVE="crane-${TAG}-${TARGET}.tar.gz"
DOWNLOAD_URL="https://github.com/$REPO/releases/download/${TAG}/${ARCHIVE}"

TMP_DIR="$(mktemp -d)"
cleanup() {
    rm -rf "$TMP_DIR"
}
trap cleanup EXIT INT TERM

echo "Downloading Crane ${TAG} for ${TARGET}..."
if ! curl -fsSL "$DOWNLOAD_URL" -o "$TMP_DIR/$ARCHIVE"; then
    echo "Error: Failed to download $DOWNLOAD_URL" >&2
    exit 1
fi

tar -xzf "$TMP_DIR/$ARCHIVE" -C "$TMP_DIR"

if [ ! -f "$TMP_DIR/crane" ]; then
    echo "Error: Binary 'crane' not found in downloaded archive." >&2
    exit 1
fi

# Determine install location
if [ -n "$CRANE_INSTALL_DIR" ]; then
    DEST_DIR="$CRANE_INSTALL_DIR"
elif [ "$(id -u)" -eq 0 ]; then
    DEST_DIR="/usr/local/bin"
elif [ -w "/usr/local/bin" ]; then
    DEST_DIR="/usr/local/bin"
else
    DEST_DIR="$HOME/.local/bin"
fi

mkdir -p "$DEST_DIR"
cp "$TMP_DIR/crane" "$DEST_DIR/crane"
chmod +x "$DEST_DIR/crane"

echo "Crane successfully installed to $DEST_DIR/crane"

case ":$PATH:" in
    *:"$DEST_DIR":*) ;;
    *)
        echo ""
        echo "Notice: $DEST_DIR is not in your PATH."
        echo "Add it to your PATH by adding this line to your shell configuration file (~/.bashrc, ~/.zshrc, etc.):"
        echo "  export PATH=\"\$PATH:$DEST_DIR\""
        ;;
esac

