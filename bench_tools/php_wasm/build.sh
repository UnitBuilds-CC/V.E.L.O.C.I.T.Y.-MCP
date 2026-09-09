#!/bin/bash
# Build PHP as WASI reactor for VELOCITY-MCP
# NOTE: This is a template - PHP embedding requires php-src with embed SAPI enabled
set -e

# PHP source (download from https://www.php.net/distributions/)
PHP_VERSION="8.3.3"
PHP_SRC="php-${PHP_VERSION}"
PHP_TAR="${PHP_SRC}.tar.gz"

WASI_SDK="/c/wasi-sdk"
CC="$WASI_SDK/bin/clang"
CFLAGS="--sysroot=$WASI_SDK/share/wasi-sysroot -O2 -DWASM"
LDFLAGS="--sysroot=$WASI_SDK/share/wasi-sysroot -Wl,--no-entry -Wl,--export-all"

echo "=== Building PHP ${PHP_VERSION} WASI reactor ==="
echo ""
echo "NOTE: PHP WASM requires php-src compiled with embed SAPI."
echo "This is a complex build - consider using pre-built php-wasm packages."
echo ""

# Check if PHP source exists
if [ ! -d "$PHP_SRC" ]; then
    echo "PHP source not found. Download from: https://www.php.net/distributions/php-${PHP_VERSION}.tar.gz"
    echo ""
    echo "Alternative: Use pre-built php-wasm from npm (php-wasm package)"
    exit 1
fi

# PHP build is complex - requires:
# 1. Configure with --enable-embed=static
# 2. Disable unsupported extensions (mysqli, pdo, etc.)
# 3. Compile with WASI SDK
# 4. Link with php_wasi.c wrapper

echo "PHP WASM build requires manual setup."
echo "See: https://github.com/seanmorris/php-wasm for reference implementation."
echo ""
echo "Quick alternative: Use npm php-wasm package and adapt to WASI reactor pattern."

# Placeholder build command (won't work without actual PHP embed SAPI)
# $CC $CFLAGS -I"$PHP_SRC/main" -I"$PHP_SRC/TSRM" -I"$PHP_SRC/Zend" \
#     -c php_wasi.c -o php_wasi.o
# $CC $LDFLAGS php_wasi.o libphp.a -o php.wasm

echo ""
echo "=== Build template complete ==="
