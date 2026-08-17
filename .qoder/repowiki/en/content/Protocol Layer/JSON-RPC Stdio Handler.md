# JSON-RPC Stdio Handler

<cite>
**Referenced Files in This Document**
- [src/protocol/json_rpc.rs](file://src/protocol/json_rpc.rs)
- [src/registry.rs](file://src/registry.rs)
</cite>

## Overview

The JSON-RPC stdio handler (`src/protocol/json_rpc.rs`) implements the standard MCP transport protocol. It reads newline-delimited JSON-RPC v2.0 requests from stdin, dispatches them to the tool registry, and writes JSON-RPC responses to stdout.

## Request Loop

The `run_stdio_loop()` function implements an infinite read-parse-dispatch-respond loop:

```
stdin → read_line → parse JSON → match method → dispatch → println! response
```

### Line Reading

- Uses `stdin.lock()` for buffered I/O (avoids per-line syscall overhead)
- Reads until newline with `read_line()`
- Exits on EOF (bytes_read == 0)
- Skips empty/whitespace-only lines

### JSON Parsing

- Parses each trimmed line as `serde_json::Value`
- On parse failure: returns JSON-RPC error `-32700` (Parse error) with `id: null`
- Does not panic on malformed input

## Method Dispatch

### `initialize`

Returns the server's capabilities and info:

```json
{
  "jsonrpc": "2.0",
  "id": <id>,
  "result": {
    "protocolVersion": "2024-11-05",
    "capabilities": { "tools": {} },
    "serverInfo": {
      "name": "velocity-mcp-rust-server",
      "version": "1.0.0"
    }
  }
}
```

### `tools/list`

Delegates to `registry::get_tools()` and returns the array of registered Tool objects:

```json
{
  "jsonrpc": "2.0",
  "id": <id>,
  "result": {
    "tools": [ /* Tool objects */ ]
  }
}
```

### `tools/call`

Extracts `params.name` and `params.arguments`, delegates to `registry::call_tool()`:

```json
{
  "jsonrpc": "2.0",
  "id": <id>,
  "result": {
    "content": [{ "type": "text", "text": "<output>" }],
    "isError": false
  }
}
```

On error, `isError` is set to `true` and the text contains the error message.

### Unknown Methods

Returns JSON-RPC error `-32601` (Method not found) — but only if the request has a non-null `id`. Notifications (id-less requests) for unknown methods are silently ignored per JSON-RPC spec.

## Error Codes

| Code | Meaning | When |
|------|---------|------|
| -32700 | Parse error | Invalid JSON on stdin |
| -32601 | Method not found | Unknown JSON-RPC method |
| *(tool error)* | Tool execution failure | Wrapped in `isError: true` result |

## Key Design Decisions

1. **Lock stdin once**: `stdin.lock()` is acquired once at loop start, not per-line. This avoids repeated mutex acquisitions.
2. **`Box<dyn Error>` return**: The function returns `Result<(), Box<dyn Error>>` — any I/O error propagates up to `main()` which prints it and exits.
3. **No response buffering**: Each response is written with `println!()` which flushes per-line. This is intentional — MCP clients expect immediate responses.
4. **Graceful degradation**: Parse errors don't terminate the loop. The server sends an error response and continues processing the next line.

**Section sources**
- [src/protocol/json_rpc.rs](file://src/protocol/json_rpc.rs)
