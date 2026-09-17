# VELOCITY-MCP Edge User Guide

Deploy VELOCITY-MCP as a serverless WebAssembly application on Wasmer Edge. Zero infrastructure management, automatic scaling, global CDN distribution.

---

## Table of Contents

1. [Quick Start: WASIX HTTP Preview](#1-quick-start-wasix-http-preview)
2. [Architecture Overview](#2-architecture-overview)
3. [Configuration Reference](#3-configuration-reference)
4. [API Reference](#4-api-reference)
5. [Edge Tools vs Native WASM Runtimes](#5-edge-tools-vs-native-wasm-runtimes)
6. [Security Configuration](#6-security-configuration)
7. [Performance Tuning](#7-performance-tuning)
8. [Troubleshooting Guide](#8-troubleshooting-guide)
9. [Migration from Native Deployment](#9-migration-from-native-deployment)

---

## 1. Quick Start: WASIX HTTP Preview

**Verified live 2026-09-17 on the public default** (`https://velocity-mcp-edge.wasmer.app`): the WASIX Hyper/Tokio MCP server (~1 MB binary) serves `/health` (200), `initialize`, `tools/list` with request `id` preserved and 10 tools, repeated `echo` calls, and rejects malformed JSON. The steps below rebuild from source and deploy a preview before you activate a public default.

### Prerequisites

- Python 3 (`python` below; use `python3` if required by your installation)
- `cargo-wasix` 0.1.34 and an already installed rustup `+wasix` toolchain
- Wasmer CLI, authenticated for preview deployment
- A local checkout of this repository

### Step 1: Check Prerequisites

```bash
python --version
cargo wasix --version
rustup run wasix rustc --version
wasmer --version
wasmer login
```

Use the WASIX toolchain, not a standard Rust `wasm32-wasip1` target installation, for Edge HTTP.

### Step 2: Build from the Repository Root

```bash
python deploy/build-edge.py
# Output: target/wasm32-wasmer-wasi/release/velocity-edge.wasm
```

The entrypoint copies current `velocity-mcp-core` and `velocity-mcp-edge` sources into an isolated temporary workspace, uses `deploy/edge-wasix.lock` and the official sparse registry `https://cargo-registry.wasix.org/`, and sets `CARGO_HTTP_MULTIPLEXING=false`. The native root `Cargo.lock` and `.cargo/config.toml` remain untouched.

### Step 3: Run Locally, Then Create a Preview

Use the [manifest below](#wasmertoml-configuration): `[[module]]` points to the WASIX artifact, `[[command]]` uses `https://webc.org/runner/wasi`, and `[command.annotations.wasi]` sets `env = ["PORT=80"]`.

```bash
wasmer run .
# In another terminal, check the local HTTP listener:
curl http://localhost:80/health
```

After local checks, create a preview without activating the public default:

```bash
wasmer deploy --no-activate
```

### Step 4: Verify the Preview

Use the preview URL returned by the CLI, not an assumed public-default URL:

```bash
curl https://<preview-host>/health
curl -X POST https://<preview-host>/mcp \
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

Also check `tools/list` (the response must preserve your request `id`) and `tools/call` with `echo`, using the API examples below. Supply `X-API-Key` if authentication is configured. Complete regression and auth/limits/CORS checks before activation.

---

## 2. Architecture Overview

The HTTP implementation is enabled with `cfg(any(not(target_arch = "wasm32"), target_vendor = "wasmer"))`, retaining the existing Hyper/Tokio server and its auth, request/rate limits and CORS on WASIX. `deploy/build-edge.py` compiles the module with a shared linear memory (`+atomics,+bulk-memory,+mutable-globals`), so tokio's `rt-multi-thread` runs a real worker pool (measured `available_parallelism()` = 2 on Edge). Plain WASI CGI is a separate entrypoint. The earlier `wasm32-wasip1` CGI artifact failed in its deployed configuration; this does not establish a general WCGI failure. Note the shared-memory binary does not run under the local `wasmer` 7.4.1 CLI (bind fails with `ENOTSUP`); only Edge's newer runtime serves it, so use a preview/live endpoint — not local `wasmer run` — to smoke-test the multi-threaded build.

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
|  - 10 pure-Rust     |    |  - Shared memory IPC       |
|    built-in tools   |    |  - Process sandbox         |
|  - Auto-scaling     |    |  - WASM plugin runtimes   |
|                     |    |  - Full filesystem access  |
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
| WASM plugin runtimes (7 production + 6 planned) | No | Yes |
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
                     |    [Execute built-in tool]
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

`wasmer.toml` packages the WASIX HTTP command. Configure Edge application settings separately through the supported Wasmer deployment configuration; legacy `[edge]` examples are not a substitute for runner/port configuration.

### wasmer.toml Configuration

```toml
[package]
name = "velocity-mcp-edge"
version = "3.2.0"
description = "VELOCITY-MCP WASIX HTTP server"

[[module]]
name = "velocity-edge"
source = "target/wasm32-wasmer-wasi/release/velocity-edge.wasm"
abi = "wasi"

[[command]]
name = "velocity-edge"
module = "velocity-edge"
runner = "https://webc.org/runner/wasi"

[command.annotations.wasi]
env = ["PORT=80"]
```

Use `[[module]]` with `source`, not the old `[module] main` layout. Keep `PORT=80` for Edge's HTTP listener. Supply other variables through the command environment or deployment-time environment/secrets, rather than `[edge.env]`.

### Environment Variables Reference

| Variable | Default | Description |
|----------|---------|-------------|
| `RUST_LOG` | `warn` | Logging level. Use `warn` on free tier, `info` for debugging |
| `VELOCITY_API_KEY` | (none) | API key for request authentication (X-API-Key header) |
| `MAX_BODY_SIZE` | `1048576` | Maximum request body in bytes (1MB default) |
| `ALLOWED_ORIGINS` | (none) | Comma-separated CORS origins, or "*" for all |
| `RATE_LIMIT_PER_MINUTE` | `100` | Max requests per minute per IP (0 to disable) |
| `PORT` | `8080` | HTTP listen port; explicitly set to `80` in the Edge command annotations |

### Configuration Examples

For request limits and CORS, replace the manifest's existing annotation table (do not append a duplicate):

```toml
[command.annotations.wasi]
env = [
  "PORT=80",
  "RUST_LOG=warn",
  "MAX_BODY_SIZE=1048576",
  "ALLOWED_ORIGINS=https://your-client.example",
  "RATE_LIMIT_PER_MINUTE=100",
]
```

Inject `VELOCITY_API_KEY` through deployment-time environment/secrets rather than committing a key. These settings configure the existing HTTP middleware; verify them on the preview before activation.

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
      "tools": {},
      "resources": {"subscribe": false, "listChanged": false},
      "prompts": {"listChanged": false},
      "elicitation": {},
      "roots": {"listChanged": false}
    },
    "serverInfo": {
      "name": "velocity-mcp-edge",
      "version": "3.2.0"
    }
  }
}
```

#### `tools/list`

List all available tools.

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
        "text": "Hello from Edge!"
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
  "result": {
    "status": "ok"
  }
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

**Why not WASM language runtimes in this Edge artifact?** The current build includes the pure-Rust tools, not the native Wasmer/Cranelift plugin subsystem. WASIX HTTP support does not automatically port that subsystem; nested interpreters or runtime integrations require separate implementation and validation. This is a limitation of this build, not a general prohibition on nested WASM execution.

### Native Deployment: Full WASM Language Runtime Support

The native build supports 7 production WASM language runtimes (QuickJS/JavaScript, QuickJS/TypeScript, MicroPython/Python, Lua, mruby/Ruby, TinyGo/Go, Rust/WASI) and 6 planned runtimes currently implemented as toy interpreters (PHP, C#, Java, R, Julia, Perl) via the Wasmer runtime with metering and caching. See the main README for details.

---

### Example Plugin Manifest (Native Only; Not in the Current Edge Artifact)

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

Enable API key authentication by injecting `VELOCITY_API_KEY` into the command environment through deployment-time environment/secrets. Do not commit keys to `wasmer.toml` or use the legacy `[edge.env]` layout.

Clients must include the key in the X-API-Key header:

```bash
curl -X POST https://<your-app>.wasmer.app/mcp \
  -H "Content-Type: application/json" \
  -H "X-API-Key: sk-your-secret-key-here" \
  -d '{"jsonrpc":"2.0","method":"ping","id":1}'
```

Without a valid key, requests receive a `401 Unauthorized` response.

### Request Size Limits

Set `MAX_BODY_SIZE`, `ALLOWED_ORIGINS` and `RATE_LIMIT_PER_MINUTE` in the command environment, as shown in [Configuration Examples](#configuration-examples). The WASIX HTTP path uses the existing body-size, rate-limit and CORS middleware; test rejection and preflight behavior on the preview.

### Instruction Limits (Metering)

Native plugin metering is separate from the WASIX HTTP server's request/rate limits. The legacy `[edge] instruction_limit` snippets do not establish per-request metering for this build. Validate supported platform resource controls separately; do not convert instruction counts into assumed execution times.

### Security Warnings

1. **Never commit API keys to version control.** Use Wasmer secrets or environment variable injection at deploy time.
2. **HTTPS is enforced** by Wasmer Edge. All traffic is encrypted in transit.
3. **WASM sandboxing** provides process-level isolation for plugin execution. There is no shared state between plugin invocations.
4. **Filesystem access is restricted** to temporary storage on Edge. Plugins cannot access arbitrary filesystem paths.
5. **No process spawning** is available on Edge. The `shell_exec` tool is not functional in Edge mode.

---

## 7. Performance Tuning

The verified WASIX artifact is approximately 1.3 MB. Cold-start latency, warm-request latency and throughput have not been established for this build; the earlier timing estimates and instruction-to-time conversions were not measurements.

### Free Tier Optimization

Check current plan limits and supported application settings before choosing scaling or resource budgets. Legacy `[edge]` templates are not validated tuning instructions for the WASIX runner.

### Latency Optimization

Measure health and MCP calls against the preview, recording client location, payload and concurrency. Do not infer latency from artifact size or successful smoke checks.

### Cold Start Behavior

Measure first-call and subsequent-call behavior separately; no cold-start elimination guarantee is established here.

### Memory Tuning

Use observed memory usage and out-of-memory logs to choose supported platform limits, then repeat representative tool calls.

### Instruction Budget Tuning

Validate any platform instruction budget independently of native plugin metering and HTTP request limits. Instruction counts alone do not specify wall-clock time.

---

## 8. Troubleshooting Guide

### Build Errors

**Missing `cargo wasix` or `wasm32-wasmer-wasi` support**

Check `cargo wasix --version` (verified with 0.1.34) and `rustup run wasix rustc --version`. The Edge build requires the installed `+wasix` toolchain; installing a standard `wasm32-wasip1` target does not provide WASIX HTTP support.

**Registry download or HTTP/2 errors**

Use `python deploy/build-edge.py`, which selects the official sparse registry `https://cargo-registry.wasix.org/` with `CARGO_HTTP_MULTIPLEXING=false`. Check network access to that registry; do not work around this by replacing the native root `Cargo.lock` or `.cargo/config.toml`.

**Missing core crate, dependency resolution or linker errors**

Run the entrypoint from the repository root with current core/edge sources and `deploy/edge-wasix.lock` present:

```bash
python deploy/build-edge.py
```

Check any new dependencies for WASIX compatibility. Building the plain WASI CGI entrypoint or using an old copied workspace does not validate the current HTTP server.

### Deployment Errors

**Error: `not logged in`**

```bash
wasmer login
```

**Error: `binary too large` or `exceeds maximum module size`**

Check the artifact referenced by the manifest:

```bash
ls -lh target/wasm32-wasmer-wasi/release/velocity-edge.wasm
```

The verified build was approximately 1.3 MB. If yours differs substantially, check the release build and dependency changes against current platform limits; size alone does not verify HTTP behavior.

**Error: `deployment rejected: memory exceeds limit`**

Check the supported Edge application resource settings against your plan limits, rather than adding legacy `[edge]` fields to the package manifest.

### Runtime Errors

**Health check returns `503`, or the server does not listen**

Check that `[[module]].source` references the new WASIX artifact, `[[command]].runner` is `https://webc.org/runner/wasi`, and `[command.annotations.wasi]` includes `env = ["PORT=80"]`. Inspect startup logs and test the preview URL returned by the CLI. A stale public-default URL may still refer to an older deployment.

The earlier plain `wasm32-wasip1` CGI entrypoint failed in its deployed configuration. Diagnose the artifact, runner and port together; do not infer that WCGI is globally broken.

**Tool execution returns a resource-limit error or runs out of memory**

Inspect the actual error and supported platform limits before changing resource allocations. Native plugin metering and HTTP body/rate limits are distinct; a legacy `[edge] instruction_limit` entry is not a verified fix for this build.

**Requests timing out**

First verify health and the runner/port configuration, then inspect logs and the tool's input and computation. Repeat against the same preview to avoid testing a different public-default version.

**`tools/list` returns the wrong JSON-RPC `id`**

Rebuild from current sources using `python deploy/build-edge.py` and retest with distinct numeric and string IDs. A request-ID preservation regression fix is under test; confirm the response retains the exact request ID before activation.

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
| Tool execution | Native + WASM runtimes | Pure-Rust built-in tools |
| Filesystem | Full access | Temp storage only |
| Process spawning | Available | Not available |
| Scaling | Manual | Automatic |
| Infrastructure | Self-managed | Fully managed |
| Cost | Server cost | Pay-per-invocation |

### What Works Without Changes

- All 14 MCP JSON-RPC methods (initialize, notifications/initialized, notifications/cancelled, ping, tools/list, tools/call, resources/list, resources/read, resources/templates/list, prompts/list, prompts/get, elicitation/create, roots/list, completion/complete, sampling/createMessage)
- WASM plugin manifests and tool definitions
- Client SDK configurations (just change the URL)

### What Needs Adaptation

- **File operations**: Edge plugins can only write to temp storage. Use external storage (S3, databases) for persistent data.
- **shell_exec**: Not available. Use the built-in pure-Rust tools for computation, or deploy natively if process spawning is required.
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
