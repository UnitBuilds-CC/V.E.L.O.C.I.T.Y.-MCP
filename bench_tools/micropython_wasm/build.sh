#!/bin/bash
# Build MicroPython as a WASI reactor (WASM binary).
#
# Prerequisites:
#   - wasi-sdk installed at /c/wasi-sdk (or set WASI_SDK env var)
#   - GnuWin32 Make on PATH (or set GNU_MAKE env var to full path)
#
# Usage:
#   ./build.sh          # full rebuild
#   ./build.sh clean    # remove build artifacts

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
OVERLAY_DIR="$SCRIPT_DIR/wasi-reactor"
SOURCE_TAR="$SCRIPT_DIR/micropython-1.24.1.tar.gz"
SOURCE_DIR="$SCRIPT_DIR/micropython-1.24.1"
PORT_DIR="$SOURCE_DIR/ports/webassembly"

MAKE="${GNU_MAKE:-make}"
WASI_SDK="${WASI_SDK:-/c/wasi-sdk}"

if [ "${1:-}" = "clean" ]; then
    rm -rf "$PORT_DIR/build-wasi"
    echo "Cleaned build artifacts."
    exit 0
fi

# Extract source if needed.
if [ ! -d "$SOURCE_DIR" ]; then
    echo "Extracting $SOURCE_TAR..."
    tar xzf "$SOURCE_TAR" -C "$SCRIPT_DIR"
fi

# Copy overlay files into the source tree.
echo "Applying WASI reactor overlay..."
cp "$OVERLAY_DIR/wasi_main.c" "$PORT_DIR/"
cp "$OVERLAY_DIR/Makefile.wasi" "$PORT_DIR/"
cp "$OVERLAY_DIR/setjmp.h" "$PORT_DIR/"
cp "$OVERLAY_DIR/variants/wasi/mpconfigvariant.h" "$PORT_DIR/variants/wasi/"
cp "$OVERLAY_DIR/variants/wasi/mpconfigvariant.mk" "$PORT_DIR/variants/wasi/"
cp "$OVERLAY_DIR/qstrdefsport.h" "$PORT_DIR/"

# Build.
echo "Building MicroPython WASI reactor..."
cd "$PORT_DIR"
export PATH="$(dirname "$WASI_SDK")/bin:$PATH"
exec "$MAKE" -f Makefile.wasi WASI_SDK="$WASI_SDK"
