#!/bin/bash
# Installation script for rnpm (macOS/Linux)

set -e

echo "Installing rnpm..."

# Detect OS
OS="$(uname -s)"
case "$OS" in
    Linux*)     PLATFORM="linux";;
    Darwin*)    PLATFORM="macos";;
    *)          echo "Unsupported OS: $OS"; exit 1;;
esac

echo "Detected platform: $PLATFORM"

# Build the project
echo "Building rnpm..."
cargo build --release

# Determine installation directory
if [ "$EUID" -eq 0 ]; then
    # Root user - install to /usr/local/bin
    INSTALL_DIR="/usr/local/bin"
else
    # Non-root user - check if ~/.local/bin exists in PATH
    if [[ ":$PATH:" == *":$HOME/.local/bin:"* ]]; then
        INSTALL_DIR="$HOME/.local/bin"
    else
        # Try to use cargo's bin directory or suggest alternatives
        CARGO_BIN="$HOME/.cargo/bin"
        if [ -d "$CARGO_BIN" ] && [[ ":$PATH:" == *":$CARGO_BIN:"* ]]; then
            INSTALL_DIR="$CARGO_BIN"
        else
            INSTALL_DIR="$HOME/.local/bin"
            mkdir -p "$INSTALL_DIR"
            echo "Note: Added $INSTALL_DIR to installation path"
            echo "You may need to add it to your PATH manually:"
            echo "  export PATH=\"\$HOME/.local/bin:\$PATH\""
        fi
    fi
fi

# Create installation directory if it doesn't exist
mkdir -p "$INSTALL_DIR"

# Copy binary
cp target/release/rnpm "$INSTALL_DIR/rnpm"
chmod +x "$INSTALL_DIR/rnpm"

echo ""
echo "rnpm installed successfully to $INSTALL_DIR/rnpm"
echo ""
echo "To verify the installation, run:"
echo "  rnpm --version"
echo ""
echo "If 'rnpm' is not found, you may need to restart your terminal or add to PATH:"
echo "  export PATH=\"$INSTALL_DIR:\$PATH\""
