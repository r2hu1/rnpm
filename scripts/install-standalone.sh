#!/bin/bash
# Standalone installation script for rnpm (macOS/Linux)
# Downloads latest release and installs without requiring git clone

set -e

echo "Installing rnpm..."

# Detect OS and architecture
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
    Linux*)     PLATFORM="linux";;
    Darwin*)    PLATFORM="macos";;
    *)          echo "Unsupported OS: $OS"; exit 1;;
esac

case "$ARCH" in
    x86_64)     ARCH_NAME="x86_64";;
    arm64|aarch64) ARCH_NAME="aarch64";;
    *)          echo "Unsupported architecture: $ARCH"; exit 1;;
esac

echo "Detected platform: $PLATFORM ($ARCH_NAME)"

# Get latest release from GitHub
REPO="r2hu1/rnpm"
LATEST_RELEASE=$(curl -s "https://api.github.com/repos/$REPO/releases/latest")

if [ -z "$LATEST_RELEASE" ]; then
    echo "Error: Could not fetch latest release information"
    exit 1
fi

# Extract version
VERSION=$(echo "$LATEST_RELEASE" | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": "\(.*\)".*/\1/')

if [ -z "$VERSION" ]; then
    echo "Warning: Could not determine version, using 'latest'"
    VERSION="latest"
fi

echo "Latest version: $VERSION"

# Determine binary name
BINARY_NAME="rnpm-$PLATFORM-$ARCH_NAME"
if [ "$PLATFORM" = "linux" ]; then
    BINARY_NAME="$BINARY_NAME-linux"
fi

# Try to download from releases first
DOWNLOAD_URL="https://github.com/$REPO/releases/download/$VERSION/$BINARY_NAME"

echo "Downloading rnpm..."

TEMP_DIR=$(mktemp -d)
trap "rm -rf $TEMP_DIR" EXIT

if curl -L -o "$TEMP_DIR/rnpm" -f "$DOWNLOAD_URL" 2>/dev/null; then
    chmod +x "$TEMP_DIR/rnpm"
else
    echo "Pre-built binary not found. Building from source..."

    # Check if Rust is installed
    if ! command -v cargo &> /dev/null; then
        echo "Error: Rust/Cargo is not installed."
        echo "Please install from https://rustup.rs/"
        exit 1
    fi

    # Clone, build, and install
    echo "Cloning repository..."
    git clone --depth 1 "https://github.com/$REPO.git" "$TEMP_DIR/rnpm-src" 2>/dev/null || {
        echo "Error: Could not clone repository"
        exit 1
    }

    cd "$TEMP_DIR/rnpm-src"
    echo "Building..."
    cargo build --release

    cp target/release/rnpm "$TEMP_DIR/rnpm"
fi

# Determine installation directory
if [ "$EUID" -eq 0 ]; then
    INSTALL_DIR="/usr/local/bin"
else
    if [[ ":$PATH:" == *":$HOME/.local/bin:"* ]]; then
        INSTALL_DIR="$HOME/.local/bin"
    else
        CARGO_BIN="$HOME/.cargo/bin"
        if [ -d "$CARGO_BIN" ] && [[ ":$PATH:" == *":$CARGO_BIN:"* ]]; then
            INSTALL_DIR="$CARGO_BIN"
        else
            INSTALL_DIR="$HOME/.local/bin"
            mkdir -p "$INSTALL_DIR"
        fi
    fi
fi

mkdir -p "$INSTALL_DIR"
cp "$TEMP_DIR/rnpm" "$INSTALL_DIR/rnpm"
chmod +x "$INSTALL_DIR/rnpm"

echo ""
echo "✓ rnpm installed successfully to $INSTALL_DIR/rnpm"
echo ""
echo "To verify the installation, run:"
echo "  rnpm --version"
echo ""
if [[ ":$PATH:" != *":$INSTALL_DIR:"* ]]; then
    echo "Note: You may need to add $INSTALL_DIR to your PATH:"
    echo "  export PATH=\"$INSTALL_DIR:\$PATH\""
fi
