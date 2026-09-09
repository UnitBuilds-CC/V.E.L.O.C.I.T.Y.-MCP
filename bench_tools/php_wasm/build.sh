#!/bin/bash
# Build minimal PHP interpreter as WASI reactor for VELOCITY-MCP
set -e

WASI_SDK="/c/wasi-sdk"
CC="$WASI_SDK/bin/clang"
CFLAGS="--sysroot=$WASI_SDK/share/wasi-sysroot -O2 -DWASM"
LDFLAGS="--sysroot=$WASI_SDK/share/wasi-sysroot -Wl,--no-entry -Wl,--export-all"

echo "=== Building PHP WASI reactor ==="

# Compile PHP interpreter wrapper
echo "Compiling php_wasi.c..."
$CC $CFLAGS -c php_wasi.c -o php_wasi.o

# Link as WASI reactor
echo "Linking php.wasm..."
$CC $LDFLAGS php_wasi.o -o php.wasm

# Report size
ls -lh php.wasm
echo ""
echo "=== Build complete: php.wasm ==="
