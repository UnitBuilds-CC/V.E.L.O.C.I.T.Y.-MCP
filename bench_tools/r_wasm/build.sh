#!/bin/bash
# Build minimal R interpreter as WASI reactor for VELOCITY-MCP
set -e

WASI_SDK="/c/wasi-sdk"
CC="$WASI_SDK/bin/clang"
CFLAGS="--sysroot=$WASI_SDK/share/wasi-sysroot -O2 -DWASM"
LDFLAGS="--sysroot=$WASI_SDK/share/wasi-sysroot -Wl,--no-entry -Wl,--export-all"

echo "=== Building R WASI reactor ==="

echo "Compiling r_wasi.c..."
$CC $CFLAGS -c r_wasi.c -o r_wasi.o

echo "Linking r.wasm..."
$CC $LDFLAGS r_wasi.o -o r.wasm

ls -lh r.wasm
echo ""
echo "=== Build complete: r.wasm ==="
