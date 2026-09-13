# VELOCITY-MCP Edge User Guide

Deploy VELOCITY-MCP as a serverless WebAssembly application on Wasmer Edge. Zero infrastructure management, automatic scaling, global CDN distribution.

---

## Table of Contents

1. [Quick Start: Deploy in 5 Minutes](#1-quick-start-deploy-in-5-minutes)
2. [Architecture Overview](#2-architecture-overview)
3. [Configuration Reference](#3-configuration-reference)
4. [API Reference](#4-api-reference)
5. [WASM Plugin Runtimes on Edge](#5-wasm-plugin-runtimes-on-edge)
6. [Security Configuration](#6-security-configuration)
7. [Performance Tuning](#7-performance-tuning)
8. [Troubleshooting Guide](#8-troubleshooting-guide)
9. [Migration from Native Deployment](#9-migration-from-native-deployment)

---

## 1. Quick Start: Deploy in 5 Minutes

### Prerequisites

- Rust toolchain with `wasm32-wasip1` target installed
- Wasmer CLI v7+ installed and authenticated
- Git (to clone the repository)

### Step 1: Install Prerequisites

```bash
# Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Add WASM target
rustup target add wasm32-wasip1

# Install Wasmer CLI
curl -sSf https://get.wasmer.io | sh

# Authenticate with Wasmer
wasmer login
```

### Step 2: Clone and Build

```bash
git clone https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP.git
cd V.E.L.O.C.I.T.Y.-MCP

# Build the WASM binary (optimized for Edge)
cargo build --target wasm32-wasip1 --release --bin velocity-edge
```

### Step 3: Deploy

```bash
# One-command deploy (builds, validates, deploys, and verifies)
# On Linux/macOS:
./deploy-edge.sh

# On Windows:
deploy-edge.bat
```

### Step 4: Verify

```bash
# Replace with your actual Edge endpoint URL
curl https://<your-app>.wasmer.app/health
# Expected: {"status":"healthy","version":"3.2.0"}
```

### Step 5: Send Your First MCP Request

```bash
curl -X POST https://<your-app>.wasmer.app/mcp \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "initialize",
    "params": {
      "protocolVersion": "2024-11-05",
      "capabilities": {},
      "clientInfo": {"name": "test-client", "version": "1.0.0"}
    },
    "id": 1
  }'
```

That is it. Your VELOCITY-MCP server is running on Wasmer Edge.

---

## 2. Architecture Overview

### Edge vs Native Deployment

```
+---------------------------------------------------+
|                 Client (MCP)                       |
+---------------------------------------------------+
          |                              |
          | HTTP/SSE                     | stdio/shmem/NDA
          v                              v
+---------------------+    +----------------------------+
|   Wasmer Edge       |    |   Native Binary            |
|   (WASM Module)     |    |   (Rust executable)        |
|                     |    |                            |
|  - HTTP server      |    |  - stdio JSON-RPC          |
|  - JSON-RPC handler |    |  - NDA binary protocol     |
|  - WASM plugins     |    |  - Shared memory IPC       |
|  - Metering         |    |  - Process sandbox         |
|  - Auto-scaling     |    |  - Full filesystem access  |
+---------------------+    +----------------------------+
   Global CDN                  Local machine
   0-instances idle            Always running
   128MB memory cap            Full system resources
```

### What Runs on Edge

| Feature | Edge | Native |
|---------|------|--------|
| HTTP/SSE transport | Yes | Yes |
| JSON-RPC over HTTP | Yes | Yes |
| Pure-Rust edge tools (10 built-in) | Yes | No |
| WASM plugin runtimes (12 languages) | No | Yes |
| Metering/instruction counting | No | Yes |
| Module compilation caching | No | Yes |
| NDA binary protocol | No | Yes |
| Shared memory IPC | No | Yes |
| Process-based sandbox (seccomp) | No | Yes |
| shell_exec (process spawning) | No | Yes |
| Local filesystem access | Limited | Full |
| Database resources (SQLite) | No | Yes |
| Custom TCP/UDP sockets | No | Yes |
| WebSocket transport | No | Yes |

### Request Lifecycle

```
Client Request
    |
    v
[Wasmer Edge CDN] --> [WASM Instance]
                          |
                     [Parse JSON-RPC]
                          |
                     [Route to handler]
                          |
                     +----+----+
                     |         |
                  /health   /mcp
                     |         |
                     |    [Dispatch tool]
                     |         |
                     |    [Execute WASM plugin]
                     |         |
                     |    [Serialize response]
                     v         v
                  JSON Response
                     |
                     v
                  Client
```

---

## 3. Configuration Reference

All configuration is managed through `wasmer.toml` at the project root. Environment variables can override defaults at deploy time.

### wasmer.toml Configuration

```toml
[package]
name = "velocity-mcp-edge"
version = "3.2.0"
description = "VELOCITY-MCP serverless deployment on Wasmer Edge"

[module]
# Path to the compiled WASM binary
main = "target/wasm32-wasip1/release/velocity_edge.wasm"

[edge]
# Memory allocation per instance (MB)
# Free tier: max 128MB. Production: up to 2048MB
memory_mb = 128

# Maximum WASM instructions per single request
# 5M instructions ~= 50ms compute budget at typical throughput
# Increase for complex tool executions; decrease for stricter cost control
instruction_limit = 5_000_000

# Minimum instances to keep warm (0 = scale to zero when idle)
# Setting to 1 eliminates cold starts at the cost of idle billing
min_instances = 0

# Maximum concurrent instances before rejecting new requests
# Free tier: keep at 3. Production: increase to 10-50
max_instances = 3

[edge.env]
# Logging verbosity: "error", "warn", "info", "debug", "trace"
# Use "warn" on free tier to minimize overhead
RUST_LOG = "warn"

# Optional: API key for request authentication
# VELOCITY_API_KEY = "your-secret-key-here"

# Optional: Maximum request body size in bytes (default: 1MB)
# MAX_BODY_SIZE = "1048576"

# Optional: Allowed CORS origins (comma-separated, or "*" for all)
# ALLOWED_ORIGINS = "*"

# Optional: Rate limit per minute per IP (default: 100, 0 to disable)
# RATE_LIMIT_PER_MINUTE = "100"

[edge.healthcheck]
# Health check endpoint path
path = "/health"

# How often Wasmer checks instance health (seconds)
interval_seconds = 30

# Number of consecutive failures before marking unhealthy
# A lower value detects failures faster but may cause false positives
```

### Environment Variables Reference

| Variable | Default | Description |
|----------|---------|-------------|
| `RUST_LOG` | `warn` | Logging level. Use `warn` on free tier, `info` for debugging |
| `VELOCITY_API_KEY` | (none) | API key for request authentication (X-API-Key header) |
| `MAX_BODY_SIZE` | `1048576` | Maximum request body in bytes (1MB default) |
| `ALLOWED_ORIGINS` | (none) | Comma-separated CORS origins, or "*" for all |
| `RATE_LIMIT_PER_MINUTE` | `100` | Max requests per minute per IP (0 to disable) |
| `PORT` | `8080` | HTTP listen port (overridden by Edge platform) |

### Configuration Examples

**Minimal free-tier deployment:**

```toml
[edge]
memory_mb = 128
instruction_limit = 5_000_000
min_instances = 0
max_instances = 3

[edge.env]
RUST_LOG = "warn"
VELOCITY_API_KEY = "sk-your-secret-key-here"
```

**Production deployment with authentication:**

```toml
[edge]
memory_mb = 512
instruction_limit = 10_000_000
min_instances = 1
max_instances = 20

[edge.env]
RUST_LOG = "info"
VELOCITY_API_KEY = "sk-..."
MAX_BODY_SIZE = "4194304"  # 4MB
ALLOWED_ORIGINS = "*"
RATE_LIMIT_PER_MINUTE = "200"
```

---

## 4. API Reference

### Endpoints

#### `GET /health`

Health check endpoint. Used by Wasmer Edge to verify instance status.

**Response:**
```json
{
  "status": "healthy",
  "version": "3.2.0"
}
```

**HTTP Status Codes:**
- `200 OK` -- Instance is healthy
- `503 Service Unavailable` -- Instance is starting up or unhealthy

#### `POST /mcp`

Main MCP endpoint. Accepts JSON-RPC 2.0 requests.

**Request Headers:**
- `Content-Type: application/json` (required)
- `X-API-Key: <api-key>` (required if `VELOCITY_API_KEY` is set)

**Request Body:** Standard MCP JSON-RPC 2.0 format.

**Response:** Standard MCP JSON-RPC 2.0 response.

#### `POST /`

Alias for `/mcp`. Accepts the same JSON-RPC 2.0 requests.

### Supported MCP Methods

#### `initialize`

Establish a session and negotiate capabilities.

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
  "id": 1,
  "result": {
    "protocolVersion": "2024-11-05",
    "capabilities": {
      "tools": {"listChanged": true},
      "logging": {}
    },
    "serverInfo": {
      "name": "velocity-mcp-edge",
      "version": "3.2.0"
    }
  }
}
```

#### `tools/list`

List all available tools, including WASM plugin tools.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "tools/list",
  "params": {},
  "id": 2
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "result": {
    "tools": [
      {
        "name": "bench_echo",
        "description": "Echo tool for benchmarking latency and throughput",
        "inputSchema": {
          "type": "object",
          "properties": {
            "message": {
              "type": "string",
              "description": "Message to echo back"
            }
          },
          "required": ["message"]
        }
      }
    ]
  }
}
```

#### `tools/call`

Execute a tool by name with the given arguments.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": {
    "name": "bench_echo",
    "arguments": {
      "message": "Hello from Edge!"
    }
  },
  "id": 3
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 3,
  "result": {
    "content": [
      {
        "type": "text",
        "text": "Echo: Hello from Edge!"
      }
    ]
  }
}
```

#### `ping`

Keep-alive ping to verify connectivity.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "ping",
  "id": 4
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 4,
  "result": {}
}
```

### Error Responses

All errors follow the JSON-RPC 2.0 error format:

```json
{
  "jsonrpc": "2.0",
  "id": null,
  "error": {
    "code": -32700,
    "message": "Parse error: unexpected end of input"
  }
}
```

| Error Code | Meaning |
|------------|---------|
| `-32700` | Parse error (invalid JSON) |
| `-32600` | Invalid request (missing required fields) |
| `-32601` | Method not found |
| `-32602` | Invalid params |
| `-32603` | Internal error |
| `401` | Unauthorized (invalid or missing API key) |
| `413` | Request too large (exceeds `MAX_BODY_SIZE`) |
| `429` | Too many requests (rate limit exceeded) |
| `500` | Internal server error |

---

## 5. Edge Tools vs Native WASM Runtimes

### Edge Deployment: Pure-Rust Built-in Tools

The WASM edge deployment uses 10 pure-Rust tools that compile to WebAssembly without external dependencies:

| Tool | Description |
|------|-------------|
| echo | Echo back input message |
| json_format | Parse and pretty-print JSON |
| text_count | Count characters, words, lines, bytes |
| text_transform | Transform text (upper, lower, title, reverse, trim, truncate, replace, slug) |
| math_eval | Evaluate mathematical expressions |
| bench_echo | Generate benchmark payloads of specific size |
| timestamp | Return current UTC timestamp |
| base64_encode | Encode text to Base64 |
| base64_decode | Decode Base64 to text |
| hash_text | Compute SHA-256/SHA-1/MD5 hash |

**Why not WASM language runtimes on Edge?** Wasmer's Cranelift backend JIT-compiles WASM to native machine code. Inside a WASM/WASIX environment, there is no native execution environment — you cannot JIT to code you cannot run. WASM modules also cannot be loaded or instantiated from within a WASM sandbox. The edge deployment therefore uses pure Rust implementations instead.

### Native Deployment: Full WASM Language Runtime Support

The native build supports all 12 WASM language runtimes (QuickJS, MicroPython, Lua, mruby, TinyGo, Rust, PHP, C#, Java, R, Julia, Perl) via the Wasmer runtime with metering and caching. See the main README for details.

---

### Example Plugin Manifest (Edge-Compatible)

```json
{
  "name": "json-transform",
  "version": "1.0.0",
  "tools": [
    {
      "name": "transform_json",
      "description": "Transform JSON data using a JavaScript expression",
      "executor_type": "wasm",
      "language": "javascript",
      "source": "function call_tool(args) { return JSON.stringify(JSON.parse(args.input)); }",
      "inputSchema": {
        "type": "object",
        "properties": {
          "input": {"type": "string", "description": "JSON string to transform"}
        },
        "required": ["input"]
      }
    }
  ]
}
```

---

## 6. Security Configuration

### API Key Authentication

Enable API key authentication by setting the `VELOCITY_API_KEY` environment variable:

```toml
[edge.env]
VELOCITY_API_KEY = "sk-your-secret-key-here"
```

Clients must include the key in the X-API-Key header:

```bash
curl -X POST https://<your-app>.wasmer.app/mcp \
  -H "Content-Type: application/json" \
  -H "X-API-Key: sk-your-secret-key-here" \
  -d '{"jsonrpc":"2.0","method":"ping","id":1}'
```

Without a valid key, requests receive a `401 Unauthorized` response.

### Request Size Limits

Prevent abuse by limiting request body size:

```toml
[edge.env]
MAX_BODY_SIZE = "1048576"  # 1MB default
ALLOWED_ORIGINS = "*"      # CORS origins
RATE_LIMIT_PER_MINUTE = "100"  # Rate limit per IP
```

### Instruction Limits (Metering)

The instruction limit prevents runaway computations and controls costs:

```toml
[edge]
instruction_limit = 5_000_000  # ~50ms compute budget
```

When a tool execution exceeds this limit, the request is terminated and an error is returned to the client. This protects against:
- Infinite loops in plugin code
- Excessive resource consumption
- Unexpected cost spikes

### Security Warnings

1. **Never commit API keys to version control.** Use Wasmer secrets or environment variable injection at deploy time.
2. **HTTPS is enforced** by Wasmer Edge. All traffic is encrypted in transit.
3. **WASM sandboxing** provides process-level isolation for plugin execution. There is no shared state between plugin invocations.
4. **Filesystem access is restricted** to temporary storage on Edge. Plugins cannot access arbitrary filesystem paths.
5. **No process spawning** is available on Edge. The `shell_exec` tool is not functional in Edge mode.

---

## 7. Performance Tuning

### Free Tier Optimization

The free tier has strict limits. These settings maximize throughput within those constraints:

```toml
[edge]
memory_mb = 128          # Minimum viable memory
instruction_limit = 5_000_000  # Keep low to stay within free invocation budget
min_instances = 0        # Scale to zero when idle (no idle charges)
max_instances = 3        # Conservative scaling

[edge.env]
RUST_LOG = "warn"       # Minimal logging reduces CPU overhead
```

### Latency Optimization

To minimize latency at the cost of higher billing:

```toml
[edge]
min_instances = 1        # Keep at least one instance warm (eliminates cold starts)
max_instances = 10       # Allow more concurrent instances

[edge.env]
RUST_LOG = "error"      # Only log errors to minimize I/O
```

### Cold Start Behavior

- **min_instances = 0**: First request after idle may take 100-500ms (cold start). Subsequent requests are fast (<10ms).
- **min_instances = 1**: One instance is always warm. No cold starts, but you are billed for idle time.

### Memory Tuning

If you see out-of-memory errors:

```toml
[edge]
memory_mb = 256  # Increase from 128
```

If you want to minimize cost and memory footprint:

```toml
[edge]
memory_mb = 64  # Tight but possible for simple workloads
```

### Instruction Budget Tuning

| Budget | Approximate Time | Use Case |
|--------|-----------------|----------|
| 1,000,000 | ~10ms | Simple echo, ping, health checks |
| 5,000,000 | ~50ms | Standard tool execution |
| 10,000,000 | ~100ms | Complex data transformation |
| 50,000,000 | ~500ms | Heavy computation, large data processing |

---

## 8. Troubleshooting Guide

### Build Errors

**Error: `target not found: wasm32-wasip1`**

```bash
rustup target add wasm32-wasip1
```

**Error: `cannot find crate 'velocity_mcp_core'`**

Ensure you are building from the workspace root:

```bash
cargo build --target wasm32-wasip1 --release --bin velocity-edge
```

**Error: linker errors or undefined references**

The `velocity-mcp-core` crate must compile for `wasm32-wasip1`. It has no OS-specific dependencies. If you added dependencies to the core crate, verify they support WASM targets.

### Deployment Errors

**Error: `not logged in`**

```bash
wasmer login
```

**Error: `binary too large` or `exceeds maximum module size`**

Check binary size:

```bash
ls -lh target/wasm32-wasip1/release/velocity_edge.wasm
```

If over 5MB, check your dependency tree for heavy crates. The Edge binary should only depend on `velocity-mcp-core`, `hyper`, `tokio`, and `serde_json`.

**Error: `deployment rejected: memory exceeds limit`**

Reduce `memory_mb` in `wasmer.toml` to match your plan limits.

### Runtime Errors

**Health check returns `503`**

The instance is still initializing. Wait 10-30 seconds and retry. If it persists, check logs:

```bash
wasmer edge logs <app-name>
```

**Tool execution returns instruction limit error**

Increase the instruction limit in `wasmer.toml`:

```toml
[edge]
instruction_limit = 10_000_000
```

**Out of memory during tool execution**

Increase memory allocation:

```toml
[edge]
memory_mb = 256
```

**Requests timing out**

Check the instruction limit in `wasmer.toml` and increase if needed. Complex tools may need more than the default 5M instructions. Also verify the tool being called is not hitting an infinite loop or excessive computation.

**`401 Unauthorized` on all requests**

Verify the API key is correct and the `X-API-Key` header is set to the correct key value. Note that empty API keys are treated as disabled authentication.

### Monitoring

```bash
# List your Edge deployments
wasmer edge list

# View real-time logs
wasmer edge logs <app-name>

# View metrics (invocations, errors, latency)
wasmer edge metrics <app-name>
```

---

## 9. Migration from Native Deployment

If you are currently running VELOCITY-MCP as a native binary, here is what changes when moving to Edge:

| Aspect | Native | Edge |
|--------|--------|------|
| Transport | stdio, HTTP, shmem, NDA | HTTP only |
| Tool execution | Native + WASM | WASM only |
| Filesystem | Full access | Temp storage only |
| Process spawning | Available | Not available |
| Scaling | Manual | Automatic |
| Infrastructure | Self-managed | Fully managed |
| Cost | Server cost | Pay-per-invocation |

### What Works Without Changes

- All MCP JSON-RPC methods (initialize, tools/list, tools/call, ping)
- WASM plugin manifests and tool definitions
- Client SDK configurations (just change the URL)

### What Needs Adaptation

- **File operations**: Edge plugins can only write to temp storage. Use external storage (S3, databases) for persistent data.
- **shell_exec**: Not available. Use WASM plugin runtimes for computation instead.
- **Database resources**: Not available on Edge. Connect to external databases via WASM-compatible drivers.
- **NDA binary protocol**: Not available over HTTP. Use JSON-RPC.

### Client Configuration Change

For native deployment:
```json
{
  "mcpServers": {
    "velocity": {
      "command": "./velocity_mcp",
      "args": ["--mode", "stdio"]
    }
  }
}
```

For Edge deployment:
```json
{
  "mcpServers": {
    "velocity-edge": {
      "url": "https://<your-app>.wasmer.app/mcp",
      "headers": {
        "X-API-Key": "sk-your-api-key"
      }
    }
  }
}
```

---

## Additional Resources

- [Operational Runbook](edge_operations.md) -- Monitoring, alerting, incident response
- [Main Deployment Guide](DEPLOYMENT.md) -- Native Docker/Kubernetes deployment
- [Performance Comparison](COMPARISON.md) -- Benchmark data
- [Wasmer Edge Documentation](https://docs.wasmer.io/edge/)
- [MCP Protocol Specification](https://modelcontextprotocol.io/)
