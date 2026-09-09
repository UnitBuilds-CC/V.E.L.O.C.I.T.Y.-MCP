#!/bin/bash
# Build minimal C# interpreter as WASI reactor for VELOCITY-MCP
set -e

WASI_SDK="/c/wasi-sdk"
CC="$WASI_SDK/bin/clang"
CFLAGS="--sysroot=$WASI_SDK/share/wasi-sysroot -O2 -DWASM"
LDFLAGS="--sysroot=$WASI_SDK/share/wasi-sysroot -Wl,--no-entry -Wl,--export-all"

echo "=== Building C# WASI reactor ==="

echo "Compiling dotnet_wasi.c..."
$CC $CFLAGS -c dotnet_wasi.c -o dotnet_wasi.o

echo "Linking dotnet.wasm..."
$CC $LDFLAGS dotnet_wasi.o -o dotnet.wasm

ls -lh dotnet.wasm
echo ""
echo "=== Build complete: dotnet.wasm ==="
