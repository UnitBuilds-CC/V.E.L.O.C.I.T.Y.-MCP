# V.E.L.O.C.I.T.Y.-MCP User Guide

A complete guide to installing, configuring, and using the V.E.L.O.C.I.T.Y. MCP Server.

---

## Table of Contents

1. [Getting Started](#1-getting-started)
2. [Configuring MCP Clients](#2-configuring-mcp-clients)
3. [Using the Tools](#3-using-the-tools)
4. [NDA Format and Tool Structuring Guide](#4-nda-format-and-tool-structuring-guide)
5. [Transport Modes](#5-transport-modes)
6. [Client SDKs](#6-client-sdks)
7. [Plugin Marketplace](#7-plugin-marketplace)
8. [Monitoring and Observability](#8-monitoring-and-observability)
9. [Configuration](#9-configuration)
10. [Health Checks](#10-health-checks)
11. [Logging and Diagnostics](#11-logging-and-diagnostics)
12. [Performance Benchmarks](#12-performance-benchmarks)
13. [Security Model](#13-security-model)
14. [Troubleshooting](#14-troubleshooting)
15. [Frequently Asked Questions](#15-frequently-asked-questions)

---

## 1. Getting Started

### Prerequisites

- **Rust toolchain** (rustc 1.70+, cargo) — [Install via rustup](https://rustup.rs/)
- **Windows, Linux, or macOS** — Cross-platform support with platform-specific sandboxing

### Building from Source

```bash
# Clone the repository
git clone https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP.git
cd V.E.L.O.C.I.T.Y.-MCP

# Build in release mode (optimized)
cargo build --release

# The executable is at:
# ./target/release/velocity_mcp        (Linux/macOS)
# ./target/release/velocity_mcp.exe    (Windows)
```

### Verifying the Build

```bash
# Run the test suite (1,100+ tests)
cargo test --all-features

# Run the benchmark suite
./target/release/velocity_mcp --benchmark

# Print help
./target/release/velocity_mcp --help
```

### First Run (Stdio Mode)

```bash
./target/release/velocity_mcp --mode stdio
```

The server starts and waits for JSON-RPC requests on stdin. Press `Ctrl+C` to shut down gracefully.

### First Run (HTTP Mode)

```bash
./target/release/velocity_mcp --mode http --addr 0.0.0.0:3000
```

The server starts an HTTP endpoint with JSON-RPC, SSE streaming, WebSocket, and Prometheus metrics.

---

## 2. Configuring MCP Clients

### Claude Desktop

Add the server to your Claude Desktop configuration file:

**Windows:** `%APPDATA%\Claude\claude_desktop_config.json`

```json
{
  "mcpServers": {
    "velocity-mcp": {
      "command": "C:\\path\\to\\target\\release\\velocity_mcp.exe",
      "args": ["--mode", "stdio"]
    }
  }
}
```

### Cursor

Add to your Cursor MCP settings (`.cursor/mcp.json` in your project root):

```json
{
  "mcpServers": {
    "velocity-mcp": {
      "command": "/path/to/target/release/velocity_mcp",
      "args": ["--mode", "stdio"]
    }
  }
}
```

### HTTP Mode (Any Client)

When running in HTTP mode, any HTTP client can connect:

```bash
# Start the server
./velocity_mcp --mode http --addr 0.0.0.0:3000

# Send a JSON-RPC request
curl -X POST http://localhost:3000/v1/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"tools/list","id":1}'
```

### Using Client SDKs

Official client SDKs are available in 4 languages (see [Client SDKs](#6-client-sdks)):

```python
# Python
from velocity_mcp import Client
client = Client("http://localhost:3000")
tools = client.list_tools()
result = client.call_tool("file_read", {"path": "/tmp/data.csv"})
```

### Custom MCP Client (Programmatic)

Any MCP-compatible client can connect via stdio. The server implements the MCP JSON-RPC v2.0 protocol:

```
Client → Server (stdin):  {"jsonrpc":"2.0","method":"initialize","id":1}
Server → Client (stdout): {"jsonrpc":"2.0","id":1,"result":{...}}
```

**Initialization sequence:**
1. Send `initialize` — receive server capabilities and version
2. Send `notifications/initialized` — confirm initialization (no response)
3. Send `tools/list` — discover available tools (supports cursor pagination)
4. Send `tools/call` — invoke a tool

**Additional methods (MCP spec compliant):**
- `ping` — keepalive check, returns empty result
- `logging/setLevel` — set server log verbosity (`debug`, `info`, `warning`, `error`)
- `notifications/cancelled` — cancel an in-flight request by ID

---

## 3. Using the Tools

### Built-in Tools

The server provides 16 built-in tools — four for general operations, four for NDA binary format operations, and eight additional tools. All tools run natively in Rust.

### General Tools

#### file_read

Read file contents with path validation and size limits.

**Parameters:**

| Parameter | Required | Description |
|-----------|----------|-------------|
| `path` | Yes | Absolute path to the file |
| `encoding` | No | Text encoding (default: `utf-8`) |

**Example:**
```json
{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": {
    "name": "file_read",
    "arguments": { "path": "C:\\Users\\me\\data.csv" }
  },
  "id": 1
}
```

#### file_write

Write content to a file with path validation.

**Parameters:**

| Parameter | Required | Description |
|-----------|----------|-------------|
| `path` | Yes | Absolute path for the output file |
| `content` | Yes | Content to write |
| `encoding` | No | Text encoding (default: `utf-8`) |

#### shell_exec

Execute shell commands in a sandboxed environment with timeout and output limits.

**Parameters:**

| Parameter | Required | Description |
|-----------|----------|-------------|
| `command` | Yes | Shell command to execute |
| `timeout` | No | Timeout in seconds (default: 30, max: 30) |

**Security:** Commands run in a capability-based sandbox with no network access, filesystem isolated to a temp directory, and memory limits enforced by OS-level job objects (Windows) or seccomp filters (Linux).

#### http_request

Make HTTP requests with automatic retry logic and circuit breaker protection.

**Parameters:**

| Parameter | Required | Description |
|-----------|----------|-------------|
| `url` | Yes | Target URL |
| `method` | No | HTTP method (default: `GET`) |
| `headers` | No | Request headers as key-value pairs |
| `body` | No | Request body |
| `timeout` | No | Request timeout in seconds (default: 30) |

### NDA Tools

#### convert_to_nda_document

Convert any file into a cryptographically signed `.nda` binary document with semantic triples and visual display commands.

**Parameters:**

| Parameter | Required | Description |
|-----------|----------|-------------|
| `filePath` | Yes | Absolute path to the input file |
| `outputPath` | No | Absolute path for the output `.nda` file. Defaults to input path with `.nda` extension. |

**Supported input types:** C# source code, PDF, CSV, Excel, DOCX, Image (PNG, JPG, WebP), Zip archives, and other file formats.

#### convert_to_nda_tool

Convert a JSON-RPC tool call into native NDA binary format for faster binary parsing.

**Parameters:**

| Parameter | Required | Description |
|-----------|----------|-------------|
| `jsonRequest` | Yes | JSON-RPC tool call request to convert |
| `outputPath` | No | Path to write the NDA binary file. If omitted, returns base64-encoded binary data. |

**NDA binary format:**
- `[4 bytes: magic "NMCP"]`
- `[32 bytes: merkle root (SHA-256 of payload)]`
- `[1 byte: method type (1=tools/call)]`
- `[2 bytes: tool name length]`
- `[N bytes: tool name]`
- `[2 bytes: arguments length]`
- `[M bytes: arguments as binary key-value pairs]`

The native NDA binary format enables faster parsing than JSON through zero-copy pointer casts and TLV binary encoding. Measured speedup is 1.6x for parse + field extraction with Merkle integrity verification, and 3.1x for raw shared memory throughput.

#### read_nda

Read and inspect a compiled `.nda` binary file.

**Parameters:**

| Parameter | Required | Description |
|-----------|----------|-------------|
| `ndaPath` | Yes | Absolute path to the `.nda` file |

**Returns:** Semantic triples, visual display commands, string pool contents, Merkle verification status, and Ed25519 signature status.

#### execute_nda

Execute a runnable `.nda` container in a capability-based sandbox.

**Parameters:**

| Parameter | Required | Description |
|-----------|----------|-------------|
| `ndaPath` | Yes | Absolute path to the runnable `.nda` file |
| `arguments` | No | Array of command-line arguments to pass to the executable or script |

**Execution modes:**
- Compiled C# binary → runs in-memory
- Script (Python, Node.js, PowerShell, Bash) → executes via the corresponding shell process

### Dynamic Tool Hosting

The server can also discover additional tools from plugins or a C# backend engine. When a client sends a `tools/list` request:

1. The server returns the 16 built-in tools (always available)
2. It loads any installed plugins and queries the C# engine (if present)
3. Results are cached for subsequent requests
4. Built-in and discovered tools are merged (deduplicated by name)

---

## 4. NDA Format and Tool Structuring Guide

The NDA (Neural Document Archive) format is the core data format that the V.E.L.O.C.I.T.Y. server operates on.

### What is an NDA File?

An `.nda` file is a **cryptographically signed binary container** that encapsulates:

- **Semantic Triples** — Structured knowledge representations (subject-predicate-object) that capture the semantic meaning of the original content
- **Visual Display Commands** — Instructions for rendering the content visually (layout, formatting, display logic)
- **String Pool** — An optimized, deduplicated pool of all string data referenced by the triples and display commands
- **Executable Payloads** (optional) — Compiled binaries or scripts that can be executed in-memory

The NDA format is designed for:
- **Zero-trust distribution** — Cryptographic signing ensures content integrity and authenticity
- **Semantic search** — Triples enable AI agents to query the *meaning* of content, not just text matching
- **Compact storage** — Binary encoding with string deduplication is significantly smaller than source formats
- **In-memory execution** — Runnable NDAs execute without writing to disk

### The NDA Pipeline: Convert → Read → Execute

```
┌─────────────┐  convert_to_nda_document  ┌──────────────┐     read_nda      ┌────────────────┐
│ Source File  │ ─────────────────────────▶ │   .nda File  │ ────────────────▶ │ Inspect Content│
│ .cs .pdf     │                           │ (signed,     │                   │ (triples,      │
│ .csv .xlsx   │                       │  binary,     │                   │  display cmds, │
│ .png .zip    │                       │  compact)    │                   │  string pool)  │
└─────────────┘                        └──────┬───────┘                   └────────────────┘
                                              │
                                              │ execute_nda
                                              ▼
                                       ┌──────────────┐
                                       │  In-Memory   │
                                       │  Execution   │
                                       │  (sandboxed) │
                                       └──────────────┘
```

### Supported Source File Types

| File Type | Extension | What Gets Encapsulated |
|-----------|-----------|----------------------|
| **C# Source Code** | `.cs` | Compiled assembly + semantic analysis triples + type hierarchy |
| **PDF Documents** | `.pdf` | Extracted text triples + page layout commands |
| **CSV Data** | `.csv` | Column schema triples + row data triples + statistical metadata |
| **Excel Workbooks** | `.xlsx` | Sheet structure triples + cell data + formula dependency graph |
| **Word Documents** | `.docx` | Paragraph triples + style information + document structure |
| **Images** | `.png`, `.jpg`, `.webp` | Visual display commands + metadata triples + pixel data |
| **Zip Archives** | `.zip` | File manifest triples + embedded file payloads |
| **Source code** | common programming languages | Syntax-colored code editor layout |
| **Other formats** | Any | Raw binary payload + file-type metadata triples |

### Structuring Tools for NDA Support

If you are building tools that will be invoked through the V.E.L.O.C.I.T.Y. server, follow these guidelines:

#### 1. Standard Input/Output Contract

Your tool must communicate via **JSON-RPC over stdin/stdout**:

**Request format:**
```json
{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": {
    "name": "your_tool_name",
    "arguments": { ... }
  },
  "id": 999
}
```

**Response format:**
```json
{
  "jsonrpc": "2.0",
  "id": 999,
  "result": {
    "content": [{ "type": "text", "text": "Your tool's output here" }],
    "isError": false
  }
}
```

#### 2. File Path Requirements

All file paths must be **absolute**. The server validates this before your tool receives the request:

| Valid | Invalid | Reason |
|-------|---------|--------|
| `C:\Users\me\file.nda` | `file.nda` | Relative path |
| `/home/user/file.nda` | `.\output.nda` | Relative with `.` |
| `D:\Projects\output.nda` | `C:\Users\..\Windows\f.txt` | Path traversal (`..`) |

#### 3. Creating Runnable NDAs

1. **Compile your code** to an executable or script
2. **Convert it** using `convert_to_nda_document`
3. **Execute it** using `execute_nda` — runs in-memory within the sandbox

### NDA File Inspection with read_nda

**What you get back:**

```
Semantic Triples:
  Subject          Predicate           Object
  ─────────────────────────────────────────────────
  :document        :hasTitle           "My Document"
  :document        :hasAuthor          "UnitBuilds"
  :class:MyClass   :hasMethod          :method:Process

Visual Display Commands:
  [PAGE 1] Layout: A4, Portrait
  [BLOCK 1] TextBlock at (72, 72) size (468, 600)

String Pool:
  0: "My Document"
  1: "UnitBuilds"
  ... (N unique strings, deduplicated)

Merkle Root: VERIFIED
Signature: VERIFIED (Ed25519)
```

---

## 5. Transport Modes

The server supports three transport modes for different use cases: `stdio`, `http`, and `shmem`.

### Stdio Mode (Default)

```bash
velocity_mcp --mode stdio
```

Standard input/output transport. Compatible with all MCP clients (Claude Desktop, Cursor, IDE plugins). JSON-RPC v2.0 over stdin/stdout.

### HTTP/SSE Mode

```bash
velocity_mcp --mode http --addr 0.0.0.0:3000
```

Full HTTP transport with:
- **JSON-RPC over HTTP POST** at `/v1/mcp` (plus `/v1/mcp/batch` for batched requests and `/v1/mcp/nda` for NDA-native frames)
- **Streamable HTTP** at `/v1/mcp/stream` (POST with SSE response)
- **SSE streaming** at `/v1/sse` for real-time event streaming
- **WebSocket** at `/v1/ws` for bidirectional communication
- **Session management** with automatic session creation and cleanup
- **API key authentication** via `Authorization: Bearer <key>` header, with the key set in `[http].api_key` or `VELOCITY_API_KEY`
- **CORS** with origin restrictions configured through `[http].cors_origins`
- **JSON metrics** at `/v1/metrics` and **Prometheus exposition** at `/v1/metrics/prometheus`
- **Health check** at `/health` (the only route outside `/v1`, so it is unauthenticated)
- **Performance metrics** at `/v1/performance`
- **Request size limits** (default: 10 MB, set by `[http].max_request_size`)
- **TLS/HTTPS** support via the `--tls-cert` / `--tls-key` flags (both must be provided together)

### Shared Memory Mode

```bash
velocity_mcp --mode shmem --buffer-path /tmp/nmcp_buffer.bin
```

Memory-mapped file IPC for ultra-low latency communication. Supports two wire formats with auto-detection:

**NDA-native** (binary, starts with `NMCP`):
```
[4 bytes: magic "NMCP"]
[32 bytes: Merkle root (SHA-256 of payload)]
[1 byte:  method type (0x01=initialize, 0x02=tools/list, 0x03=tools/call, ...)]
[TLV:     request id]
[TLV:     method-specific data]
```

**JSON-RPC** (backwards-compatible): Standard JSON-RPC v2.0 string.

**Buffer layout (64 KB):**

| Offset | Size | Field |
|--------|------|-------|
| 0 | 1 byte | State (0=Idle, 1=Request Ready, 2=Processing, 3=Response Ready, 4=Error) |
| 1–4 | 4 bytes | Input buffer length (u32, little-endian) |
| 5–8 | 4 bytes | Output buffer length (u32, little-endian) |
| 10–4095 | 4086 bytes | Input request buffer |
| 4096–65535 | 61440 bytes | Output response buffer |

**Synchronization:**
- State byte: Atomic Acquire/Release ordering
- Length fields: `SeqCst` fence between length and state
- Windows: `CreateEventW`/`WaitForSingleObject`/`SetEvent` for zero-poll blocking
- Other platforms: 100μs sleep fallback

---

## 6. Client SDKs

Client SDKs are provided in 4 languages: the Rust client crate in `client/` (package `velocity-mcp-client`), and the Python, TypeScript and Go SDKs in `sdk/python`, `sdk/typescript` and `sdk/go`.

### Rust

```rust
use velocity_mcp_client::Client;

let client = Client::new("http://localhost:3000");
let tools = client.list_tools().await?;
let result = client.call_tool("file_read", &json!({"path": "/tmp/data.csv"})).await?;
```

### Python

```python
from velocity_mcp import Client

client = Client("http://localhost:3000")
tools = client.list_tools()
result = client.call_tool("file_read", {"path": "/tmp/data.csv"})
```

### TypeScript

```typescript
import { Client } from 'velocity-mcp-client';

const client = new Client('http://localhost:3000');
const tools = await client.listTools();
const result = await client.callTool('file_read', { path: '/tmp/data.csv' });
```

### Go

```go
import velocity_mcp "github.com/UnitBuilds-CC/velocity-mcp/sdk/go"

client := velocity_mcp.NewClient("http://localhost:3000")
tools, err := client.ListTools()
result, err := client.CallTool("file_read", map[string]interface{}{"path": "/tmp/data.csv"})
```

---

## 7. Plugin Marketplace

The server includes a plugin marketplace for discovering, installing, and managing plugins that extend the server's capabilities.

The marketplace has **no CLI subcommands** — it is driven entirely through the HTTP API while the server runs in HTTP mode. Every route below is under `/v1` and requires the `Authorization: Bearer <key>` header when `[http].api_key` (or `VELOCITY_API_KEY`) is set.

### Searching for Plugins

```bash
curl -H "Authorization: Bearer $VELOCITY_API_KEY" \
  "http://localhost:3000/v1/marketplace/plugins?query=data-analysis&sort_by=downloads&limit=20"
```

### Installing a Plugin

```bash
curl -X POST -H "Authorization: Bearer $VELOCITY_API_KEY" \
  http://localhost:3000/v1/marketplace/install/author.plugin-name
```

### Managing Plugins

```bash
# List installed plugins
curl -H "Authorization: Bearer $VELOCITY_API_KEY" \
  http://localhost:3000/v1/marketplace/installed

# Check for available updates
curl -H "Authorization: Bearer $VELOCITY_API_KEY" \
  http://localhost:3000/v1/marketplace/updates

# Update a plugin
curl -X POST -H "Authorization: Bearer $VELOCITY_API_KEY" \
  http://localhost:3000/v1/marketplace/update/author.plugin-name

# Remove a plugin
curl -X DELETE -H "Authorization: Bearer $VELOCITY_API_KEY" \
  http://localhost:3000/v1/marketplace/install/author.plugin-name

# Review a plugin (1-5)
curl -X POST -H "Authorization: Bearer $VELOCITY_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"reviewer":"me","rating":5,"comment":"works"}' \
  http://localhost:3000/v1/marketplace/plugins/author.plugin-name/review
```

### Plugin Types

| Type | Language | Description |
|------|----------|-------------|
| **Python** | Python | Scripts and modules executed via Python runtime |
| **Node.js** | JavaScript/TypeScript | Executed via Node.js runtime |
| **Rust** | Rust | Native compiled plugins with direct API access |
| **WASM** | 7 production + 6 tree-walk | Production runtimes: JavaScript, TypeScript, Python, Ruby, Lua, Go, Rust. Tree-walk interpreters (PHP, C#, Java, R, Julia, Perl) share a single C core |

### Plugin Security

All plugins run in the capability-based sandbox:
- No network access by default
- Filesystem isolated to sandbox temp directory
- Memory limits enforced (256 MB default)
- Linux: seccomp syscall filtering
- Windows: Job Object memory caps
- All violations logged to audit trail

See [Plugin Marketplace Guide](MARKETPLACE.md) for details.

---

## 8. Monitoring and Observability

### Prometheus Metrics

When running in HTTP mode, Prometheus-format metrics are exposed at `/v1/metrics/prometheus`, and the same counters as JSON at `/v1/metrics`:

```bash
curl -H "Authorization: Bearer $VELOCITY_API_KEY" \
  http://localhost:3000/v1/metrics/prometheus
```

**Exported metrics (7):**
- `velocity_mcp_requests_total` — Total requests received (counter)
- `velocity_mcp_requests_successful` — Successful requests (counter)
- `velocity_mcp_requests_failed` — Failed requests (counter)
- `velocity_mcp_auth_failures_total` — Authentication failures (counter)
- `velocity_mcp_rate_limit_hits_total` — Requests rejected by the rate limiter (counter)
- `velocity_mcp_latency_microseconds` — Average request latency in microseconds (gauge)
- `velocity_mcp_sse_connections_active` — Currently active SSE connections (gauge)

### Prometheus Alerting Rules

Pre-configured alerting rules are available in `deploy/prometheus/alerts.yml`:
- `HighErrorRate` / `CriticalErrorRate` — failed-request ratio over 5m / 2m windows
- `HighLatency` — latency increase over 5 minutes
- `LowThroughput` — request rate dropping below 1 r/s
- `HighRateLimitHits` — more than 10 rate-limit rejections per second
- `HighMemoryUsage` / `HighCPUUsage` — container resource pressure
- `TLSCertificateExpiringSoon` / `TLSCertificateExpiringImminently`
- `PluginInstallationFailures` — failed plugin installs
- `AuditLogBufferFull` — audit ring buffer above 9,000 entries

### Grafana Dashboard

A pre-built Grafana dashboard is available in `deploy/grafana/dashboards/velocity-mcp.json` (datasource in `deploy/grafana/datasources/prometheus.yml`) with panels for:
- Total Requests
- Success Rate
- Request Rate
- Error Rate
- Active Sessions
- Rate Limit Hits
- Plugin Statistics

### OpenTelemetry

The `observability` feature compiles in the `observability` module, which provides `init_observability(otlp_endpoint)` for setting up an OTLP tracer pipeline:

```bash
cargo build --release --features observability
```

The `velocity_mcp` binary does not call this itself, so spans are not exported automatically; embedders that use the library set up the tracer (passing the collector endpoint, e.g. `http://localhost:4317`) before serving requests.

### Health and Performance Endpoints

```bash
# Health check (top-level, no /v1 prefix, no auth)
curl http://localhost:3000/health

# Performance metrics (JSON)
curl -H "Authorization: Bearer $VELOCITY_API_KEY" \
  http://localhost:3000/v1/performance
```

---

## 9. Configuration

### Configuration File (TOML)

The server can be configured via a TOML file:

```bash
velocity_mcp --config config.toml
```

**Example `config.toml`:**

```toml
# Top-level keys
mode = "http"                     # stdio | shmem | http (default: stdio)
buffer_path = "nmcp_buffer.bin"   # shmem mode only
csharp_path = "NdaMcpServer.exe"  # optional C# engine used for dynamic tool hosting
plugin_dir = "plugins"

[http]
addr = "0.0.0.0:3000"
api_key = "secret-key-123"        # omit (or leave unset) to disable auth
max_request_size = 10485760       # 10 MB default
enable_rate_limit = true          # boolean only; the token-bucket sizes are not configurable here
cors_origins = ["https://myapp.example.com"]

[logging]
level = "info"                    # error | warn | info | debug | trace

[features]
database = false                  # database resource adapters
oauth2 = false                    # OAuth2 connector framework
http = false                      # HTTP transport (build feature toggle)
nda_merkle = true                 # NDA frame Merkle integrity verification (disable for max throughput on trusted transports)

[wasm_runtimes]
# Per-call instruction limit for WASM plugin runtimes (metering middleware).
# Default: 10,000,000 (~1-5 seconds of computation). Set to 0 to disable metering
# entirely — NOT recommended for production since runaway tools would loop forever.
# Note: interpreter bootstrap consumes metered instructions too, so very low
# values trap during runtime creation. QuickJS needs >1,000,000 just to start.
instruction_limit = 10_000_000

# Each of the 13 languages (7 production runtimes + 6 tree-walk interpreters)
# has its own `enabled` + `wasm_path` block, e.g.
[wasm_runtimes.javascript]
enabled = true
wasm_path = "bench_tools/quickjs_wasm/quickjs.wasm"
```

TLS/HTTPS is not a config-file option: pass `--tls-cert <path>` and `--tls-key <path>` on the command line (both are required together) to serve HTTPS in HTTP mode.

### CLI Options

| Flag | Description | Default |
|------|-------------|---------|
| `--mode <stdio\|shmem\|http>` | Transport mode | `stdio` |
| `--addr <address>` | HTTP listen address (http mode only) | `0.0.0.0:3000` |
| `--config <path>` | Configuration file path | — |
| `--buffer-path <path>` | Shared memory buffer file (shmem mode) | `nmcp_buffer.bin` |
| `--tls-cert <path>` | TLS certificate (PEM); enables HTTPS when paired with `--tls-key` | — |
| `--tls-key <path>` | TLS private key (PEM); enables HTTPS when paired with `--tls-cert` | — |
| `--benchmark` | Run performance benchmarks | — |
| `-h, --help` | Print help | — |

### Feature Flags

Build with optional features using `--features`:

| Feature | Description |
|---------|-------------|
| `http` | HTTP/SSE/WebSocket transport with session management |
| `oauth2` | OAuth2 connector framework with token refresh |
| `database` | Database resource adapters (SQLite) |
| `observability` | OpenTelemetry distributed tracing and metrics |
| `wasm-networking` | Networking syscalls for WASM plugin runtimes |

Example: `cargo build --release --features http,oauth2,observability`

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `VELOCITY_MODE` | Override the transport mode (`stdio`/`shmem`/`http`) | `stdio` |
| `VELOCITY_BUFFER_PATH` | Override the shared memory buffer path | `nmcp_buffer.bin` |
| `VELOCITY_CSHARP_PATH` | Path to C# NdaMcpServer.exe (for dynamic tool hosting) | `NdaMcpServer.exe` |
| `VELOCITY_LOG_LEVEL` | Override the configured log level | `info` |
| `VELOCITY_HTTP_ADDR` | Override the HTTP listen address | `0.0.0.0:3000` |
| `VELOCITY_API_KEY` | Set the HTTP API key (alternative to `[http].api_key`) | — (no auth) |
| `VELOCITY_MAX_REQUEST_SIZE` | Override the maximum HTTP request body size in bytes | `10485760` |
| `VELOCITY_ENABLE_RATE_LIMIT` | Enable/disable the HTTP rate limiter | `true` |
| `VELOCITY_RATE_LIMIT` | Token-bucket refill rate (tokens/sec) for the global limiter | `20` |
| `VELOCITY_RATE_BURST` | Token-burst capacity for the global limiter | `100` |
| `VELOCITY_NDA_MERKLE` | NDA frame Merkle integrity verification (`0`/`false`/`no` to disable) | `1` |
| `VELOCITY_WASM_INSTRUCTION_LIMIT` | Per-call WASM metering instruction limit (`0` disables metering) | `10000000` |
| `RUST_LOG` | Log level for the tracing subscriber (`error`, `warn`, `info`, `debug`, `trace`) | `info` |

---

## 10. Health Checks

### Stdio Mode

```json
→ {"jsonrpc":"2.0","method":"health/check","id":1}
← {"jsonrpc":"2.0","id":1,"result":{"status":"healthy","mode":"stdio","version":"3.2.0"}}
```

### HTTP Mode

```bash
curl http://localhost:3000/health
```

```json
{"status":"healthy","transport":"http","version":"3.2.0","activeSessions":0}
```

The `/v1/performance` endpoint (authenticated) is where uptime and throughput are reported.

### Shared Memory Mode

```json
→ {"jsonrpc":"2.0","method":"health/check","id":1}
← {"jsonrpc":"2.0","id":1,"result":{"status":"healthy","mode":"shmem","version":"3.2.0"}}
```

Over NDA-native framing the same call reports `"mode":"shmem-nda"`.

---

## 11. Logging and Diagnostics

### Log Levels

Set the log level with `RUST_LOG` or dynamically via `logging/setLevel`:

**Dynamic (MCP method):**
```json
{"jsonrpc":"2.0","method":"logging/setLevel","params":{"level":"debug"},"id":1}
```

**Environment variable:**

| Level | Usage |
|-------|-------|
| `error` | Only errors (tool failures, process crashes) |
| `warn` | Errors + warnings (unknown methods, path rejections) |
| `info` | Errors + warnings + info (startup, tool dispatch, shutdown) — **default** |
| `debug` | All above + debug (per-request method names, tool dispatch details) |
| `trace` | Maximum verbosity |

### Log Output Format

Logs are written as plain-text lines to **stderr** by `tracing_subscriber::fmt()`; the level is taken from `RUST_LOG` (falling back to `info`). `[logging].level` in the config file is parsed and validated but there is no JSON formatter option — no `format` key exists under `[logging]`.

### Examples

```bash
# Default (info level)
velocity_mcp --mode stdio

# Debug logging for troubleshooting
RUST_LOG=debug velocity_mcp --mode stdio

# HTTP mode with a config file
velocity_mcp --mode http --config config.toml
```

### Audit Log

Every tool execution is recorded in a global audit log:
- Ring buffer: 10,000 entries maximum
- Each entry: sequence number, timestamp, tool name, duration, outcome
- Export to JSON or CSV via the audit export endpoint (HTTP mode)
- Poisoning-tolerant mutex (recovers from panics in other threads)

---

## 12. Performance Benchmarks

Run the built-in benchmark suite:

```bash
./target/release/velocity_mcp --benchmark
```

### Reference Results (Intel Core 5 210H, release build, 2026-09-17)

Raw: `bench_nda_2026-09-17.txt` (500 iterations × 3 rounds, median kept).

**NDA/shmem Transport (warm p50, 40 tools in list):**

| Method | Latency | Throughput | vs JSON/stdio (avg) |
|--------|:-------:|:----------:|:-------------------:|
| ping | 0.001 ms | 1,761,585 r/s | **82.0x** |
| tools/list (40 tools) | 0.003 ms | 284,311 r/s | **249.7x** |
| tools/call (64B) | 0.001 ms | 511,819 r/s | **36.2x** |

**Fair Comparison (Rust JSON/stdio vs Node.js/stdio, warm p50, 40 tools):**

| Method | Node.js | Rust | Result |
|--------|:-------:|:----:|:------:|
| ping | 0.098 ms | 0.044 ms | **Rust 2.2x** |
| tools/list | 0.175 ms | 0.807 ms | **Node 4.6x*** |
| tools/call | 0.083 ms | 0.061 ms | **Rust 1.4x** |

*tools/list: Node.js returns a static array; Rust's `get_tools()` dynamically assembles 40 tools from 5 registries with cache validation + dedup + pagination.

**4-Pipeline Comparison (warm avg):**

| Pipeline | Ping | tools/list | tools/call |
|----------|:----:|:----------:|:----------:|
| Node.js JSON/stdio | 0.149 ms | 0.197 ms | 0.088 ms |
| Rust JSON/stdio | 0.047 ms | 0.878 ms | 0.071 ms |
| Rust NDA/stdio | 0.038 ms | 0.204 ms | 0.043 ms |
| Rust NDA/shmem | 0.001 ms | 0.004 ms | 0.002 ms |

**Overall:** NDA/shmem is 36x–250x faster than JSON/stdio; on identical JSON/stdio, Rust wins ping and tools/call (1.4x–2.2x), Node wins tools/list avg on this single-pass run (Rust dynamic 40-tool assembly). See [docs/COMPARISON.md](COMPARISON.md) for full 8-pipeline + scaling.

---

## 13. Security Model

The V.E.L.O.C.I.T.Y. server implements **15+ defense layers** providing enterprise-grade protection.

### Defense Layers

| # | Layer | Description |
|---|-------|-------------|
| 1 | **Input Validation** | Bounds-checked NDA parser, spec-compliant XML (quick-xml), path traversal rejection, injection protection |
| 2 | **Capability-Based Sandbox** | Restricted profile: no network, filesystem isolated to work dir, approved interpreters only |
| 3 | **Linux Seccomp Filters** | Kernel-level syscall filtering — blocks network, process creation, mount, ptrace, kernel modules |
| 4 | **Windows Job Object Limits** | Memory cap (256 MB) and process limits for sandboxed processes |
| 5 | **Execution Timeout** | 30-second hard deadline with process kill |
| 6 | **Output Size Limits** | stdout: 1 MB, stderr: 256 KB — prevents OOM from runaway output |
| 7 | **Ed25519 Signatures** | Sign and verify NDA documents for authenticity and tamper detection |
| 8 | **Merkle Tree Integrity** | SHA-256 root verification of NDA binary content |
| 9 | **Rate Limiting** | Global token bucket: 20 req/sec, burst 100 (shared across clients); HTTP mode gated by `[http].enable_rate_limit` |
| 10 | **Audit Logging** | 10K entry ring buffer, JSON/CSV export, poisoning-tolerant mutex |
| 11 | **Error Sanitization** | Strips internal paths (Win/Unix), truncates at 500 chars |
| 12 | **CORS Protection** | Origin restrictions via `[http].cors_origins` |
| 13 | **API Key Authentication** | Timing-safe comparison of `Authorization: Bearer <key>` against `[http].api_key` / `VELOCITY_API_KEY` |
| 14 | **Dependency Audit** | 0 vulnerabilities across all crate dependencies (`cargo audit`) |
| 15 | **CI/CD Automation** | GitHub Actions: build + test + cargo audit on every push/PR |

### Path Validation

All file paths are validated before execution (cross-platform):

| Check | Rejects | Example |
|-------|---------|--------|
| Empty path | `""` | Missing parameter |
| Relative path | `"documents/file.nda"` | Not anchored |
| Path traversal | `"/home/user/../../etc/passwd"` | Directory traversal |
| Must be absolute | `"./relative/file"` | Relative with `./` |

### Capability-Based Sandbox

All NDA execution and shell commands go through a capability-based sandbox:

**ProcessCapabilities (restricted profile):**
- No network access
- File system isolated to sandbox temp directory
- 6 approved interpreters: python, node, powershell, bash, cmd.exe, dotnet
- Binary payload execution allowed
- 256 MB memory cap

**Platform-specific enforcement:**
- **Windows**: Job Object memory limits, process isolation
- **Linux**: seccomp-bpf syscall filters (whitelist approach), blocking network syscalls, process creation, mount, ptrace, kernel module operations

**Violation tracking:**
- Categories: FileSystem, Network, Interpreter, Memory, Timeout
- All violations logged to the global audit trail

### Ed25519 NDA Signatures

- **Signing**: `compile_signed()` signs the entire NDA binary with an Ed25519 key
- **Verification**: `verify_signature()` authenticates the document and detects tampering
- **Backward compatible**: Unsigned documents still parse correctly
- **Status reporting**: `read_nda` shows signature status: VERIFIED / UNSIGNED / FAILED

### Rate Limiting

A single global token-bucket rate limiter protects against abuse:
- 20 tokens per second refill rate, burst capacity 100 tokens (defaults in `src/rate_limit.rs`)
- Tuned through the `VELOCITY_RATE_LIMIT` / `VELOCITY_RATE_BURST` environment variables — there are no per-second/burst keys in the config file
- Shared across all clients in the native server; it is toggled in HTTP mode by the boolean `[http].enable_rate_limit` (default `true`)
- The `velocity-mcp-edge` crate keys its own limiter per client IP
- Exceeded requests receive a JSON-RPC error with HTTP status 429

### Input Size Limits

| Limit | Value | Behavior |
|-------|-------|----------|
| Max JSON-RPC request size (stdio) | 1 MB | Rejected before JSON parsing |
| Max HTTP request size | 10 MB (`[http].max_request_size`) | Rejected with 413 status |
| Max shared memory input | 4,086 bytes | Rejected with buffer overflow error |
| Max shared memory output | 61,440 bytes | Rejected with buffer overflow error |
| Max captured stdout | 1 MB | Truncated to prevent OOM |
| Max captured stderr | 256 KB | Truncated to prevent OOM |
| Max process memory | 256 MB | OS-level limit (Job Object / cgroup) |

### Testing and Verification

1,100+ tests verify all security layers:
- **Unit tests**: Parser bounds checking, sandbox capabilities, signature verification, NDA-native protocol, MCP spec compliance, rate limiter, audit log, error sanitization, transport layers, middleware, plugin system, marketplace
- **Integration tests**: Adversarial tests covering path traversal, network blocking, XML attacks, tamper detection, sandbox escape attempts, HTTP transport, authentication, batch endpoints
- **Property-based fuzz tests**: Random cases proving parser never panics, signatures always verify, NDA frames resist tampering and truncation
- **Proc macro tests**: Type-safe tool registration, constraint validation
- **Cross-platform tests**: Path validation, timeout enforcement

---

## 14. Troubleshooting

### "File path contains traversal sequence '..'"

**Cause:** The provided file path contains `..` sequences, which are rejected for security.

**Fix:** Use absolute paths without directory traversal.

### "File path must be absolute"

**Cause:** A relative path was provided instead of an absolute path.

**Fix:** Provide the full absolute path (e.g., `/home/user/file.nda` or `C:\Users\me\file.nda`).

### Server doesn't respond to MCP client

**Cause:** The client may not be sending the initialization sequence correctly.

**Fix:** Ensure the client sends:
1. `initialize` request
2. `notifications/initialized` notification
3. Then `tools/list` or `tools/call`

### "C# core engine not found at expected path"

**Cause:** The C# NdaMcpServer executable is not at the configured path.

**Fix:** Set the `VELOCITY_CSHARP_PATH` environment variable to the correct path.

### "C# process timed out after 30s"

**Cause:** The C# engine took longer than 30 seconds to process the request.

**Fix:** The server automatically kills the timed-out process. Check the engine's logs for large file processing or hangs.

### Shared memory: host sees stale data

**Cause:** Missing memory fence between length field writes and state transitions.

**Fix:** Follow the synchronization protocol:
- Writer: data → length → `SeqCst fence` → state (Release)
- Reader: state (Acquire) → `SeqCst fence` → length → data

### "Method 'X' not found"

**Cause:** The requested method is not supported.

**Supported methods:** `initialize`, `notifications/initialized`, `notifications/cancelled`, `ping`, `logging/setLevel`, `tools/list`, `tools/call`, `health/check`

### HTTP: Connection refused

**Cause:** The HTTP server is not running or listening on a different port.

**Fix:** Start with `--mode http --addr 0.0.0.0:3000` and verify the port is not in use.

### Graceful shutdown not working

**Cause:** The Ctrl+C handler may have failed to install.

**Fix:** Check startup logs for "Failed to set Ctrl+C handler". The server will still exit when stdin reaches EOF (stdio mode).

---

## 15. Frequently Asked Questions

### Q: What MCP protocol version does this server support?

A: Protocol version `2024-11-05`. The server reports this in the `initialize` response. Supports `ping`, `logging/setLevel`, `notifications/cancelled`, cursor pagination on `tools/list`, elicitation, and roots.

### Q: Can I use this server with multiple MCP clients simultaneously?

A: In stdio mode, each client needs its own server process. In HTTP mode, multiple clients can connect simultaneously with automatic per-client session management (no fixed cap configured in code). In shared memory mode, only one host can use the buffer at a time.

### Q: What happens if the C# engine crashes?

A: The server captures stderr from child processes and returns an error response. For built-in tools, errors are handled natively in Rust and returned as structured error responses.

### Q: How do I update the server?

A: Rebuild from source with `cargo build --release` and replace the executable. Restart any connected clients.

### Q: Can I add custom tools?

A: Yes, three ways:
1. **Proc macros** — Use `#[mcp_tool]` for compile-time registration with automatic schema generation
2. **Plugins** — Install a plugin via the marketplace for dynamic tool loading
3. **Registry** — Add a `Tool` entry in `src/registry.rs` for built-in tools

### Q: Is the server compatible with Linux or macOS?

A: Yes. The server is fully cross-platform. Linux gets additional security from seccomp syscall filters. Shared memory mode uses file-based memory mapping on all platforms. Path validation works with both Windows and Unix paths.

### Q: What's the maximum file size that can be converted?

A: For built-in NDA tools, file size is limited by available memory. For external engine tools, the server passes the file path without reading the file itself.

### Q: How do I enable verbose logging?

A: Set `RUST_LOG=debug` or `RUST_LOG=trace` before starting the server. In HTTP mode, use structured JSON logging for log aggregation.

### Q: How do I deploy in production?

A: See the [Deployment Guide](DEPLOYMENT.md) for Docker, Kubernetes, and bare metal deployment instructions. Pre-built Docker images are available.

### Q: Can I monitor the server with Prometheus?

A: Yes. In HTTP mode, Prometheus metrics are exposed at `/v1/metrics/prometheus` (the same counters as JSON at `/v1/metrics`). Pre-built Grafana dashboards and alerting rules are included under `deploy/grafana/` and `deploy/prometheus/`.
