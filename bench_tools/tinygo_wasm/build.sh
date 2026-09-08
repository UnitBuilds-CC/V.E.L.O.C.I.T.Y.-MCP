#!/bin/bash
set -e

echo "Building TinyGo WASM tool..."

# Check for tinygo
if ! command -v tinygo &> /dev/null; then
    echo "ERROR: tinygo not found in PATH"
    echo "Install from: https://tinygo.org/getting-started/install/"
    exit 1
fi

echo "TinyGo version:"
tinygo version

# Build for WASI target
tinygo build -target=wasi -o tool.wasm tool.go

echo "Built: tool.wasm ($(wc -c < tool.wasm) bytes)"
echo ""
echo "Note: TinyGo modules include the Go runtime (GC, scheduler) so they're larger"
echo "than interpreter-based approaches, but each tool is a standalone WASM module."
