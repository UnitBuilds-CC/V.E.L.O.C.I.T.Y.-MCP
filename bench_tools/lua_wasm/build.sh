#!/bin/bash
# Build Lua 5.4 as WASI reactor for VELOCITY-MCP
set -e

LUA_SRC="lua-5.4.7/src"
WASI_SDK="/c/wasi-sdk"
CC="$WASI_SDK/bin/clang"
# Include current directory first for custom setjmp.h
CFLAGS="--sysroot=$WASI_SDK/share/wasi-sysroot -I. -O2 -DWASM -D_WASI_EMULATED_SIGNAL -D_WASI_EMULATED_PROCESS_CLOCKS"
LDFLAGS="--sysroot=$WASI_SDK/share/wasi-sysroot -Wl,--no-entry -Wl,--export-all -Wl,--allow-undefined -lwasi-emulated-signal -lwasi-emulated-process-clocks"

echo "=== Building Lua 5.4 WASI reactor ==="

# Core Lua source files (excluding standalone interpreters)
LUA_CORE_SRCS=(
    lapi.c lauxlib.c lbaselib.c lcode.c lcorolib.c lctype.c
    ldblib.c ldebug.c ldo.c ldump.c lfunc.c lgc.c linit.c
    llex.c lmathlib.c lmem.c lobject.c lopcodes.c lparser.c
    lstate.c lstring.c lstrlib.c ltable.c ltablib.c ltm.c
    lundump.c lvm.c lzio.c
)

# Optional libs - exclude loslib.c (needs tmpnam), loadlib.c (needs dlopen), liolib.c (needs tmpfile)
LUA_OPTIONAL_SRCS=(
    lutf8lib.c  # UTF-8 support
)

# Collect all sources
ALL_SRCS=("${LUA_CORE_SRCS[@]}" "${LUA_OPTIONAL_SRCS[@]}")

# Clean old objects
rm -f *.o

# Build object files
echo "Compiling ${#ALL_SRCS[@]} Lua source files..."
OBJS=""
for src in "${ALL_SRCS[@]}"; do
    obj="${src%.c}.o"
    echo "  $src -> $obj"
    $CC $CFLAGS -I"$LUA_SRC" -c "$LUA_SRC/$src" -o "$obj" 2>&1 || {
        echo "ERROR: Failed to compile $src"
        exit 1
    }
    OBJS="$OBJS $obj"
done

# Compile wrapper
echo "Compiling lua_wasi.c..."
$CC $CFLAGS -I"$LUA_SRC" -c lua_wasi.c -o lua_wasi.o

# Link as WASI reactor
echo "Linking lua.wasm..."
$CC $LDFLAGS $OBJS lua_wasi.o -o lua.wasm

# Report size
ls -lh lua.wasm
echo ""
echo "=== Build complete: lua.wasm ==="
