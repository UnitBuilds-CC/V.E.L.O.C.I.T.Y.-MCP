# VELOCITY-MCP API Reference

Complete API reference for VELOCITY-MCP server v3.0.0.

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
./velocity_mcp --mode shmem --shmem-path velocity_mcp_shmem
```

---

## HTTP REST API

Base URL: `http://localhost:3000` (default)

### Health Check

**Endpoint:** `GET /health`

Returns server health status.

**Response:**
```json
{
  "status": "healthy",
  "version": "3.0.0",
  "uptime_seconds": 3600,
  "mode": "http"
}
```

**Status Codes:**
- `200 OK` - Server is healthy
- `503 Service Unavailable` - Server is shutting down

---

### Performance Metrics

**Endpoint:** `GET /performance`

Returns real-time performance metrics.

**Response:**
```json
{
  "server": {
    "version": "3.0.0",
    "uptime_seconds": 3600.5,
    "protocol": "MCP",
    "protocol_version": "2024-11-05",
    "runtime": "Rust (native)",
    "transport": "HTTP/SSE"
  },
  "throughput": {
    "total_requests": 15234,
    "requests_per_second": 4.23,
    "successful_requests": 15200,
    "failed_requests": 34
  },
  "latency": {
    "average_us": 164.5,
    "average_ms": 0.1645,
    "total_processing_ms": 2503.2
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
  },
  "vs_nodejs": {
    "estimated_nodejs_latency_us": 625.1,
    "speed_multiplier": "3.8x",
    "total_time_saved_ms": 7023.5,
    "note": "Based on comparative benchmarks of identical MCP workloads"
  }
}
```

---

### Server Metrics

**Endpoint:** `GET /metrics`

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

### List Sessions

**Endpoint:** `GET /sessions`

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

**Endpoint:** `DELETE /sessions/:id`

Deletes a specific session.

**Path Parameters:**
- `id` - Session ID to delete

**Response:**
- `200 OK` - Session deleted
- `404 Not Found` - Session not found

---

### MCP Endpoint (JSON-RPC)

**Endpoint:** `POST /mcp`

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
      "name": "velocity-mcp",
      "version": "3.0.0"
    }
  },
  "id": 1
}
```

---

### Batch Requests

**Endpoint:** `POST /mcp/batch`

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

**Endpoint:** `POST /mcp/stream`

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

**Endpoint:** `GET /sse`

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
      "name": "velocity-mcp",
      "version": "3.0.0"
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

Enable authentication via CLI:
```bash
./velocity_mcp --mode http --api-key your-secret-key
```

Or via config file:
```toml
[http]
api_key = "your-secret-key"
```

### Usage

Include the API key in the `Authorization` header:

```bash
curl -H "Authorization: Bearer your-secret-key" \
     http://localhost:3000/mcp \
     -d '{"jsonrpc":"2.0","method":"ping","id":1}'
```

### Error Responses

**Missing API key:**
```json
{
  "jsonrpc": "2.0",
  "error": {
    "code": -32000,
    "message": "Missing API key"
  },
  "id": null
}
```

**Invalid API key:**
```json
{
  "jsonrpc": "2.0",
  "error": {
    "code": -32000,
    "message": "Invalid API key"
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

Configure via CLI:
```bash
./velocity_mcp --mode http --rate-limit 50 --rate-burst 200
```

Or via config file:
```toml
[http]
rate_limit = 50
rate_burst = 200
```

### Rate Limit Headers

When rate limited, responses include:

```
HTTP/1.1 429 Too Many Requests
Retry-After: 1
X-RateLimit-Limit: 20
X-RateLimit-Remaining: 0
X-RateLimit-Reset: 1234567890
```

### Error Response

```json
{
  "jsonrpc": "2.0",
  "error": {
    "code": -32000,
    "message": "Rate limit exceeded"
  },
  "id": 1
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
  --mode <MODE>              Transport mode: stdio, http, shmem [default: stdio]
  --addr <ADDR>              HTTP listen address [default: 127.0.0.1:3000]
  --api-key <KEY>            API key for authentication
  --rate-limit <NUM>         Requests per second [default: 20]
  --rate-burst <NUM>         Burst request count [default: 100]
  --max-body-size <BYTES>    Max request body size [default: 10485760]
  --cors-origins <ORIGINS>   Comma-separated CORS origins
  --tls-cert <PATH>          TLS certificate file
  --tls-key <PATH>           TLS private key file
  --shmem-path <PATH>        Shared memory path (Windows)
  --config <PATH>            Configuration file path
  --help                     Print help information
  --version                  Print version information
```

### Configuration File

VELOCITY-MCP supports TOML configuration files.

**Example config.toml:**
```toml
[server]
mode = "http"
version = "3.0.0"

[http]
addr = "0.0.0.0:3000"
api_key = "your-secret-key"
rate_limit = 20
rate_burst = 100
max_body_size = 10485760
cors_origins = ["https://example.com", "https://app.example.com"]

[http.tls]
cert = "/path/to/cert.pem"
key = "/path/to/key.pem"

[logging]
level = "info"
format = "json"

[security]
max_sessions = 10000
session_timeout = 1800
enable_audit_log = true

[performance]
enable_metrics = true
metrics_interval = 60
```

**Load configuration:**
```bash
./velocity_mcp --config config.toml
```

### Environment Variables

All configuration options can be set via environment variables with the `VELOCITY_` prefix.

```bash
export VELOCITY_MODE=http
export VELOCITY_HTTP_ADDR=0.0.0.0:3000
export VELOCITY_HTTP_API_KEY=your-secret-key
export VELOCITY_LOG_LEVEL=debug

./velocity_mcp
```

---

## Built-in Tools

VELOCITY-MCP includes 8 built-in tools.

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
- `input_path` (string, required) - Input file path
- `output_path` (string, optional) - Output NDA file path

**Example:**
```json
{
  "name": "convert_to_nda_document",
  "arguments": {
    "input_path": "/path/to/document.txt",
    "output_path": "/path/to/document.nda"
  }
}
```

---

### read_nda

Read an NDA document.

**Parameters:**
- `path` (string, required) - Path to NDA file

**Example:**
```json
{
  "name": "read_nda",
  "arguments": {
    "path": "/path/to/document.nda"
  }
}
```

---

### execute_nda

Execute an NDA document.

**Parameters:**
- `path` (string, required) - Path to NDA file
- `arguments` (array, optional) - Execution arguments

**Example:**
```json
{
  "name": "execute_nda",
  "arguments": {
    "path": "/path/to/script.nda",
    "arguments": ["arg1", "arg2"]
  }
}
```

---

### convert_to_nda_tool

Convert a JSON tool to NDA format for 2.8x faster parsing (measured).

**Parameters:**
- `tool_name` (string, required) - Tool name
- `tool_definition` (object, required) - Tool definition

**Example:**
```json
{
  "name": "convert_to_nda_tool",
  "arguments": {
    "tool_name": "my_tool",
    "tool_definition": {
      "description": "My custom tool",
      "parameters": {
        "type": "object",
        "properties": {
          "input": {"type": "string"}
        }
      }
    }
  }
}
```

---

## Examples

### Complete MCP Session

**1. Initialize:**
```bash
curl -X POST http://localhost:3000/mcp \
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
curl -X POST http://localhost:3000/mcp \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "notifications/initialized"
  }'
```

**3. List tools:**
```bash
curl -X POST http://localhost:3000/mcp \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "tools/list",
    "id": 2
  }'
```

**4. Call a tool:**
```bash
curl -X POST http://localhost:3000/mcp \
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

*Last updated: 2026-08-30*
*Version: 3.0.0*
