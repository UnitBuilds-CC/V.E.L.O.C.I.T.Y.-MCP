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

All endpoints except `GET /health` are mounted under the `/v1` prefix.

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
- `503 Service Unavailable` - Server is shutting down

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
      "resources": { "subscribe": true, "listChanged": true },
      "prompts": { "listChanged": true },
      "sampling": {},
      "logging": {}
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
      "tools": [...]
    },
    "id": 2
  }
]
```

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

**Response (SSE stream):**
```
event: connected
data: {"sessionId": "session-abc123"}

event: resource_update
data: {"uri": "file:///path/to/file.txt", "mime_type": "text/plain"}

:heartbeat

event: request_completed
data: {"sessionId": "session-abc123", "method": "tools/call"}
```

---

## JSON-RPC Protocol

VELOCITY-MCP implements the Model Context Protocol (MCP) v2024-11-05.

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
      "resources": { "subscribe": true, "listChanged": true },
      "prompts": { "listChanged": true },
      "sampling": {},
      "logging": {}
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
    "nextCursor": "next-page-cursor"
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
    ]
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
        "text": "Error type: NOT_FOUND\nFile not found: /path/to/file.txt\n\nSuggestion: Check that the file path is correct and the file exists."
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
    "nextCursor": "next-page-cursor"
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
    "nextCursor": "next-page-cursor"
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

**Valid levels:** `debug`, `info`, `warn`, `error`

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

| Code | Message | Description |
|------|---------|-------------|
| -32000 | Server error | Generic server error |
| -32001 | Unknown tool | Tool not found |
| -32002 | Tool execution failed | Tool returned an error |
| -32003 | Resource not found | Resource URI not found |
| -32004 | Prompt not found | Prompt name not found |
| -32005 | Sampling failed | Sampling request failed |

### Error Response Format

```json
{
  "jsonrpc": "2.0",
  "error": {
    "code": -32001,
    "message": "Unknown tool: nonexistent_tool",
    "data": {
      "available_tools": ["file_read", "file_write", "shell_exec"]
    }
  },
  "id": 1
}
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
- `timeout` (integer, optional) - Timeout in seconds [default: 30]

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

Make an HTTP request with retry logic.

**Parameters:**
- `url` (string, required) - Target URL
- `method` (string, optional) - HTTP method [default: GET]
- `headers` (object, optional) - Request headers
- `body` (string, optional) - Request body
- `timeout` (integer, optional) - Timeout in seconds [default: 30]

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

Convert a JSON tool to NDA format for 2.8x faster parsing (measured).

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

## Support

- **GitHub Issues:** https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/issues
- **Documentation:** https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/wiki
- **Discussions:** https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/discussions

---

*Last updated: 2026-09-02*
*Version: 3.2.0*
