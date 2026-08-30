# NMCP Protocol and Multi-Mode Execution

<cite>
**Referenced Files in This Document**
- [src/main.rs](file://src/main.rs)
- [src/protocol/mod.rs](file://src/protocol/mod.rs)
- [src/protocol/json_rpc.rs](file://src/protocol/json_rpc.rs)
- [src/protocol/nmcp_binary.rs](file://src/protocol/nmcp_binary.rs)
- [src/protocol/nda_native.rs](file://src/protocol/nda_native.rs)
- [src/transport/http.rs](file://src/transport/http.rs)
</cite>

## Overview

The V.E.L.O.C.I.T.Y. MCP Server implements a multi-protocol execution model. The CLI entry point (`main.rs`) parses the `--mode` argument and dispatches to the appropriate protocol handler. All modes share the same tool registry but differ in transport mechanism and serialization format.

## Mode 1: Stdio JSON-RPC v2.0

**Entry**: `protocol::json_rpc::run_stdio_loop()`

The stdio mode implements a standard MCP transport with full spec compliance:
- Reads newline-delimited JSON from stdin via a dedicated reader thread
- Parses each line as a JSON-RPC v2.0 request
- Dispatches to the appropriate handler
- Writes JSON-RPC responses to stdout
- Uses channel architecture so stdin reads never block shutdown checks

### Supported Methods

| Method | Description | Response |
|--------|-------------|----------|
| `initialize` | Server handshake with full capability declaration | Protocol version, capabilities, server info |
| `tools/list` | Enumerate available tools (with cursor pagination) | Array of Tool objects with JSON Schema |
| `tools/call` | Execute a tool by name | Tool result content array |
| `ping` | Keepalive check | Empty result |
| `logging/setLevel` | Set log verbosity | Empty result |
| `notifications/cancelled` | Cancel in-flight request by ID | No response (notification) |
| `health/check` | Server health status | Status, mode, version |

### Error Handling

- **Parse errors** (invalid JSON): Returns `-32700` and continues the loop
- **Method not found**: Returns `-32601` (only for requests with non-null `id`)
- **Tool execution errors**: Returns result with `isError: true` and sanitized error message
- **Request size limit**: Requests > 1 MB rejected before JSON parsing

## Mode 2: HTTP/SSE/WebSocket

**Entry**: `transport::http::run_http_loop()`

The HTTP mode provides a full web transport via Axum:

### Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/mcp` | POST | JSON-RPC over HTTP (stateless) |
| `/mcp/stream` | POST | Streamable HTTP with SSE response |
| `/mcp/batch` | POST | Batch JSON-RPC (multiple requests) |
| `/sse` | GET | SSE endpoint for real-time streaming |
| `/ws` | GET/WS | WebSocket bidirectional transport |
| `/health` | GET | Health check |
| `/performance` | GET | Performance metrics (JSON) |
| `/metrics` | GET | Prometheus metrics export |
| `/sessions` | GET | Active session management |

### Features

- **Session management**: Automatic session creation, tracking, and cleanup (up to 1000 concurrent)
- **API key authentication**: Timing-safe comparison via `Authorization: Bearer <key>` header
- **CORS protection**: Configurable origin restrictions
- **Request size limits**: Configurable max request size (default: 1 MB)
- **TLS/HTTPS**: Configurable certificate and key paths
- **SSE streaming**: Real-time event streaming for tool progress and updates
- **WebSocket**: Bidirectional communication for real-time applications
- **Request ID correlation**: Track requests across connections
- **Connection lifecycle**: Connect/disconnect event handling

## Mode 3: WebSocket (Dedicated)

**Entry**: Via `transport::http.rs` WebSocket upgrade handler

Dedicated WebSocket transport for bidirectional real-time communication. Supports JSON-RPC messages over WebSocket frames with the same method dispatch as stdio mode.

## Mode 4: Shared Memory NMCP Binary

**Entry**: `protocol::nmcp_binary::run_shmem_loop(buffer_path)`

The shared memory mode implements a zero-copy IPC transport with auto-detection of wire format:

### Wire Formats

**NDA-native** (binary, starts with `NMCP`):
```
[4 bytes: magic "NMCP"]
[32 bytes: Merkle root (SHA-256 of payload)]
[1 byte:  method type (0x01=initialize, 0x02=tools/list, 0x03=tools/call, ...)]
[TLV:     request id]
[TLV:     method-specific data]
```

**JSON-RPC** (backwards-compatible): Standard JSON-RPC v2.0 string.

### State Machine

```
IDLE (0) → REQ_READY (1) → PROCESSING (2) → RES_READY (3)
                ↑                                   │
                └───────────────────────────────────┘
                          (back to IDLE on next poll)

Error path: any state → ERROR (4)
```

### Synchronization

- **State byte**: Atomic Acquire/Release ordering
- **Length fields**: `SeqCst` memory fence between length and state
- **Windows**: `CreateEventW`/`WaitForSingleObject`/`SetEvent` for zero-poll blocking waits
- **Other platforms**: 100μs sleep fallback

## NDA Binary Frame Format

The `NmcpBinaryFrame` struct provides zero-allocation parsing of binary frames:

| Field | Size | Description |
|-------|------|-------------|
| Magic | 4 bytes | ASCII `NMCP` signature |
| Merkle Root | 32 bytes | SHA-256 Merkle root of payload |
| Method Type | 1 byte | Encoded method (initialize, tools/list, tools/call, etc.) |
| TLV Data | Variable | Type-Length-Value encoded request data |

**Performance**: 459.1 ns per parse with Merkle verification (1.6x faster than JSON), 12.6 ns for raw shared memory R/W (3.1x faster than JSON shmem).

## Key Design Decisions

1. **Four transport modes**: Each optimized for different use cases — stdio for MCP client compatibility, HTTP for web/SDK access, WebSocket for real-time, shmem for ultra-low latency IPC.
2. **Same tool registry**: All modes call the same `registry::get_tools()` and `registry::call_tool()` — no code duplication.
3. **Auto-detection**: Shared memory mode detects wire format by checking for `NMCP` magic bytes, enabling backwards compatibility.
4. **Full MCP spec compliance**: Supports ping, logging/setLevel, notifications/cancelled, cursor pagination, elicitation, and roots.
5. **Graceful shutdown**: Atomic shutdown flag polled by all modes. Stdio uses reader thread + `recv_timeout` so stdin reads never block shutdown.

**Section sources**
- [src/protocol/json_rpc.rs](file://src/protocol/json_rpc.rs)
- [src/protocol/nmcp_binary.rs](file://src/protocol/nmcp_binary.rs)
- [src/protocol/nda_native.rs](file://src/protocol/nda_native.rs)
- [src/transport/http.rs](file://src/transport/http.rs)
