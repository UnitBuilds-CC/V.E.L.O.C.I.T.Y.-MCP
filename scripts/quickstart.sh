#!/usr/bin/env bash
# VELOCITY-MCP Quickstart — zero-config MCP server in 10 seconds
# Usage: curl -fsSL https://raw.githubusercontent.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/main/scripts/quickstart.sh | bash

set -euo pipefail

REPO="UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP"
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

# Published asset names are not stable across releases (v3.0.0 shipped
# velocity_mcp-v3.0.0-windows-x86_64.exe, v3.1.0 shipped velocity_mcp.exe, and the release
# workflow uploads velocity_mcp_v<version>_<platform>_<arch>), so nothing here constructs a
# name: the download URL is read out of the release's own asset list.
if [ -n "${VELOCITY_VERSION:-}" ]; then
    VERSION="$VELOCITY_VERSION"
else
    VERSION="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null | grep -m1 -o '"tag_name": *"[^"]*"' | cut -d'"' -f4 || true)"
fi
ASSET_API="https://api.github.com/repos/${REPO}/releases/tags/${VERSION}"

# Create install directory
mkdir -p "$INSTALL_DIR"

# Download binary
case "$PLATFORM" in
    linux) ASSET_MATCH='linux' ;;
    macos) ASSET_MATCH='macos\|darwin' ;;
    win)   ASSET_MATCH='\.exe$' ;;
esac
ASSET_LIST="$(curl -fsSL "$ASSET_API" 2>/dev/null | grep -o '"browser_download_url": *"[^"]*"' | cut -d'"' -f4 || true)"
DOWNLOAD_URL="$(printf '%s\n' "$ASSET_LIST" | grep -m1 "$ASSET_MATCH" || true)"
if [ "$PLATFORM" = "win" ]; then
    BINARY_NAME="velocity_mcp.exe"
fi
if [ -z "$DOWNLOAD_URL" ]; then
    if [ -z "$ASSET_LIST" ]; then
        echo "Release '${VERSION:-unknown}' does not exist or publishes no files."
    else
        echo "Release '${VERSION}' publishes no ${PLATFORM} asset. Its files are:"
        printf '%s\n' "$ASSET_LIST" | sed 's|.*/|  |'
    fi
    echo ""
    echo "Install a build for this platform with one of:"
    echo "  cargo install velocity_mcp"
    echo "  docker pull unitbuilds/velocity-mcp:latest"
    exit 1
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
echo "Performance: curl http://localhost:3000/v1/performance (HTTP mode only)"
echo ""

exec "$INSTALL_DIR/$BINARY_NAME" --mode stdio
