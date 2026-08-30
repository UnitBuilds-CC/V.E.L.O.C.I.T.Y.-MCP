# JSON-RPC Stdio Handler

<cite>
**Referenced Files in This Document**
- [src/protocol/json_rpc.rs](file://src/protocol/json_rpc.rs)
- [src/registry.rs](file://src/registry.rs)
</cite>

## Overview

The JSON-RPC stdio handler (`src/protocol/json_rpc.rs`) implements the standard MCP transport protocol. It uses a reader thread + channel architecture so that stdin reads never block shutdown checks. The handler reads newline-delimited JSON-RPC v2.0 requests from stdin, dispatches them to the tool registry, and writes JSON-RPC responses to stdout.

## Request Loop

The `run_stdio_loop()` function implements a threaded read-parse-dispatch-respond loop:

```
Reader Thread:  stdin → read_line → send to channel
Main Thread:    recv from channel → parse JSON → match method → dispatch → println! response
```

### Line Reading

- A dedicated reader thread acquires `stdin.lock()` for buffered I/O
- Reads until newline with `read_line()`
- Sends lines via channel to the main dispatch loop
- Main loop uses `recv_timeout` so shutdown checks are never blocked by stdin

### JSON Parsing

- Parses each trimmed line as `serde_json::Value`
- On parse failure: returns JSON-RPC error `-32700` (Parse error) with `id: null`
- Does not panic on malformed input
- Requests > 1 MB are rejected before JSON parsing

## Method Dispatch

### `initialize`

Returns the server's capabilities and info with full capability declaration:

```json
{
  "jsonrpc": "2.0",
  "id": <id>,
  "result": {
    "protocolVersion": "2024-11-05",
    "capabilities": {
      "tools": { "listChanged": false },
      "resources": { "subscribe": true, "listChanged": true },
      "prompts": { "listChanged": true },
      "sampling": {},
      "logging": {},
      "elicitation": {}
    },
    "serverInfo": {
      "name": "velocity-mcp",
      "version": "3.0.0"
    }
  }
}
```

### `tools/list`

Delegates to `registry::get_tools()` and returns the array of registered Tool objects with cursor pagination support:

```json
{
  "jsonrpc": "2.0",
  "id": <id>,
  "result": {
    "tools": [ /* 8 built-in Tool objects + dynamic tools */ ],
    "nextCursor": "..."
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

On error, `isError` is set to `true` and the text contains a sanitized error message.

### `ping`

Returns an empty result for keepalive:

```json
{"jsonrpc": "2.0", "id": <id>, "result": {}}
```

### `logging/setLevel`

Sets the server log verbosity dynamically:

```json
→ {"jsonrpc":"2.0","method":"logging/setLevel","params":{"level":"debug"},"id":1}
← {"jsonrpc":"2.0","id":1,"result":{}}
```

### `notifications/cancelled`

Cancels an in-flight request by ID. No response is sent (notification).

### `health/check`

Returns server health status:

```json
{"jsonrpc":"2.0","id":<id>,"result":{"status":"healthy","mode":"stdio","version":"3.0.0"}}
```

### Unknown Methods

Returns JSON-RPC error `-32601` (Method not found) — but only if the request has a non-null `id`. Notifications (id-less requests) for unknown methods are silently ignored per JSON-RPC spec.

## Error Codes

| Code | Meaning | When |
|------|---------|------|
| -32700 | Parse error | Invalid JSON on stdin or request > 1 MB |
| -32601 | Method not found | Unknown JSON-RPC method |
| -32602 | Invalid params | Missing required parameters |
| *(tool error)* | Tool execution failure | Wrapped in `isError: true` result |

## Graceful Shutdown

The server installs a Ctrl+C handler that sets an atomic shutdown flag. The main dispatch loop checks this flag via `recv_timeout` and exits cleanly when set. The reader thread is detached but the process exits when the main thread completes.

## Key Design Decisions

1. **Reader thread + channel**: Stdin reads happen on a dedicated thread, sending lines via channel. The main loop uses `recv_timeout` so shutdown checks are never blocked by a pending stdin read.
2. **Lock stdin once**: `stdin.lock()` is acquired once in the reader thread, not per-line.
3. **No response buffering**: Each response is written with `println!()` which flushes per-line. MCP clients expect immediate responses.
4. **Graceful degradation**: Parse errors don't terminate the loop. The server sends an error response and continues processing the next line.
5. **Full MCP spec compliance**: Supports all standard methods including ping, logging, cancellation, and capability declaration.

**Section sources**
- [src/protocol/json_rpc.rs](file://src/protocol/json_rpc.rs)
