#!/bin/bash
# Build C#/.NET as WASI reactor for VELOCITY-MCP
# NOTE: .NET WASM is complex - requires Mono WASM or IL interpreter
set -e

echo "=== Building C#/.NET WASI reactor ==="
echo ""
echo ".NET WASM requires one of:"
echo "  1. Mono WASM runtime (mono-sdks/wasm-release-Linux.zip)"
echo "  2. .NET 8+ NativeAOT-WASM (experimental)"
echo "  3. Custom IL interpreter compiled to WASM"
echo ""
echo "Recommended approach:"
echo "  - Download Mono WASM SDK from https://www.mono-project.com/download/"
echo "  - Compile a minimal C# interpreter that exposes WASI exports"
echo "  - Or use Blazor WASM runtime as a base"
echo ""
echo "Alternative: Pre-compile C# tools to WASM via:"
echo "  dotnet publish -c Release -r wasm-wasi /p:StripSymbols=true"
echo ""
echo "See: https://github.com/dotnet/runtime/tree/main/src/mono/wasm"
echo ""
echo "=== Build template complete ==="
