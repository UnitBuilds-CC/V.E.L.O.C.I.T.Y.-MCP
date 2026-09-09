#!/bin/bash
# Build Java as WASI reactor for VELOCITY-MCP
# NOTE: Java WASM requires TeaVM, GraalVM, or a JVM compiled to WASM
set -e

echo "=== Building Java WASI reactor ==="
echo ""
echo "Java WASM requires one of:"
echo "  1. TeaVM - compile Java bytecode to WASM"
echo "     https://teavm.org/"
echo "     git clone https://github.com/konsoletyper/teavm"
echo ""
echo "  2. GraalVM Native Image with WASM target (experimental)"
echo "     https://www.graalvm.org/"
echo ""
echo "  3. CheerpJ - Java to JS/WASM compiler"
echo "     https://leaningtech.com/cheerpj/"
echo ""
echo "Recommended: TeaVM approach"
echo "  1. Install TeaVM CLI"
echo "  2. Write a Java class that implements the WASI interface"
echo "  3. Compile: teavm --target WASM --output java.wasm ToolRunner.class"
echo ""
echo "Alternative: Use a minimal JVM like MicroVM or JamVM compiled with wasi-sdk"
echo ""
echo "See: https://github.com/nicko88/teavm-examples"
echo ""
echo "=== Build template complete ==="
