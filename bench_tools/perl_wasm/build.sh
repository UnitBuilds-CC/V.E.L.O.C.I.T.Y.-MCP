#!/bin/bash
# Build minimal Perl interpreter as WASI reactor for VELOCITY-MCP
set -e

WASI_SDK="/c/wasi-sdk"
CC="$WASI_SDK/bin/clang"
CFLAGS="--sysroot=$WASI_SDK/share/wasi-sysroot -O2 -DWASM"
LDFLAGS="--sysroot=$WASI_SDK/share/wasi-sysroot -Wl,--no-entry -Wl,--export-all"

echo "=== Building Perl WASI reactor ==="

echo "Compiling perl_wasi.c..."
$CC $CFLAGS -c perl_wasi.c -o perl_wasi.o

echo "Linking perl.wasm..."
$CC $LDFLAGS perl_wasi.o -o perl.wasm

ls -lh perl.wasm
echo ""
echo "=== Build complete: perl.wasm ==="
