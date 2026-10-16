#!/bin/sh
set -eu

REPO="DeadZone-0/Lyre-rust"
BIN_DIR="${BIN_DIR:-$HOME/.local/bin}"

OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
if [ "$OS" != "linux" ]; then
    echo "Error: unsupported operating system: $OS (lyre currently supports Linux)" >&2
    exit 1
fi

ARCH="$(uname -m)"
case "$ARCH" in
    x86_64|amd64)
        TARGET="x86_64-unknown-linux-gnu"
        ;;
    aarch64|arm64)
        TARGET="aarch64-unknown-linux-gnu"
        ;;
    *)
        echo "Error: unsupported architecture: $ARCH" >&2
        exit 1
        ;;
esac

mkdir -p "$BIN_DIR"

TAG="${LYRE_VERSION:-latest}"
if [ "$TAG" = "latest" ]; then
    DOWNLOAD_URL="https://github.com/$REPO/releases/latest/download/lyre-$TARGET.tar.gz"
else
    DOWNLOAD_URL="https://github.com/$REPO/releases/download/$TAG/lyre-$TARGET.tar.gz"
fi

TMP_DIR="$(mktemp -d -t lyre-install.XXXXXX)"
trap 'rm -rf "$TMP_DIR"' EXIT INT TERM

echo "Downloading lyre for $TARGET..."
if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$DOWNLOAD_URL" -o "$TMP_DIR/lyre.tar.gz"
elif command -v wget >/dev/null 2>&1; then
    wget -qO "$TMP_DIR/lyre.tar.gz" "$DOWNLOAD_URL"
else
    echo "Error: curl or wget is required to download lyre" >&2
    exit 1
fi

echo "Extracting binary..."
tar -xzf "$TMP_DIR/lyre.tar.gz" -C "$TMP_DIR"

if [ -f "$TMP_DIR/lyre" ]; then
    mv "$TMP_DIR/lyre" "$BIN_DIR/lyre"
elif [ -f "$TMP_DIR/lyre-$TARGET/lyre" ]; then
    mv "$TMP_DIR/lyre-$TARGET/lyre" "$BIN_DIR/lyre"
else
    echo "Error: failed to find lyre binary in downloaded archive" >&2
    exit 1
fi

chmod +x "$BIN_DIR/lyre"
echo "Successfully installed lyre to $BIN_DIR/lyre"

case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *)
        echo "Notice: $BIN_DIR is not in your PATH."
        echo "Add it by running:"
        echo "  export PATH=\"$BIN_DIR:\$PATH\""
        ;;
esac
