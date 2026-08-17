# NMCP Protocol and Dual-Mode Execution

<cite>
**Referenced Files in This Document**
- [src/main.rs](file://src/main.rs)
- [src/protocol/mod.rs](file://src/protocol/mod.rs)
- [src/protocol/json_rpc.rs](file://src/protocol/json_rpc.rs)
- [src/protocol/nmcp_binary.rs](file://src/protocol/nmcp_binary.rs)
</cite>

## Overview

The V.E.L.O.C.I.T.Y. NMCP Server implements a dual-protocol execution model. The CLI entry point (`main.rs`) parses the `--mode` argument and dispatches to the appropriate protocol handler. Both modes share the same tool registry but differ fundamentally in transport mechanism and serialization format.

## Mode 1: Stdio JSON-RPC v2.0

**Entry**: `protocol::json_rpc::run_stdio_loop()`

The stdio mode implements a standard MCP transport:
- Reads newline-delimited JSON from stdin
- Parses each line as a JSON-RPC v2.0 request
- Dispatches to the appropriate handler (`initialize`, `tools/list`, `tools/call`)
- Writes JSON-RPC responses to stdout via `println!`

### Supported Methods

| Method | Description | Response |
|--------|-------------|----------|
| `initialize` | Server handshake | Protocol version, capabilities, server info |
| `tools/list` | Enumerate available tools | Array of Tool objects with JSON Schema |
| `tools/call` | Execute a tool by name | Tool result content array |
| *(other)* | Unknown method | JSON-RPC error -32601 |

### Error Handling

- **Parse errors** (invalid JSON): Returns `-32700` and continues the loop
- **Method not found**: Returns `-32601` (only for requests with non-null `id`)
- **Tool execution errors**: Returns result with `isError: true` and error message text

## Mode 2: Shared Memory NMCP Binary

**Entry**: `protocol::nmcp_binary::run_shmem_loop(buffer_path)`

The shared memory mode implements a zero-copy IPC transport:
- Opens a memory-mapped file buffer (64KB)
- Polls the state byte for `STATE_REQ_READY` (value 1)
- Transitions to `STATE_PROCESSING` (value 2) to lock the buffer
- Reads JSON-RPC request from the input region
- Processes the request through the same tool registry
- Writes JSON-RPC response to the output region
- Transitions to `STATE_RES_READY` (value 3)

### State Machine

```
IDLE (0) → REQ_READY (1) → PROCESSING (2) → RES_READY (3)
                ↑                                   │
                └───────────────────────────────────┘
                          (back to IDLE on next poll)

Error path: any state → ERROR (4)
```

### Polling Strategy

The server uses adaptive backoff with 100-microsecond sleep when the state is not `STATE_REQ_READY`. This prevents CPU pegging while maintaining sub-millisecond response latency.

## NMCP Binary Frame Format

The `NmcpBinaryFrame` struct provides zero-allocation parsing of binary frames:

| Field | Size | Description |
|-------|------|-------------|
| Magic | 4 bytes | ASCII `NMCP` signature |
| Merkle Root | 32 bytes | SHA-256 Merkle root of payload |
| Payload | Variable | Frame content (starts at offset 36) |

**Minimum frame size**: 36 bytes (header only, empty payload)

The parser uses `unsafe` pointer casts to avoid copying the slice data, achieving ~1.54 ns per parse (73x faster than JSON-RPC parsing).

## Key Design Decisions

1. **Both modes use JSON internally**: Even the shared memory mode serializes requests/responses as JSON within the memory-mapped buffer. The binary frame parser is a future optimization path.
2. **Same tool registry**: Both modes call `registry::get_tools()` and `registry::call_tool()` — no code duplication.
3. **State machine locking**: The server transitions to `STATE_PROCESSING` immediately upon detecting `STATE_REQ_READY`, preventing the host from overwriting the buffer mid-processing.
4. **Flush after state changes**: `buffer.flush()` is called after state transitions and writes to ensure the memory-mapped pages are synced to disk.

**Section sources**
- [src/protocol/json_rpc.rs](file://src/protocol/json_rpc.rs)
- [src/protocol/nmcp_binary.rs](file://src/protocol/nmcp_binary.rs)
