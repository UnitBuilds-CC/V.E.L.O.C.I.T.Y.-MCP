#!/usr/bin/env bash
# VELOCITY-MCP Quickstart — zero-config MCP server in 10 seconds
# Usage: curl -fsSL https://raw.githubusercontent.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/main/quickstart.sh | bash

set -euo pipefail

VERSION="3.0.0"
INSTALL_DIR="${VELOCITY_INSTALL_DIR:-$HOME/.velocity-mcp}"
BINARY_NAME="velocity_mcp"

# Detect platform
OS="$(uname -s)"
ARCH="$(uname -m)"
case "$OS" in
    Linux*)  PLATFORM="linux" ;;
    Darwin*) PLATFORM="macos" ;;
    MINGW*|MSYS*|CYGWIN*) PLATFORM="win" ;;
    *) echo "Unsupported OS: $OS"; exit 1 ;;
esac
case "$ARCH" in
    x86_64|amd64) ARCH_SUFFIX="x64" ;;
    aarch64|arm64) ARCH_SUFFIX="arm64" ;;
    *) echo "Unsupported architecture: $ARCH"; exit 1 ;;
esac

echo "=== VELOCITY-MCP Quickstart ==="
echo "Platform: ${PLATFORM}_${ARCH_SUFFIX}"
echo ""

# Create install directory
mkdir -p "$INSTALL_DIR"

# Download binary
DOWNLOAD_URL="https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/releases/download/v${VERSION}/velocity_mcp_v${VERSION}_${PLATFORM}_${ARCH_SUFFIX}"
if [ "$PLATFORM" = "win" ]; then
    DOWNLOAD_URL="${DOWNLOAD_URL}.exe"
    BINARY_NAME="velocity_mcp.exe"
fi

echo "Downloading VELOCITY-MCP v${VERSION}..."
if command -v curl &>/dev/null; then
    curl -fsSL -o "$INSTALL_DIR/$BINARY_NAME" "$DOWNLOAD_URL"
elif command -v wget &>/dev/null; then
    wget -q -O "$INSTALL_DIR/$BINARY_NAME" "$DOWNLOAD_URL"
else
    echo "Error: curl or wget required"
    exit 1
fi

chmod +x "$INSTALL_DIR/$BINARY_NAME"
echo "Installed to: $INSTALL_DIR/$BINARY_NAME"
echo ""

# Start server in stdio mode (default, works with Claude Desktop, Cursor, etc.)
echo "Starting VELOCITY-MCP server in stdio mode..."
echo "Connect your MCP client (Claude Desktop, Cursor, etc.) to begin."
echo ""
echo "To run in HTTP mode: $INSTALL_DIR/$BINARY_NAME --mode http --addr 0.0.0.0:3000"
echo "To see all options:  $INSTALL_DIR/$BINARY_NAME --help"
echo ""
echo "Performance: curl http://localhost:3000/performance (HTTP mode only)"
echo ""

exec "$INSTALL_DIR/$BINARY_NAME" --mode stdio
