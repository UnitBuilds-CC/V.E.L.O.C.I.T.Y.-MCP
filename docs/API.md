# VELOCITY-MCP API Reference

Complete API reference for VELOCITY-MCP server v3.2.0.

## Table of Contents

- [Transport Modes](#transport-modes)
- [HTTP REST API](#http-rest-api)
- [JSON-RPC Protocol](#json-rpc-protocol)
- [Authentication](#authentication)
- [Rate Limiting](#rate-limiting)
- [Error Codes](#error-codes)
- [Configuration](#configuration)

---

## Transport Modes

VELOCITY-MCP supports three transport modes:

### 1. stdio (Default)
Standard input/output for integration with MCP clients.

```bash
./velocity_mcp --mode stdio
```

### 2. HTTP/SSE
HTTP server with Server-Sent Events for web clients.

```bash
./velocity_mcp --mode http --addr 0.0.0.0:3000
```

### 3. shmem (Windows)
Shared memory IPC for ultra-low latency on Windows.

```bash
./velocity_mcp --mode shmem --buffer-path nmcp_buffer.bin
```

---

## HTTP REST API

Base URL: `http://localhost:3000` (default)

All endpoints except `GET /health` are mounted under the `/v1` prefix. Every `/v1`
route passes through the same middleware stack in this order: rate limiting, API-key
authentication, request validation, request logging. `/health` is unauthenticated.

### Route Index

The complete route table registered by `build_router` (`src/transport/http.rs`). 31
routes: 1 public, 30 under `/v1`. "Detail" marks whether a full request/response
section follows in this document — the routes marked *index only* are documented here
by method, path and purpose, and their response bodies are the Rust types named in the
handler signature, except `/v1/ws`: `impl IntoResponse` names no body, and it answers 101.

| Method | Path | Purpose | Detail |
|---|---|---|---|
| GET | `/health` | Liveness/readiness probe (no auth) | yes |
| POST | `/v1/mcp` | JSON-RPC 2.0 MCP endpoint | yes |
| POST | `/v1/mcp/batch` | JSON-RPC batch requests | yes |
| POST | `/v1/mcp/stream` | Streamable HTTP with progress notifications | yes |
| POST | `/v1/mcp/nda` | NDA binary frames (`application/octet-stream` in and out) | index only |
| GET | `/v1/sse` | Server-sent event stream | yes |
| GET | `/v1/ws` | WebSocket upgrade: one JSON-RPC object per text frame; binary frames and JSON arrays get no reply at all, and text frames over 1 MiB get `-32700` | index only |
| GET | `/v1/performance` | Latency/throughput counters | yes |
| GET | `/v1/metrics` | Server metrics as JSON | yes |
| GET | `/v1/metrics/prometheus` | Same metrics in Prometheus text format | yes |
| GET | `/v1/nda/capabilities` | Per-WASM-tool report of which have a binary (TLV) executor | yes |
| GET | `/v1/sessions` | List active sessions | yes |
| DELETE | `/v1/sessions/:id` | Delete one session | yes |
| GET | `/v1/audit/export/json` | Audit entries from all sessions, JSON — identical body to `/v1/admin/audit/export/json` | index only |
| GET | `/v1/audit/export/csv` | Audit entries from all sessions, CSV — identical body to `/v1/admin/audit/export/csv` | index only |
| POST | `/v1/audit/flush` | Flush all audit logs to disk; a failed flush still answers HTTP 200 with `{"status":"error","error":…}` | index only |
| GET | `/v1/sessions/:id/audit/export/json` | One session's audit entries, JSON | index only |
| GET | `/v1/sessions/:id/audit/export/csv` | One session's audit entries, CSV | index only |
| POST | `/v1/sessions/:id/audit/flush` | Flush one session's audit log; also HTTP 200 on failure | index only |
| GET | `/v1/admin/audit/export/json` | The same all-session aggregate as `/v1/audit/export/json` — a separate handler, identical body, no extra privilege | index only |
| GET | `/v1/admin/audit/export/csv` | The same all-session aggregate as `/v1/audit/export/csv` — a separate handler, identical body, no extra privilege | index only |
| GET | `/v1/admin/audit/summary` | Per-session audit entry counts | index only |
| GET | `/v1/marketplace/plugins` | Search the plugin marketplace (`SearchQuery` → `SearchResults`) | index only |
| GET | `/v1/marketplace/plugins/:id` | One plugin's metadata; 404 if unknown | index only |
| POST | `/v1/marketplace/plugins/:id/review` | Submit a review `{reviewer, rating, comment}`; 201 on success | index only |
| POST | `/v1/marketplace/install/:id` | Install a plugin, then reload `plugins/` | index only |
| DELETE | `/v1/marketplace/install/:id` | Uninstall a plugin, then reload `plugins/` | index only |
| GET | `/v1/marketplace/installed` | Installed plugins | index only |
| GET | `/v1/marketplace/updates` | Available plugin updates | index only |
| POST | `/v1/marketplace/update/:id` | Update one plugin, then reload `plugins/` | index only |
| GET | `/v1/marketplace/stats` | Marketplace aggregate stats | index only |

Authentication is a single shared API key, enforced only when `[http].api_key` is
configured: with no key set, every `/v1` route is open, and with one set, all of them
require `Authorization: Bearer <key>`. There is no role or scope concept — the `admin`
prefix on those three routes denotes cross-session scope, not a higher privilege level.
Marketplace handlers read and write a `marketplace/` directory relative to the server's
working directory, and the audit flush handlers write to `VELOCITY_AUDIT_LOG_PATH`
(default `audit_logs`). All of these routes exist only in a build with the `http` Cargo
feature; the default feature set is empty, so `cargo build --release` alone produces a
binary that prints `Error: HTTP transport not enabled. Build with --features http` and
exits 1 for `--mode http`.

### Health Check

**Endpoint:** `GET /health`

Returns server health status.

**Response:**
```json
{
  "status": "healthy",
  "transport": "http",
  "version": "3.2.0",
  "activeSessions": 3
}
```

**Status Codes:**
- `200 OK` - Server is healthy
- `503 Service Unavailable` - not returned by this endpoint. The shutdown refusal lives on the `/v1` request handlers; `health_check` never reads the shutdown flag, so a probe on an already-open connection reports `healthy` while the server drains

---

### Performance Metrics

**Endpoint:** `GET /v1/performance`

Returns real-time performance metrics.

**Response:**
```json
{
  "server": {
    "version": "3.2.0",
    "uptime_seconds": 3600.5,
    "protocol": "MCP",
    "protocol_version": "2024-11-05",
    "runtime": "Rust (native)",
    "transport": "HTTP/SSE"
  },
  "throughput": {
    "total_requests": 15234,
    "requests_per_second": "4.2",
    "successful_requests": 15200,
    "failed_requests": 34
  },
  "latency": {
    "average_us": "164.5",
    "average_ms": "0.165",
    "total_processing_ms": "2503.2"
  },
  "connections": {
    "active_sse": 5,
    "active_sessions": 12
  },
  "security": {
    "auth_failures": 3,
    "rate_limit_hits": 12,
    "tls_enabled": true,
    "cors_restricted": false,
    "body_size_limit_bytes": 10485760
  }
}
```

---

### Server Metrics

**Endpoint:** `GET /v1/metrics`

Returns detailed server metrics in JSON format.

**Response:**
```json
{
  "total_requests": 15234,
  "successful_requests": 15200,
  "failed_requests": 34,
  "auth_failures": 3,
  "rate_limit_hits": 12,
  "average_latency_us": 164.5,
  "active_sse_connections": 5
}
```

---

### Prometheus Metrics

**Endpoint:** `GET /v1/metrics/prometheus`

Returns the same counters in Prometheus text exposition format
(`text/plain; version=0.0.4`). Emitted metric names:

- `velocity_mcp_requests_total` (counter)
- `velocity_mcp_requests_successful` (counter)
- `velocity_mcp_requests_failed` (counter)
- `velocity_mcp_auth_failures_total` (counter)
- `velocity_mcp_rate_limit_hits_total` (counter)
- `velocity_mcp_latency_microseconds` (gauge)
- `velocity_mcp_sse_connections_active` (gauge)

---

### NDA Capability Report

**Endpoint:** `GET /v1/nda/capabilities`

Reports, per registered **WASM** tool, whether the NDA binary (TLV) argument path is live
for that tool or whether it is served through the decoded-JSON fall-back. The array is
built by `registry::nda_binary_capabilities` (`src/registry.rs:715`) and served by
`nda_capabilities` (`src/transport/http.rs:902`).

**Response** — measured from a freshly started `--features http` process with `plugins/`
loaded, abridged to the first three of its 22 entries:

```json
{
  "protocol": "NDA",
  "binary_executors": 22,
  "json_fallback": 0,
  "tools": [
    { "name": "analyze_logs_go", "language": "go", "binaryExecutor": true },
    { "name": "analyze_logs_js", "language": "javascript", "binaryExecutor": true },
    { "name": "analyze_logs_lua", "language": "lua", "binaryExecutor": true }
  ]
}
```

- `protocol` — the constant string `"NDA"`. It is not a negotiated value and does not
  depend on which transport the client used.
- `binary_executors` / `json_fallback` — the counts of `true` and `false` in `tools`,
  recomputed on every request, so they always agree with the array beside them.
- `tools[]` — one entry per tool whose plugin executor type is `wasm`. Non-WASM tools are
  absent: the same process answers 40 tools on `tools/list` and 22 entries here. Order is
  registration order, not sorted — measured first entry `analyze_logs_go`, last
  `ts_text_stats`.
- `tools[].language` — from the plugin manifest, the empty string when the manifest omits
  it.
- `tools[].binaryExecutor` — `false` only when this process has recorded a JSON fall-back
  for that tool name; `true` otherwise.

The key case is mixed: the three top-level keys are snake_case while the per-tool flag is
camelCase `binaryExecutor`. That is the wire shape, not a typo in this document — a client
must read `binaryExecutor`.

**The flag is learned, not declared.** A tool starts `true` because nothing has failed
yet, and moves to `false` only when a binary attempt provably failed before the guest ran:
the up-front source registration failed (`src/registry.rs:769-777`), or the binary call
returned a pre-execution error that a retry after re-registration did not clear
(`src/registry.rs:792-806`). The record is a process-global in-memory set
(`src/registry.rs:616`), so a restart returns every tool to `true`, and the *reason* for a
`false` is in the server log (`WARN … no NDA-native binary executor, serving via
decoded-JSON path`), not in this payload.

**The count describes the answering process, not the build.** Measured on one working tree: a freshly started process reports 22/0, and once the 22-tool differential has called every WASM tool it reports **21/1** — the single `false` being `analyze_logs_go`, whose Go guest is compiled on demand by a TinyGo toolchain this environment does not provide. A long-running demo server on the same machine answers 15/7 (`analyze_logs_go` plus the six tree-walk `*_greet` tools: `csharp_greet`, `java_greet`, `julia_greet`, `perl_greet`, `php_greet`, `r_greet`); that process started before those six guests were last rebuilt, and the per-tool reasons live in its log. Each `false` is a record of what *this process* has already watched fail, and a restart clears them all, so `binary_executors: 22` is only ever a statement that nothing has been recorded yet — it is not evidence about the deployment. A client that needs to know whether a tool's binary path works has to make the call, not read this endpoint.

**What `true` does not claim.** It says the NDA request reaches that runtime's binary
entry point — no JSON envelope on the wire for the tool. It does not say the TLV bytes are
decoded inside the sandbox; a host-decoded tool reports `true` as well.
`docs/NDA_PROTOCOL.md` §9.2 measures which of the 22 decode in-guest and which the host
decodes before handing the guest JSON.

Always `200` with `Content-Type: application/json` — the handler has no failure path. Like
every other `/v1` route it sits behind the API-key middleware when `[http].api_key` is
configured, and it exists only in a build with the `http` feature.

---

### List Sessions

**Endpoint:** `GET /v1/sessions`

Returns list of active sessions.

**Response:**
```json
{
  "sessions": [
    {
      "id": "session-abc123",
      "created_at": 1234567890,
      "last_activity": 1234567900,
      "request_count": 42
    }
  ]
}
```

---

### Delete Session

**Endpoint:** `DELETE /v1/sessions/:id`

Deletes a specific session.

**Path Parameters:**
- `id` - Session ID to delete

**Response:**
- `200 OK` - Session deleted
- `404 Not Found` - Session not found

---

### MCP Endpoint (JSON-RPC)

**Endpoint:** `POST /v1/mcp`

Main JSON-RPC endpoint for MCP protocol.

**Headers:**
```
Content-Type: application/json
Authorization: Bearer <api-key>  # If authentication enabled
```

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "initialize",
  "params": {
    "protocolVersion": "2024-11-05",
    "capabilities": {},
    "clientInfo": {
      "name": "my-client",
      "version": "1.0.0"
    }
  },
  "id": 1
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": {
    "protocolVersion": "2024-11-05",
    "capabilities": {
      "tools": { "listChanged": true },
      "resources": { "subscribe": false, "listChanged": false },
      "prompts": { "listChanged": false },
      "sampling": {},
      "logging": {},
      "elicitation": {}
    },
    "serverInfo": {
      "name": "velocity-mcp-rust-server",
      "version": "3.2.0"
    }
  },
  "id": 1
}
```

A `true` flag in `capabilities` is a promise that the server emits the matching notification, so only `tools.listChanged` is true: registering a tool at runtime broadcasts `notifications/tools/list_changed` to every `/v1/sse` subscriber within one second. Resource subscriptions and prompt-list changes are recorded in-process but no transport delivers them, so `resources` and `prompts` report false. `sampling` and `elicitation` are client-side capabilities in the MCP spec — the server answers `sampling/createMessage`, `elicitation/create` and `roots/list` when a client sends them, but never initiates them, and `roots` is absent from this map because the map describes what the server can do.

---

### Batch Requests

**Endpoint:** `POST /v1/mcp/batch`

Process multiple JSON-RPC requests in a single HTTP request.

**Request:**
```json
{
  "requests": [
    {
      "jsonrpc": "2.0",
      "method": "ping",
      "id": 1
    },
    {
      "jsonrpc": "2.0",
      "method": "tools/list",
      "id": 2
    }
  ]
}
```

**Response:**
```json
[
  {
    "jsonrpc": "2.0",
    "result": {},
    "id": 1
  },
  {
    "jsonrpc": "2.0",
    "result": {
      "tools": [{ "name": "…", "description": "…", "inputSchema": {} }]
    },
    "id": 2
  }
]
```

**Rules:**

- Batches go to this endpoint wrapped as `{"requests": [...]}`. A bare JSON array posted to
  `/v1/mcp` is refused with HTTP 400 and JSON-RPC error `-32600`, whose message names this endpoint.
- Notifications generate no response object, so a batch of only notifications returns `[]`.
- A batch of more than 100 requests is refused with `-32600` before any request runs.
- A batch element that is not a JSON-RPC request object gets its own `-32600` entry rather than
  a success.

---

### Streamable HTTP

**Endpoint:** `POST /v1/mcp/stream`

Stream JSON-RPC responses via Server-Sent Events.

**Headers:**
```
Content-Type: application/json
Accept: text/event-stream
```

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": {
    "name": "file_read",
    "arguments": {
      "path": "/path/to/file.txt"
    }
  },
  "id": 1
}
```

**Response (SSE stream):**
```
event: session
data: {"sessionId": "session-abc123"}

event: response
data: {"jsonrpc":"2.0","result":{"content":[{"type":"text","text":"file contents"}]},"id":1}

event: complete
data: {}
```

---

### SSE Events

**Endpoint:** `GET /v1/sse`

Subscribe to server-sent events for real-time notifications.

**Query Parameters:**
- `session_id` (optional) - Session ID to associate with connection

**Response (SSE stream, measured wire bytes with chunked framing removed):**
```
event: connected
data: {"sessionId": "framing2"}

: heartbeat

event: notification
data: {"jsonrpc":"2.0","method":"notifications/tools/list_changed"}

: heartbeat

```

**Reading the stream.** Each frame carries its event name and payload separately down to the HTTP layer, so axum writes the `event:` / `data:` lines and a browser `EventSource` gets real named events: register `addEventListener('connected' | 'notification' | 'request_completed', …)` and read `event.data` as JSON directly. There is no `onmessage` fallback to parse — before this fix the server pre-framed each event into one payload string, axum re-wrapped it, and every line came back prefixed with `data:`, so no listener ever saw an event name.

Two keepalive sources are wired on this endpoint — a spawned 15 s task and axum's `KeepAlive` (also 15 s) — but only one comment per period reaches the wire: `KeepAlive` resets its deadline whenever the inner stream yields, so the task's heartbeat always beats it to the punch. Measured cadence over a 40 s idle subscription: `connected` at +87 ms, `: heartbeat` at +99 ms (the interval's first tick fires immediately), then +15,099 ms and +30,098 ms. Note the space: the comment is `: heartbeat`, not `:heartbeat`.

Two frame sources feed this stream:
- `notifications/tools/list_changed`, broadcast as `event: notification` by a 1 s poller that watches the tool-registry generation. Any operation that registers a tool bumps it, e.g. `convert_to_nda_tool` over `POST /v1/mcp`.
- `request_completed`, broadcast as `event: request_completed` by `POST /v1/mcp/stream` only. `POST /v1/mcp` does **not** emit it — measured: a `tools/list` posted to `/v1/mcp` produced no frame for two live subscribers, while the same method posted to `/v1/mcp/stream` reached both (+547 ms and +552 ms). Its payload is `{"method": "...", "sessionId": "..."}`, and the `sessionId` is the *poster's* server-assigned session, not the subscriber's own, so it cannot be used to filter events to yourself.

`POST /v1/mcp/stream` is a separate, self-contained stream: its response body is `text/event-stream` and delivers `event: session` (the poster's new session id) followed by `event: response` (the JSON-RPC result), then closes. It does not require a prior `GET /v1/sse`.

No `resource_update` frame is emitted. `resources/subscribe` records the subscription in-process, but nothing polls or delivers it, which is why `resources.subscribe` is `false` in the capabilities map above.

---

## JSON-RPC Protocol

VELOCITY-MCP implements the Model Context Protocol (MCP) v2024-11-05.

### Method Surface

The dispatcher in `src/protocol/json_rpc.rs` recognises twenty method names. Twelve are
expanded below with request/response examples; the other eight are recorded here, and the
*result* column is what the built server actually answered, not what the spec says it
should answer. Probes ran on 2026-10-11 against `target/debug/velocity_mcp.exe --mode stdio`
(47,076,352 bytes, mtime 2026-10-10 21:31, `serverInfo.version` `3.2.0`). Four of the expanded examples below are illustrative rather than replayable on that build: `resources/read` and `prompts/get` name a file and a prompt that a stock server does not register -- measured, `resources/list` answers `{"resources":[]}` and `prompts/list` answers `{"prompts":[]}` -- so sending those two requests verbatim returns the in-band not-found arm, not the success payload shown, and the same measurement makes the `resources/list` and `prompts/list` examples illustrative too, since each pairs a one-item array with a `nextCursor` that no handler can emit. A paged list carries that field only when the page was cut short by the cap, and its value is the decimal index of the next item, the count `src/protocol/json_rpc.rs:20` names as `DEFAULT_PAGE_SIZE`: the three examples now show the real `"100"` rather than the reference placeholder, which this server never emits and which, resent as a cursor, fails `parse::<usize>()` and restarts the walk at 0. `src/protocol/json_rpc.rs:1558` pins the emitted value with its `assert_eq!(cursor, "100")`. The `tools/call` success example does show `isError`, because every result carries it (measured on 2026-10-11 against a rebuilt release binary). The `tools/call` error example is likewise the real wire shape now: the `Error running tool name: cause` prefix comes from the single `format!` in the dispatch branch and the cause is this host's own error text, so only the `Error type:` / `Hint:` lines are stable across platforms.

| method | expanded below | measured result |
|--------|:--:|---|
| `initialize` | yes | capability map |
| `ping` | yes | `{}` |
| `tools/list` | yes | tool array, `nextCursor` when paginated |
| `tools/call` | yes | tool content blocks; failures come back in-band with `isError: true` |
| `health/check` | no | `{"status":"healthy","mode":"stdio","version":"3.2.0"}` — read the note on `mode` |
| `resources/list` | yes | resource array |
| `resources/read` | yes | `{"contents":[…]}`, or in-band `isError` for an unknown URI |
| `resources/templates/list` | no | `{"resourceTemplates":[]}` on a stock server — a template only exists once something calls `register_resource_template()`, which no built-in path does |
| `resources/subscribe` | no | in-band `{"content":[{"type":"text","text":"Resource not found: <uri>"}],"isError":true}` for an unknown URI — a JSON-RPC *result*, not an error object |
| `resources/unsubscribe` | no | `{"status":"unsubscribed","uri":"<uri>"}` — succeeds whether or not a subscription existed |
| `prompts/list` | yes | prompt array |
| `prompts/get` | yes | description + messages, or in-band `isError` |
| `logging/setLevel` | yes | `{}` |
| `sampling/createMessage` | no | `-32603 "No sampling callback registered"` unless the host registered a callback first, and the built server never registers one -- `register_sampling_callback` has no call site outside library consumers, so this refusal is the only answer a network client can get. `params.modelPreferences` is read under the spec camelCase keys (`costPriority`, `speedPriority`, `intelligencePriority`) and under those spellings only -- the Rust-shaped `cost_priority` / `speed_priority` / `intelligence_priority` / `progress_token` deserialize to `None` with no error at all, and they were the accepted form before the camelCase fix, so a host that sends them gets silently unmodelled preferences; `conversation_id` is unaffected because it was never renamed. `params.metadata` reads `conversation_id` and `progressToken`, and `progressToken` must be a JSON *string* -- a numeric token makes the whole metadata object fail to deserialize and is discarded silently. A `conversation_id` appends every message of the request, verbatim and with no byte cap, to an in-process history bounded only by count (256 conversations, 1,000 messages each), written *before* the refusal, so even the refused request costs the server that memory; nothing in the built server ever reads it back, because the only reader is `create_sampling_request_with_history`, counted at 0 in production in the capability table below |
| `elicitation/create` | no | stub: `{"action":"accept","message":"Elicitation acknowledged"}` — the server does not pause for user input |
| `roots/list` | no | stub: `{"roots":[]}` |
| `connector/call` | no | compiled only with `--features oauth2` (`default = []`); measured `-32603 "Missing connectorId parameter"` — the parameter is `connectorId`, not `connector`. Without the feature the arm does not exist and `_` returns `-32601` |
| `notifications/initialized` | yes | no response |
| `notifications/cancelled` | yes | no response |
| `notifications/progress` | yes | no response |

Any other name reaches the `_` arm: `-32601 Method '<name>' not found` when the request
carries an `id`, and silence when it does not.

**`health/check`'s `mode` is a literal, not the running transport.** The shared JSON-RPC
handler hardcodes `"stdio"`, so `POST /v1/mcp {"method":"health/check"}` on a live HTTP
server returned `{"mode":"stdio","status":"healthy","version":"3.2.0"}` (measured
2026-10-11, 127.0.0.1:3900). The two shared-memory loops overwrite the field with the
accurate value (`"shmem"` for JSON frames, `"shmem-nda"` for NDA-framed calls), so `mode`
is trustworthy for stdio and shmem and wrong for HTTP. Use `GET /health` or `/v1/metrics`
to identify an HTTP server's mode.

### Core Methods

#### initialize

Initialize the MCP session.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "initialize",
  "params": {
    "protocolVersion": "2024-11-05",
    "capabilities": {},
    "clientInfo": {
      "name": "client-name",
      "version": "1.0.0"
    }
  },
  "id": 1
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": {
    "protocolVersion": "2024-11-05",
    "capabilities": {
      "tools": { "listChanged": true },
      "resources": { "subscribe": false, "listChanged": false },
      "prompts": { "listChanged": false },
      "sampling": {},
      "logging": {},
      "elicitation": {}
    },
    "serverInfo": {
      "name": "velocity-mcp-rust-server",
      "version": "3.2.0"
    }
  },
  "id": 1
}
```

---

#### ping

Test connectivity.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "ping",
  "id": 1
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": {},
  "id": 1
}
```

---

#### tools/list

List available tools.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "tools/list",
  "params": {
    "cursor": "optional-cursor-for-pagination"
  },
  "id": 1
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": {
    "tools": [
      {
        "name": "file_read",
        "description": "Read file contents as UTF-8 text...",
        "inputSchema": {
          "type": "object",
          "properties": {
            "path": {
              "type": "string",
              "description": "Absolute path to the file."
            }
          },
          "required": ["path"]
        }
      }
    ],
    "nextCursor": "100"
  },
  "id": 1
}
```

---

#### tools/call

Call a tool.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": {
    "name": "file_read",
    "arguments": {
      "path": "/path/to/file.txt"
    }
  },
  "id": 1
}
```

**Response (success):**
```json
{
  "jsonrpc": "2.0",
  "result": {
    "content": [
      {
        "type": "text",
        "text": "file contents here..."
      }
    ],
    "isError": false
  },
  "id": 1
}
```

**Response (error):**
```json
{
  "jsonrpc": "2.0",
  "result": {
    "content": [
      {
        "type": "text",
        "text": "Error running tool 'file_read': The system cannot find the file specified. (os error 2)\\n\\nError type: NOT_FOUND\\nHint: Check that the path or resource exists."
      }
    ],
    "isError": true
  },
  "id": 1
}
```

---

#### resources/list

List available resources.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "resources/list",
  "params": {
    "cursor": "optional-cursor"
  },
  "id": 1
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": {
    "resources": [
      {
        "uri": "file:///path/to/file.txt",
        "name": "file.txt",
        "description": "Text file",
        "mimeType": "text/plain"
      }
    ],
    "nextCursor": "100"
  },
  "id": 1
}
```

---

#### resources/read

Read a resource.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "resources/read",
  "params": {
    "uri": "file:///path/to/file.txt"
  },
  "id": 1
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": {
    "contents": [
      {
        "uri": "file:///path/to/file.txt",
        "mimeType": "text/plain",
        "text": "file contents..."
      }
    ]
  },
  "id": 1
}
```

---

#### prompts/list

List available prompts.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "prompts/list",
  "params": {
    "cursor": "optional-cursor"
  },
  "id": 1
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": {
    "prompts": [
      {
        "name": "code-review",
        "description": "Review code for best practices",
        "arguments": [
          {
            "name": "code",
            "description": "Code to review",
            "required": true
          }
        ]
      }
    ],
    "nextCursor": "100"
  },
  "id": 1
}
```

---

#### prompts/get

Get a prompt with arguments.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "prompts/get",
  "params": {
    "name": "code-review",
    "arguments": {
      "code": "fn main() { println!(\"Hello\"); }"
    }
  },
  "id": 1
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": {
    "description": "Review code for best practices",
    "messages": [
      {
        "role": "user",
        "content": {
          "type": "text",
          "text": "Please review this code:\nfn main() { println!(\"Hello\"); }"
        }
      }
    ]
  },
  "id": 1
}
```

---

#### logging/setLevel

Set logging level.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "logging/setLevel",
  "params": {
    "level": "debug"
  },
  "id": 1
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "result": {},
  "id": 1
}
```

**Valid levels:** `debug`, `info`, `notice`, `warning`, `error`, `critical`, `alert`, `emergency` -- any other string, including the `warn` that the startup filter accepts, is refused with `-32602`.

---

### Notifications

Notifications are JSON-RPC requests without an `id` field. The server does not send a response.

#### notifications/initialized

Sent by client after successful initialization.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "notifications/initialized"
}
```

---

#### notifications/cancelled

Cancel a pending request.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "notifications/cancelled",
  "params": {
    "requestId": 1,
    "reason": "User cancelled"
  }
}
```

---

#### notifications/progress

Client-reported progress toward a long-running task. The server reads `progressToken`,
`progress` and `total`, writes them to a `debug!` line, and keeps no state — nothing is
forwarded to an SSE subscriber and no stream is advanced. Set `RUST_LOG=debug` to see it.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "notifications/progress",
  "params": {
    "progressToken": "tok-1",
    "progress": 3,
    "total": 10
  }
}
```

**Response:** none. Measured 2026-10-11: the same object with `"id": 2` also produced no
response line — the notification arms return before the `id` is read, so attaching an `id`
does not turn this into a request the client can await. `progressToken` is read as a string,
so a numeric token (which the MCP spec allows) arrives as `null` in the log line.

The NDA binary path routes the same method to opcode `0x12`; see `docs/NDA_PROTOCOL.md`.

---

## Authentication

VELOCITY-MCP supports API key authentication for HTTP transport.

### Configuration

Enable authentication via config file:
```toml
[http]
api_key = "your-secret-key"
```

Or via the environment (no CLI option exists for the API key):
```bash
VELOCITY_API_KEY=your-secret-key ./velocity_mcp --mode http
```

### Usage

Include the API key in the `Authorization` header:

```bash
curl -H "Authorization: Bearer your-secret-key" \
     http://localhost:3000/v1/mcp \
     -d '{"jsonrpc":"2.0","method":"ping","id":1}'
```

### Error Responses

A missing `Authorization` header and a key that does not match both return
`401 Unauthorized` with the same body:

```json
{
  "jsonrpc": "2.0",
  "error": {
    "code": -32000,
    "message": "Unauthorized"
  },
  "id": null
}
```

---

## Rate Limiting

VELOCITY-MCP includes built-in rate limiting to prevent abuse.

### Default Limits

- **20 requests per second** (sustained)
- **100 request burst** (short-term)

### Configuration

Rate limiting is a boolean switch: no CLI option sets the numeric limits and
there is no numeric rate/burst config key.

Via config file:
```toml
[http]
enable_rate_limit = true   # default
```

Or via the environment:
```bash
VELOCITY_ENABLE_RATE_LIMIT=false
```

The numeric bucket values above are compiled in (`src/rate_limit.rs`) and can
only be raised through the `VELOCITY_RATE_LIMIT` (requests/second) and
`VELOCITY_RATE_BURST` (burst capacity) environment variables, e.g. for load
tests.

### Error Response

When the bucket is empty the request is rejected with `429 Too Many Requests`
and a JSON body. No `Retry-After` or `X-RateLimit-*` headers are sent:

```json
{
  "jsonrpc": "2.0",
  "error": {
    "code": -32000,
    "message": "Rate limit exceeded"
  },
  "id": null
}
```

---

## Error Codes

VELOCITY-MCP uses standard JSON-RPC error codes plus custom codes.

### Standard JSON-RPC Errors

| Code | Message | Description |
|------|---------|-------------|
| -32700 | Parse error | Invalid JSON |
| -32600 | Invalid Request | JSON is not a valid Request object |
| -32601 | Method not found | Method does not exist |
| -32602 | Invalid params | Invalid method parameters |
| -32603 | Internal error | Internal JSON-RPC error |

### Custom Errors

Two custom codes exist in the codebase, and neither of them is what a `tools/call`
failure looks like:

| Code | Message | What actually produces it |
|------|---------|---------------------------|
| -32000 | Server error | Every `VelocityError` variant except `Json` (-32700), `Internal` (-32603) and `Protocol` — `Protocol` forwards whatever code its caller passed (`src/error.rs:118`) and has no production call site at all: `VelocityError::protocol` is constructed only by its own two tests (`src/error.rs:195`, `src/error.rs:220`), so the four standard codes above reach clients as literal JSON bodies from the handlers rather than through this enum. The variants that do map to -32000 are `src/error.rs:119` through `src/error.rs:128`. Also the literal bodies of the HTTP 401 and 429 responses (`src/transport/http.rs:334`, `src/transport/http.rs:368`), the Edge payload-too-large / unauthorized / rate-limited frames (`crates/velocity-mcp-edge/src/main.rs:112`), and core's serialization failure (`crates/velocity-mcp-core/src/lib.rs:452`). |
| -32001 | Not found | Only the `velocity-mcp-core` dispatcher, hence only Edge: `resources/read` answers `-32001 "Resource not found: <uri>"` for *any* URI (`crates/velocity-mcp-core/src/lib.rs:332`) and `prompts/get` answers `-32001 "Prompt not found: <name>"` for *any* name (`crates/velocity-mcp-core/src/lib.rs:367`). The native server never emits -32001, and never for an unknown tool. |

No -32002, -32003, -32004 or -32005 appears anywhere in the workspace — no handler,
SDK or test builds one. The four conditions those codes name are reported **in band**
instead: `tools/call`, `resources/read`, `resources/subscribe`, `resources/unsubscribe`
and `prompts/get` answer a JSON-RPC *success* envelope carrying MCP's `isError` flag, so
a failure — including a lookup miss on an unknown tool, resource URI or prompt name —
produces no `error` object at all. The five envelopes are built at
`src/protocol/json_rpc.rs:325` (the shared `tools/call` tail), `src/protocol/json_rpc.rs:357`,
`src/protocol/json_rpc.rs:383`, `src/protocol/json_rpc.rs:401` and
`src/protocol/json_rpc.rs:425`.

The two arms that *do* answer an `error` object on failure answer `-32603` Internal
error, not a custom code: `sampling/createMessage` at `src/protocol/json_rpc.rs:440`, and
`connector/call` at `src/protocol/json_rpc.rs:456` — the latter compiled only under the
`oauth2` feature, so a default build has exactly one such arm.

### Error Response Format

An `error` object holds exactly `code` and `message` (`src/error.rs:135`); no path attaches
a `data` member: of the 34 `"data"` occurrences across `src/`, `crates/` and `benches/`, every
one is a tool-call argument, a webhook payload or a test fixture, and none sits inside an
`error` object. The two shapes a client really
receives, for an unknown *method* and an unknown *tool* respectively:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "error": { "code": -32601, "message": "Method 'nonexistent/method' not found" }
}
```

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "content": [{
      "type": "text",
      "text": "Error running tool 'nonexistent_tool': <sanitized message>\n\nError type: <classification>\nHint: <retry advice>"
    }],
    "isError": true
  }
}
```

The first is the JSON-RPC layer reporting that no such method exists
(`src/protocol/json_rpc.rs:498`); the second is the MCP tool-call contract reporting a
failed call inside a successful response, whose text is built by the format string at
`src/protocol/json_rpc.rs:297`. Over NMCP/NDA binary frames the same failed call is
signalled by the frame status byte (`STATUS_ERROR`) rather than by `isError` — pinned by
`test_nda_frame_tools_call_error_path` (`src/protocol/json_rpc.rs:1159`), which sends
`nonexistent_tool` and asserts that byte.

This section was measured, not read off the enum: feeding those five requests to the stdio
loop of a binary built from the current tree returned `isError: true` results carrying
`Resource not found: …` and `Prompt not found: …` for ids 2 and 3, `-32601` for the unknown
method, and `-32603 "No sampling callback registered"` for sampling — and no `-3200x` code
in any response. To re-measure, save one request per line and pipe them in:

```bash
printf '%s\n' \
 '{"jsonrpc":"2.0","method":"tools/call","id":1,"params":{"name":"nonexistent_tool","arguments":{}}}' \
 '{"jsonrpc":"2.0","method":"resources/read","id":2,"params":{"uri":"invalid://nope"}}' \
 '{"jsonrpc":"2.0","method":"prompts/get","id":3,"params":{"name":"nope","arguments":{}}}' \
 '{"jsonrpc":"2.0","method":"bogus/method","id":4,"params":{}}' \
 '{"jsonrpc":"2.0","method":"sampling/createMessage","id":5,"params":{"messages":[],"maxTokens":1}}' \
 | velocity_mcp --mode stdio
```

---

## Configuration

### CLI Options

```bash
./velocity_mcp [OPTIONS]

Options:
  --config <path>               Path to TOML configuration file. CLI args override file values
  --mode <stdio|shmem|http>     Protocol mode. Default: stdio
  --buffer-path <path>          Path to mapped buffer file. Only used in shmem mode. Default: nmcp_buffer.bin
  --addr <address>              HTTP listen address. Only used in http mode. Default: 0.0.0.0:3000
  --tls-cert <path>             Path to TLS certificate (PEM). Enables HTTPS when paired with --tls-key
  --tls-key <path>              Path to TLS private key (PEM). Enables HTTPS when paired with --tls-cert
  --benchmark                   Run the performance benchmark suite
  -h, --help                    Print this help screen
```

No version-only flag exists; the version (`v3.2.0`) is printed in the
`--help` banner.

### Configuration File

VELOCITY-MCP supports TOML configuration files.

**Example config.toml:**
```toml
# Top-level keys
mode = "http"                                  # stdio | shmem | http
buffer_path = "nmcp_buffer.bin"                # shmem mode only
csharp_path = "NdaMcpServer.exe"               # NDA C# engine delegation
plugin_dir = "plugins"

[http]
addr = "0.0.0.0:3000"
api_key = "your-secret-key"                    # optional; enables Bearer auth
max_request_size = 10485760                    # bytes (10 MB default)
enable_rate_limit = true
cors_origins = ["https://example.com", "https://app.example.com"]

[logging]
level = "info"

[features]
database = false
oauth2 = false
http = false
nda_merkle = true

[wasm_runtimes]
instruction_limit = 10000000

[wasm_runtimes.javascript]
enabled = true
wasm_path = "bench_tools/quickjs_wasm/quickjs.wasm"
```

TLS is not part of the config schema — certificates are supplied only through
the `--tls-cert` / `--tls-key` CLI flags.

**Load configuration:**
```bash
./velocity_mcp --config config.toml
```

### Environment Variables

These settings can be overridden with environment variables (applied after the
config file, so they win):

```bash
export VELOCITY_MODE=http
export VELOCITY_BUFFER_PATH=nmcp_buffer.bin
export VELOCITY_CSHARP_PATH=NdaMcpServer.exe
export VELOCITY_LOG_LEVEL=debug
export VELOCITY_HTTP_ADDR=0.0.0.0:3000
export VELOCITY_API_KEY=your-secret-key
export VELOCITY_MAX_REQUEST_SIZE=10485760
export VELOCITY_ENABLE_RATE_LIMIT=true
export VELOCITY_NDA_MERKLE=true
export VELOCITY_WASM_INSTRUCTION_LIMIT=10000000

./velocity_mcp
```

---

## Built-in Tools

VELOCITY-MCP includes 16 built-in tools.

### file_read

Read file contents as UTF-8 text.

**Parameters:**
- `path` (string, required) - Absolute path to the file

**Example:**
```json
{
  "name": "file_read",
  "arguments": {
    "path": "/path/to/file.txt"
  }
}
```

---

### file_write

Write text content to a file.

**Parameters:**
- `path` (string, required) - Absolute path to the file
- `content` (string, required) - Text content to write

**Example:**
```json
{
  "name": "file_write",
  "arguments": {
    "path": "/path/to/file.txt",
    "content": "Hello, World!"
  }
}
```

---

### shell_exec

Execute a shell command with timeout enforcement.

**Parameters:**
- `command` (string, required) - Shell command to execute
- `timeout` (integer, optional) - Timeout in seconds [default: 30; a value above 300 is clamped to 300]
- `workingDir` (string, optional) - Directory the command runs in. Must be an absolute path and must not contain `..` anywhere in the string; it is checked by the same validator the file tools use, before the command starts

**Example:**
```json
{
  "name": "shell_exec",
  "arguments": {
    "command": "ls -la",
    "timeout": 10
  }
}
```

---

### http_request

Make an HTTP request with retry logic: up to 3 retries with exponential backoff on transport errors and 5xx responses. Requires the `oauth2` feature - on a build without it the tool is still listed, but every call is refused.

**Parameters:**
- `url` (string, required) - Target URL
- `method` (string, optional) - HTTP method [default: GET]
- `headers` (object, optional) - Request headers; only string values are sent, non-string values are silently ignored
- `body` (string, optional) - Request body
- `timeout` (integer, optional) - Timeout in seconds [default: 30; a value above 300 is clamped to 300]

**Example:**
```json
{
  "name": "http_request",
  "arguments": {
    "url": "https://api.example.com/data",
    "method": "POST",
    "headers": {
      "Content-Type": "application/json"
    },
    "body": "{\"key\": \"value\"}",
    "timeout": 60
  }
}
```

---

### convert_to_nda_document

Convert a file to NDA binary format.

**Parameters:**
- `filePath` (string, required) - Input file path
- `outputPath` (string, optional) - Output NDA file path (defaults to the input path with a `.nda` extension)

**Example:**
```json
{
  "name": "convert_to_nda_document",
  "arguments": {
    "filePath": "/path/to/document.txt",
    "outputPath": "/path/to/document.nda"
  }
}
```

---

### read_nda

Read an NDA document.

**Parameters:**
- `ndaPath` (string, required) - Path to NDA file

**Example:**
```json
{
  "name": "read_nda",
  "arguments": {
    "ndaPath": "/path/to/document.nda"
  }
}
```

---

### execute_nda

Execute an NDA document.

**Parameters:**
- `ndaPath` (string, required) - Path to NDA file
- `arguments` (array, optional) - Execution arguments

**Example:**
```json
{
  "name": "execute_nda",
  "arguments": {
    "ndaPath": "/path/to/script.nda",
    "arguments": ["arg1", "arg2"]
  }
}
```

---

### convert_to_nda_tool

Convert a JSON tool to NDA format. Converted calls go through the zero-allocation TLV frame path instead of a JSON parse per call; the built-in `--benchmark` suite measures that parse path against `serde_json`.

**Parameters:**
- `jsonRequest` (string, required) - JSON-RPC tool call to convert
- `outputPath` (string, optional) - Where to write the NDA binary (the tool is registered either way)

**Example:**
```json
{
  "name": "convert_to_nda_tool",
  "arguments": {
    "jsonRequest": "{\"jsonrpc\":\"2.0\",\"method\":\"tools/call\",\"params\":{\"name\":\"my_tool\",\"arguments\":{\"input\":\"value\"}},\"id\":1}",
    "outputPath": "/path/to/my_tool.nda"
  }
}
```

---

## Examples

### Complete MCP Session

**1. Initialize:**
```bash
curl -X POST http://localhost:3000/v1/mcp \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "initialize",
    "params": {
      "protocolVersion": "2024-11-05",
      "capabilities": {},
      "clientInfo": {"name": "test-client", "version": "1.0"}
    },
    "id": 1
  }'
```

**2. Send initialized notification:**
```bash
curl -X POST http://localhost:3000/v1/mcp \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "notifications/initialized"
  }'
```

**3. List tools:**
```bash
curl -X POST http://localhost:3000/v1/mcp \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "tools/list",
    "id": 2
  }'
```

**4. Call a tool:**
```bash
curl -X POST http://localhost:3000/v1/mcp \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "tools/call",
    "params": {
      "name": "file_read",
      "arguments": {"path": "/etc/hostname"}
    },
    "id": 3
  }'
```

---

## Declared Capabilities and What the Server Actually Emits

`initialize` builds one capability map (`src/protocol/mod.rs:19`) and every transport answers from that single
builder. One field is conditional: the `include_extra` argument adds `elicitation`, the JSON-RPC dispatcher
(`src/protocol/json_rpc.rs:151`, reached from stdio and from the HTTP `/mcp` route) and the shared-memory JSON path
(`src/protocol/nmcp_binary.rs:465`) pass `true`, and the NDA binary path (`src/protocol/nmcp_binary.rs:164`) passes
`false`. The rule the builder states for itself is that a flag is a promise that the corresponding notification will
arrive. This table records, per capability, which server-initiated message it promises and how many emission sites
exist in the tree today.

| declared | promised server→client message | emission sites | holds? |
|---|---|---|---|
| `tools: { listChanged: true }` | `notifications/tools/list_changed` | one: the HTTP SSE broadcaster at `src/transport/http.rs:840` | on HTTP only -- stdio, shared-memory and NDA have no broadcaster, so a client on those paths never receives it |
| `resources: { subscribe: false, listChanged: false }` | nothing (both flags false) | 0 for `notifications/resources/updated`, 0 for `.../list_changed` | yes -- no promise is made, none is broken |
| `prompts: { listChanged: false }` | nothing | 0 for `notifications/prompts/list_changed` | yes |
| `sampling: {}` | `sampling/createMessage` sent *by the server* | 0 in production -- the builders at `src/sampling.rs:262` and `src/sampling.rs:300` are reached only from that module's own tests | no |
| `logging: {}` | `notifications/message` | 0 anywhere in the repository | no |
| `elicitation: {}` (every transport but NDA) | `elicitation/create` sent *by the server* | 0 -- the inbound arm at `src/protocol/json_rpc.rs:460` acknowledges a client's request and its own comment says a full implementation would pause and wait | no |

`roots` is deliberately absent: it is a *client* capability, advertised by the client, and has no place in a
server-side map.

The logging row has a second consequence, and until 2026-10-11 it was a divergence rather than a gap.
`logging/setLevel` is dispatched twice, and the two arms did different things with the same request: the
JSON-RPC arm at `src/protocol/json_rpc.rs:180` -- reached from stdio and from the HTTP `/mcp` route
(`src/transport/http.rs:407`) -- refused anything outside the accepted set with `-32602 Invalid log level`,
while the NMCP binary arm at `src/protocol/nmcp_binary.rs:479` -- reached from the shared-memory loop
(`src/main.rs:195`) and from NDA-framed HTTP posts (`src/transport/http.rs:499`) -- validated nothing and
acked any string. The binary arm now calls one parser at `src/protocol/mod.rs:55`, while the JSON-RPC arm named two lines above kept its own inline copy of the same validator instead of calling it, so the four-transport agreement is pinned by test rather than produced by shared code, and an unacceptable level is
refused on all four transports, and `src/protocol/nmcp_binary.rs:2159` compares the two dispatchers' answers
as parsed JSON values over the eight names both arms accepted plus the three only the binary arm ever took. Neither
answer changes behaviour: `LOG_LEVEL` has exactly two writers (`src/protocol/json_rpc.rs:199` and that parser)
and no readers anywhere in the tree, and the server sends `notifications/message` zero times. The filter that
does control the server's own stderr is installed once at startup from `[logging].level` / `VELOCITY_LOG_LEVEL`
(`src/main.rs:125`), and `logging/setLevel` never reaches it.

`velocity-mcp-core` -- the published Edge crate -- already takes the conservative position: its map at
`crates/velocity-mcp-core/src/lib.rs:193` lists `tools`, `resources` and `prompts` only, and because its dispatch
table has no arm for `sampling/createMessage`, `elicitation/create` or `roots/list`, the catch-all at
`crates/velocity-mcp-core/src/lib.rs:147` answers all three `-32601 Method not found` -- the refusal its own test
at `crates/velocity-mcp-core/src/lib.rs:737` pins by method name and code, across five parameter shapes including
one designed to trip validation. A client that needs to know whether a promise is live should treat the flags above
as the measured state rather than as a guarantee, until the capability map is either backed by emission or narrowed
to match it.

---

## Support

- **GitHub Issues:** https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/issues
- **Documentation:** https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/wiki
- **Discussions:** https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/discussions

---

*Last updated: 2026-10-11*
*Version: 3.2.0*
