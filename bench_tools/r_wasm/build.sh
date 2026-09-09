#!/bin/bash
# Build R as WASI reactor for VELOCITY-MCP
# Uses WebR (R compiled to WASM) - https://docs.r-wasm.org/
set -e

echo "=== Building R WASI reactor ==="
echo ""
echo "R WASM is available via WebR: https://github.com/r-wasm/webr"
echo ""
echo "Option 1: Use pre-built WebR"
echo "  1. Download WebR release: https://github.com/r-wasm/webr/releases"
echo "  2. Extract webr.wasm and adapt to WASI reactor pattern"
echo "  3. Link with r_wasi.c wrapper"
echo ""
echo "Option 2: Build R from source with WASI SDK"
echo "  1. Download R source: https://cran.r-project.org/sources.html"
echo "  2. Configure with WASI SDK:"
echo "     export CC=/c/wasi-sdk/bin/clang"
echo "     export CXX=/c/wasi-sdk/bin/clang++"
echo "     ./configure --host=wasm32-unknown-wasi --disable-shared"
echo "  3. Build and link with r_wasi.c"
echo ""
echo "Option 3: Use R-SDK WASM builds"
echo "  https://repo.r-wasm.org/"
echo ""
echo "WebR provides R 4.3+ compiled to WASM with full package support."
echo "The main challenge is adapting WebR's JS interface to WASI exports."
echo ""
echo "=== Build template complete ==="
