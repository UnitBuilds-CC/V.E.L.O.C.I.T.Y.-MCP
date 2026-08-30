# JSON-RPC v2.0 Stdio Protocol Handler

## Classification
- **Category**: Protocol / Transport
- **Files**: src/protocol/json_rpc.rs
- **Criticality**: High — primary MCP client interface

## Summary

The `run_stdio_loop()` function implements the standard MCP JSON-RPC v2.0 transport with full spec compliance. It uses a reader thread + channel architecture so stdin reads never block shutdown checks. Compatible with Claude Desktop, Cursor, and other MCP clients.

## Supported Methods

| Method | Handler | Response |
|--------|---------|----------|
| `initialize` | Inline | Protocol version 2024-11-05, full capability declaration, server info v3.0.0 |
| `tools/list` | Via `registry::get_tools()` | Array of Tool objects with cursor pagination |
| `tools/call` | Via `registry::call_tool()` | Tool result with `isError` flag |
| `ping` | Inline | Empty result (keepalive) |
| `logging/setLevel` | Inline | Set log verbosity dynamically |
| `notifications/cancelled` | Inline | Cancel in-flight request (no response) |
| `health/check` | Inline | Server status, mode, version |

## Error Codes

| Code | Meaning |
|------|---------|
| -32700 | Parse error (invalid JSON or request > 1 MB) |
| -32601 | Method not found |
| -32602 | Invalid params |

## Key Implementation Details

- **Reader thread + channel**: Stdin reads on dedicated thread, lines sent via channel
- **`recv_timeout`**: Main loop checks shutdown flag between requests
- **Atomic shutdown flag**: Ctrl+C handler sets flag for graceful exit
- Empty lines are skipped (not treated as errors)
- EOF terminates the loop gracefully
- Responses written with `println!()` (auto-flush per line)
- Notifications (null `id`) for unknown methods are silently ignored
- Full MCP spec compliance: ping, logging, cancellation, cursor pagination, elicitation, roots
