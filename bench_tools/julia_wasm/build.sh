#!/bin/bash
# Build minimal Julia interpreter as WASI reactor for VELOCITY-MCP
set -e

WASI_SDK="/c/wasi-sdk"
CC="$WASI_SDK/bin/clang"
CFLAGS="--sysroot=$WASI_SDK/share/wasi-sysroot -O2 -DWASM"
LDFLAGS="--sysroot=$WASI_SDK/share/wasi-sysroot -Wl,--no-entry -Wl,--export-all"

echo "=== Building Julia WASI reactor ==="

echo "Compiling julia_wasi.c..."
$CC $CFLAGS -c julia_wasi.c -o julia_wasi.o

echo "Linking julia.wasm..."
$CC $LDFLAGS julia_wasi.o -o julia.wasm

ls -lh julia.wasm
echo ""
echo "=== Build complete: julia.wasm ==="
