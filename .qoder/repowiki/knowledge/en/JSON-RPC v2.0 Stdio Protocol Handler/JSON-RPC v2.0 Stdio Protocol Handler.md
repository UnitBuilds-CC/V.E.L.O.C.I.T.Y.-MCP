# JSON-RPC v2.0 Stdio Protocol Handler

## Classification
- **Category**: Protocol / Transport
- **Files**: src/protocol/json_rpc.rs (113 LOC)
- **Criticality**: High — primary MCP client interface

## Summary

The `run_stdio_loop()` function implements the standard MCP JSON-RPC v2.0 transport. It reads newline-delimited JSON from stdin, dispatches requests to the tool registry, and writes responses to stdout. Compatible with Claude Desktop, Cursor, and other MCP clients.

## Supported Methods

| Method | Handler | Response |
|--------|---------|----------|
| `initialize` | Inline | Protocol version 2024-11-05, capabilities, server info |
| `tools/list` | Via `registry::get_tools()` | Array of Tool objects |
| `tools/call` | Via `registry::call_tool()` | Tool result with `isError` flag |

## Error Codes

| Code | Meaning |
|------|---------|
| -32700 | Parse error (invalid JSON) |
| -32601 | Method not found |

## Key Implementation Details

- `stdin.lock()` acquired once for the entire loop (buffered I/O)
- Empty lines are skipped (not treated as errors)
- EOF terminates the loop gracefully
- Responses written with `println!()` (auto-flush per line)
- Notifications (null `id`) for unknown methods are silently ignored
