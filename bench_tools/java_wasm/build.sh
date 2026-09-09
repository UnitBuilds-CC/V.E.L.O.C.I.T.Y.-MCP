#!/bin/bash
# Build minimal Java interpreter as WASI reactor for VELOCITY-MCP
set -e

WASI_SDK="/c/wasi-sdk"
CC="$WASI_SDK/bin/clang"
CFLAGS="--sysroot=$WASI_SDK/share/wasi-sysroot -O2 -DWASM"
LDFLAGS="--sysroot=$WASI_SDK/share/wasi-sysroot -Wl,--no-entry -Wl,--export-all"

echo "=== Building Java WASI reactor ==="

echo "Compiling java_wasi.c..."
$CC $CFLAGS -c java_wasi.c -o java_wasi.o

echo "Linking java.wasm..."
$CC $LDFLAGS java_wasi.o -o java.wasm

ls -lh java.wasm
echo ""
echo "=== Build complete: java.wasm ==="
