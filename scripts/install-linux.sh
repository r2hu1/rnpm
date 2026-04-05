#!/bin/bash
# Installation script for rnpm (Linux - system-wide with sudo)

set -e

echo "Installing rnpm (system-wide)..."

# Check if running as root
if [ "$EUID" -ne 0 ]; then
    echo "This script requires root privileges for system-wide installation."
    echo "Please run with sudo or use the user-level install script."
    echo ""
    echo "Usage:"
    echo "  sudo ./scripts/install-linux.sh     # System-wide (/usr/local/bin)"
    echo "  ./scripts/install.sh                # User-level (~/.local/bin)"
    exit 1
fi

# Build the project (as non-root user if possible)
echo "Building rnpm..."

# Find the project root (parent directory of scripts/)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$PROJECT_ROOT"

# If SUDO_USER is set, build as that user to avoid permission issues
if [ -n "$SUDO_USER" ]; then
    su "$SUDO_USER" -c "cargo build --release"
else
    cargo build --release
fi

# Install to /usr/local/bin
INSTALL_DIR="/usr/local/bin"
mkdir -p "$INSTALL_DIR"

cp target/release/rnpm "$INSTALL_DIR/rnpm"
chmod +x "$INSTALL_DIR/rnpm"

echo ""
echo "rnpm installed successfully to $INSTALL_DIR/rnpm"
echo ""
echo "To verify the installation, run:"
echo "  rnpm --version"
