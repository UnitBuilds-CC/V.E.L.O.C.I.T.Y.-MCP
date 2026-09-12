# TinyGo WASM Tool Module

This directory contains the Go-based WASM tool that executes via the Wasmer runtime in VELOCITY-MCP.

## Current Status

**IMPORTANT**: This module requires **TinyGo** to compile. The pre-compiled `tool.wasm` binary was built with TinyGo v0.38.0.

### Dynamic Tool Dispatch (Updated 2026-09-12)

The Go WASM module now supports **dynamic tool dispatch** via the `_tool_name` field in the JSON payload. When the Rust server calls `tool_execute()`, it passes a payload like:

```json
{
  "_tool_name": "analyze_logs_go",
  "log_lines": ["INFO: request completed response_time=123ms", ...]
}
```

The module dispatches to different handler functions based on `_tool_name`:
- `go_text_stats`: Text analysis (word count, char count, line count)
- `analyze_logs_go`: Log analysis (error/warning/info counts, response times)

## Supported Tools

### go_text_stats
Analyzes text and returns word/character/line counts.

**Input:**
```json
{"text": "hello world"}
```

**Output:**
```json
{"word_count": 2, "char_count": 11, "line_count": 1}
```

### analyze_logs_go
Parses log lines and extracts metrics.

**Input:**
```json
{
  "log_lines": [
    "ERROR: connection failed",
    "WARN: slow query response_time=500ms",
    "INFO: request completed response_time=123ms"
  ]
}
```

**Output:**
```json
{
  "total_lines": 3,
  "error_count": 1,
  "warning_count": 1,
  "info_count": 1,
  "avg_response_time_ms": 311.5,
  "slowest_request_ms": 500
}
```

## Building

### Prerequisites

Install TinyGo from https://tinygo.org/getting-started/install/

```bash
# Verify installation
tinygo version
```

### Build Command

```bash
cd bench_tools/tinygo_wasm
./build.sh
```

Or manually:

```bash
tinygo build -target=wasi -o tool.wasm tool.go
```

This produces a WASI-compatible WASM binary (~900KB due to Go runtime inclusion).

### Build Output

```
Built: tool.wasm (909213 bytes)
```

**Note**: TinyGo modules include the full Go runtime (GC, scheduler), making them significantly larger than interpreter-based approaches or languages without GC overhead.

## Architecture

### Exports

The module exports two WASI functions:

- `prepare_call()`: No-op for Go (GC manages memory). Exported for ABI compatibility.
- `tool_execute(ptr: int32, length: int32) -> int64`: Main entry point. Reads JSON from WASM memory at the given pointer/length, dispatches based on `_tool_name`, and returns a packed `(ptr << 32) | length` pointing to the result JSON.

### Memory Management

The `encodeResult()` function:
1. Marshals the result struct to JSON
2. Pins the byte slice to prevent GC movement
3. Returns the pointer and length encoded in a single i64 value

The Rust host reads the result from WASM memory using the returned pointer/length pair.

## Adding New Tools

To add a new Go-based tool:

1. Create a handler function with signature `fn handleToolName(args map[string]interface{}) int64`
2. Add a case to the `switch toolName` statement in `tool_execute()`
3. Define input/output structs as needed
4. Rebuild the WASM binary using the build command above
5. Update the plugin manifest (`plugins/log_analyzer_go.json`) if creating a new tool

Example:

```go
func handleMyNewTool(args map[string]interface{}) int64 {
    // Extract arguments
    name := args["name"].(string)

    // Process...
    result := map[string]string{"message": "Hello, " + name}

    return encodeResult(result)
}
```

Then add to the switch:

```go
case "my_new_tool":
    return handleMyNewTool(payloadMap)
```

## Troubleshooting

### "tinygo not found in PATH"

Install TinyGo and ensure it's in your system PATH:

```bash
# Windows (PowerShell)
$env:PATH += ";C:\Program Files\TinyGo\bin"

# Linux/macOS
export PATH=$PATH:/usr/local/tinygo/bin
```

### Wrong results returned

If a tool returns incorrect results (e.g., `analyze_logs_go` returning `{"word_count":0,...}`), the WASM binary may be outdated. Rebuild it:

```bash
./build.sh
```

### Import errors

Ensure all imports are from the standard library. TinyGo does not support all Go standard library packages. Currently used:
- `encoding/json`
- `math`
- `regexp`
- `strconv`
- `strings`
- `unsafe`
