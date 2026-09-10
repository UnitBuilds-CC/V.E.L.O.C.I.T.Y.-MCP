#!/bin/bash
exec /c/wasi-sdk/bin/clang "$@" -target x86_64-pc-windows-msvc 2>/dev/null || exec /c/wasi-sdk/bin/clang "$@"
