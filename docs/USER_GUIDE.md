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
# Python — the SDK is async and the package is `velocity_mcp_client`
import asyncio
from velocity_mcp_client import HttpTransport, McpClient

async def main():
    client = McpClient(HttpTransport("http://localhost:3000/v1/mcp"))
    await client.initialize()
    tools = await client.list_tools()
    result = await client.call_tool("file_read", {"path": "/tmp/data.csv"})
    print(len(tools), result)

asyncio.run(main())
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
- `logging/setLevel` — set server log verbosity (`debug`, `info`, `notice`, `warning`, `error`, `critical`, `alert`, `emergency`)
- `notifications/cancelled` — cancel an in-flight request by ID

---

## 3. Using the Tools

### Built-in Tools

The server provides 16 built-in tools — four for general operations, four for NDA binary format operations, and eight additional tools. All tools run natively in Rust.

### General Tools

#### file_read

Read a file's contents as UTF-8 text with path validation and size limits; a file that is not valid UTF-8 fails.

**Parameters:**

| Parameter | Required | Description |
|-----------|----------|-------------|
| `path` | Yes | Absolute path to the file |

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

Write UTF-8 text content to a file with path validation.

**Parameters:**

| Parameter | Required | Description |
|-----------|----------|-------------|
| `path` | Yes | Absolute path for the output file |
| `content` | Yes | Content to write |

#### shell_exec

Execute a shell command with a clamped timeout, output-size caps and a dangerous-pattern blocklist. The command runs with the server's own permissions; no sandbox is involved.

**Parameters:**

| Parameter | Required | Description |
|-----------|----------|-------------|
| `command` | Yes | Shell command to execute |
| `timeout` | No | Timeout in seconds (default: 30; a value above 300 is clamped to 300) |
| `workingDir` | No | Directory to run the command in; must be absolute and must not contain `..` anywhere, checked before the command starts |

**Security:** No sandbox is involved: `src/registry.rs` hands the string to `cmd /C` on Windows and `sh -c` on other platforms, and the child inherits the server's permissions, environment and reachable filesystem. The guards applied before the spawn are, in code order: a blocklist of destructive Unix and Windows patterns matched as substrings against the command lowercased with whitespace collapsed, and again with backslashes stripped; a shorter variable-expansion and encoding-tricks list matched only against the first of those two normalizations; a 10,000-byte command length cap; a timeout that defaults to 30 seconds and is clamped to at most 300, after which the child is killed; stdout capped at 1 MiB and stderr at 262,144 bytes; and, when `workingDir` is supplied, the path validator the file tools use. Shell metacharacters (`;`, `|`, `&`, backtick, `$`, newline) are detected and logged as a warning but deliberately allowed, so command chaining works. Because every match is a substring test over those two fixed normalizations, a blocked pattern spelled any other way -- extra quoting, an indirection not on the list, or an equivalent command -- is not caught; the blocklist deters accidents, it does not confine the caller.

#### http_request

Make HTTP requests with automatic retry logic: up to 3 retries with exponential backoff (100ms, 200ms, 400ms) on transport errors and 5xx responses. Requires the `oauth2` feature - on a build without it the tool is still listed, but every call is refused.

**Parameters:**

| Parameter | Required | Description |
|-----------|----------|-------------|
| `url` | Yes | Target URL |
| `method` | No | HTTP method (default: `GET`) |
| `headers` | No | Request headers as key-value pairs; only string values are sent |
| `body` | No | Request body |
| `timeout` | No | Request timeout in seconds (default: 30; a value above 300 is clamped to 300) |

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
```text
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
- **WebSocket** at `/v1/ws` for bidirectional communication: one JSON-RPC object per **text** frame, and nothing else -- a binary frame or a JSON array is read and answered with no reply at all (the HTTP route answers an array with `-32600`), each text frame larger than 1 MiB is refused with `-32700` and `"id": null`, and a per-connection bucket of 50 msg/s (burst 100) drops the requests it refuses instead of deferring them
- **Session management** with automatic session creation and cleanup, scoped to the two routes that register a session: `/v1/sse` and streamable HTTP. `POST /v1/mcp` and `/v1/ws` mint a session id for the audit log but never register one, so `/v1/sessions` lists neither and `DELETE /v1/sessions/:id` answers 404 for them, while their per-session audit export still works
- **API key authentication** via `Authorization: Bearer <key>` header, with the key set in `[http].api_key` or `VELOCITY_API_KEY`
- **CORS** with origin restrictions configured through `[http].cors_origins`
- **JSON metrics** at `/v1/metrics` and **Prometheus exposition** at `/v1/metrics/prometheus`
- **Health check** at `/health` (the only route outside `/v1`, so it is unauthenticated)
- **Performance metrics** at `/v1/performance`
- **Request size limits** (default: 10 MB for POST bodies, set by `[http].max_request_size`; `/v1/ws` does not read that key -- it enforces its own 1 MiB per-frame cap, so the same 1.6 MB `tools/call` that `POST /v1/mcp` runs answers `-32700` over `/v1/ws`)
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
| 9–12 | 4 bytes | Request sequence counter (u32, little-endian) |
| 13–15 | 3 bytes | Reserved padding |
| 16–4095 | 4080 bytes | Input request buffer |
| 4096–65535 | 61440 bytes | Output response buffer |

Offsets are the `STATE_OFFSET`/`INPUT_LEN_OFFSET`/`OUTPUT_LEN_OFFSET`/`REQUEST_SEQ_OFFSET`/`INPUT_BUFFER_OFFSET`/`OUTPUT_BUFFER_OFFSET`/`TOTAL_BUFFER_SIZE` constants, identical in `src/ipc/shmem.rs` (server) and `client/src/transport/shmem.rs` (client).

**Synchronization:**
- State byte: Atomic Acquire/Release ordering
- Length fields: `SeqCst` fence between length and state
- Windows: `CreateEventW`/`WaitForSingleObject`/`SetEvent` for zero-poll blocking
- Other platforms: 100μs sleep fallback

---

## 6. Client SDKs

Client SDKs are provided in 4 languages: the Rust client crate in `client/` (package `velocity-mcp-client`), and the Python, TypeScript and Go SDKs in `sdk/python`, `sdk/typescript` and `sdk/go`.

Every SDK has the same shape: build a transport, hand it to the client, `initialize()`, then call tools. All four talk to `POST /v1/mcp`.

### Rust

```rust
use velocity_mcp_client::{HttpTransport, McpClient};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let transport = HttpTransport::new("http://localhost:3000/v1/mcp", None)?;
    let mut client = McpClient::new(transport);
    client.initialize().await?;
    let tools = client.list_tools().await?;
    let result = client
        .call_tool("file_read", serde_json::json!({"path": "/tmp/data.csv"}))
        .await?;
    println!("{} tools: {:?}", tools.len(), result);
    Ok(())
}
```

### Python

```python
import asyncio
from velocity_mcp_client import HttpTransport, McpClient

async def main():
    client = McpClient(HttpTransport("http://localhost:3000/v1/mcp"))
    await client.initialize()
    tools = await client.list_tools()
    result = await client.call_tool("file_read", {"path": "/tmp/data.csv"})
    print(len(tools), result)

asyncio.run(main())
```

### TypeScript

```typescript
import { HttpTransport, McpClient } from '@velocity-mcp/client';

const client = new McpClient(new HttpTransport('http://localhost:3000/v1/mcp'));
await client.initialize();
const tools = await client.listTools();
const result = await client.callTool('file_read', { path: '/tmp/data.csv' });
console.log(tools.length, result);
```

### Go

```go
package main

import (
	"context"
	"fmt"
	"time"

	velocity_mcp "github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/sdk/go"
)

func main() {
	transport := velocity_mcp.NewHttpTransport("http://localhost:3000/v1/mcp", "", 30*time.Second)
	client := velocity_mcp.NewMcpClient(transport)
	defer client.Close()

	ctx := context.Background()
	if _, err := client.Initialize(ctx); err != nil {
		panic(err)
	}
	tools, err := client.ListTools(ctx)
	if err != nil {
		panic(err)
	}
	result, err := client.CallTool(ctx, "file_read", map[string]interface{}{"path": "/tmp/data.csv"})
	if err != nil {
		panic(err)
	}
	fmt.Println(len(tools), result)
}
```

Full per-language API reference: `client/README.md` (Rust) and `sdk/{python,typescript,go}/README.md`.

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

**Three execution paths reach the machine, and only one of them is the capability sandbox.** Script and native plugins are spawned straight from the manifest (`src/plugins/mod.rs`): the server substitutes template variables into the declared arguments, rejects null bytes, arguments over 10 KB and -- for shell-interpreter executors only -- the thirteen shell metacharacters, caps output at 1 MiB stdout and 256 KiB stderr, and enforces a timeout; it builds no `Sandbox`, so it sets no memory cap and no network limit, and passes the manifest's own `env` map to the child verbatim. WASM plugins are isolated by their engine instead: they run inside a Wasmer instance under an instruction-metering budget, which is not this sandbox. Only NDA frame execution constructs a `Sandbox` (`src/nda_executor.rs`), always with the `restricted()` profile, and the bullets below describe that path alone.
- **Enforced at spawn:** the interpreter allowlist -- `python`, `python3`, `node`, `powershell`, `bash`, `cmd.exe`, `dotnet` -- is consulted before every `Sandbox::execute`, an unlisted program is refused and recorded, the child starts in a fresh directory under `<tmp>/velocity_sandbox/`, `VELOCITY_CSHARP_ENGINE` is stripped from its environment, and the 30-second wall clock and the output caps apply.
- **Not enforced:** the sandbox's own path and network checks (`check_file_access`, `check_network_access`) have no production call site, so no spawn path consults them -- a sandboxed child can open any absolute path and reach any host it can resolve. Filesystem isolation here is a working directory, not a jail.
- **Memory and network limits reach the child as environment variables** (`VELOCITY_SANDBOX_MEM_LIMIT` for the 256 MB default, `VELOCITY_SANDBOX_NO_NETWORK` when network is disallowed), which only cooperative code honours. On Windows the memory cap is additionally applied out-of-band by a Job Object; on Linux and macOS there is no OS-level cap.
- Linux: seccomp filter definitions ship in `src/sandbox/linux_seccomp.rs` but no spawn path installs them yet, so beyond the interpreter allowlist nothing above is OS-enforced there.
- **Violations are audited:** a refusal writes a `sandbox_violation` entry to the global audit log as a rejected tool call. Because the interpreter allowlist is the only check a spawn path runs today, `Interpreter` is the only category production can record; `FileSystem` and `Network` are exercised by unit tests only.
- **Closing the gap for real** means wiring those two checks into the spawn path or installing the seccomp filters, and either one changes what a plugin or an NDA frame can do, so it is an owner decision rather than a doc fix.

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

Alerting rules ship in `deploy/prometheus/alerts.yml`, but **that file is not loaded**: `deploy/prometheus/prometheus.yml` declares no `rule_files:` entry and the compose stack mounts only `prometheus.yml`, so no rule evaluates until an operator wires both — which starts firing real notifications and is their decision. Of its eleven rules, four key on the exported counters below, five key on series this server never emits, and two depend on a cAdvisor container exporter the shipped stack does not run:
- `HighErrorRate` / `CriticalErrorRate` — failed-request ratio over 5m / 2m windows
- `HighLatency` — latency increase over 5 minutes — **cannot fire**: no latency histogram is exported, only the average gauge listed above
- `LowThroughput` — request rate dropping below 1 r/s
- `HighRateLimitHits` — more than 10 rate-limit rejections per second
- `HighMemoryUsage` / `HighCPUUsage` — container resource pressure — these series come from a cAdvisor exporter, which the shipped compose stack does not run
- `TLSCertificateExpiringSoon` / `TLSCertificateExpiringImminently` — **cannot fire**: no certificate-expiry series is exported, and TLS is enabled by CLI flags rather than measured
- `PluginInstallationFailures` — failed plugin installs — **cannot fire**: the install handler answers with an HTTP 400 and exports no counter
- `AuditLogBufferFull` — audit ring buffer above 9,000 entries — **cannot fire**: the audit registry answers export and flush routes and exports no occupancy gauge

### Grafana Dashboard

A pre-built Grafana dashboard is available in `deploy/grafana/dashboards/velocity-mcp.json` (datasource in `deploy/grafana/datasources/prometheus.yml`, file provider in `deploy/grafana/provisioning/dashboards/velocity-mcp.yml`) with panels for:
- Total Requests
- Success Rate
- Request Rate
- Error Rate
- Active SSE Connections — `velocity_mcp_sse_connections_active`
- Rate Limit Hits
- Auth Failures And Latency — `velocity_mcp_auth_failures_total` and `velocity_mcp_latency_microseconds` as an instant table

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
csharp_path = "NdaMcpServer.exe"  # inert: parsed, never read — resolve_csharp_path() reads VELOCITY_CSHARP_PATH directly
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
database = false                  # inert: parsed, never read — database adapters are the `database` cargo feature
oauth2 = false                    # inert: parsed, never read — OAuth2 is the `oauth2` cargo feature
http = false                      # inert: parsed, never read — HTTP transport is the `http` cargo feature plus `--mode http`
nda_merkle = true                 # NDA frame Merkle integrity verification; the only [features] key the server reads (disable for max throughput on trusted transports)

[wasm_runtimes]
# Per-call instruction limit for WASM plugin runtimes (metering middleware).
# Default: 10,000,000 instructions (about 1 millisecond of measured guest
# compute — see the note below). Set to 0 to disable metering
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

**What the instruction budget actually buys.** Metering counts WASM operations, and one language-level operation costs many of them, so `instruction_limit` is a compute budget, not a time budget. Measured on this machine (Intel Core 5 210H, the shipped guests, budget reset before every call as the server does): a tree-walk interpreter call consuming roughly 8 million instructions completed in 0.8 ms, so the 10,000,000 default is on the order of a millisecond of guest compute per call. The same sources need progressively larger budgets as the workload grows — a 1,000-iteration `while` loop lands between 4M and 8M and stays inside the default, while 21,891 iterations need between 64M and 128M and a recursive `fib(20)` needs between 128M and 256M, both of which trap at the default. QuickJS tells the same story at a different scale: `fib(10)` passes at 10M while `fib(20)` still traps at 80M and only completes at 160M. A call that trips the limit returns the resource-limit message built by `trap_message` (`src/plugins/mod.rs:443`), which points at `instruction_limit`; raise it in the config file, or per process with `VELOCITY_WASM_INSTRUCTION_LIMIT`, and set `0` to remove the cap entirely.

TLS/HTTPS is not a config-file option: pass `--tls-cert <path>` and `--tls-key <path>` on the command line (both are required together) to serve HTTPS in HTTP mode.

**A key the server does not recognise is ignored, not rejected.** No config struct carries `deny_unknown_fields` (`ServerConfig` at `src/config.rs:18`, and `HttpConfig`, `LoggingConfig`, `FeaturesConfig`, `WasmRuntimesConfig` likewise), and every declared field either has a `#[serde(default)]` or is an `Option`, so a file with a misspelled key still parses and the field quietly keeps its default. `max_request_bytes = 1024` leaves the 10 MB limit in force; `apikey = "secret"` leaves the server with **no authentication**. `validate()` (`src/config.rs:505`) only checks the values it has fields for — mode, log level, and in HTTP mode a non-empty address and a non-zero request size — so nothing reports the typo at startup. Note the reverse case too: four keys the server *does* recognise are parsed and then never read — `[features].database`, `[features].oauth2` and `[features].http` (the live switches are the cargo build features of the same names, `Cargo.toml` `[features]`, plus `--mode http` for transport), and root `csharp_path`, because `registry::resolve_csharp_path()` reads `VELOCITY_CSHARP_PATH` from the environment directly and ignores the configuration file. When a setting appears to have no effect, check the key against the structs above before debugging the behaviour.

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
| `observability` | OpenTelemetry distributed tracing (spans only; no metrics pipeline) |
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
| `VELOCITY_RATE_LIMIT` | Token-bucket refill rate (tokens/sec) per bucket, for both the process-wide and per-peer limiters | `20` |
| `VELOCITY_RATE_BURST` | Token-burst capacity per bucket, for both limiters | `100` |
| `VELOCITY_NDA_MERKLE` | NDA frame Merkle integrity verification (`0`/`false`/`no` to disable) | `1` |
| `VELOCITY_WASM_INSTRUCTION_LIMIT` | Per-call WASM metering instruction limit (`0` disables metering) | `10000000` |
| `VELOCITY_AUDIT_LOG_PATH` | Directory the audit log is flushed to on shutdown | `audit_logs` |
| `VELOCITY_PHASE_TIMING` | NDA loop phase-timing instrumentation; the value is the output file, records are written on shutdown | — (off) |
| `VELOCITY_PING_DELAY_US` | Sleep inserted before each `ping` response, to separate server latency from transport latency | — (no delay) |
| `VELOCITY_SPIN_US` | Microseconds the shmem event wait spins before parking (Windows builds only) | `200` |
| `VELOCITY_BENCH_EXTRA_TOOLS` | Register N synthetic tools at startup to scale the `tools/list` payload for benchmarks | — (no-op) |
| `RUST_LOG` | Log level for the tracing subscriber (`error`, `warn`, `info`, `debug`, `trace`) | `info` |

---

## 10. Health Checks

### Stdio Mode

```text
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

`health/check` is not a transport probe: its `mode` is a literal in the shared JSON-RPC handler, so it
answers `"mode":"stdio"` even against this HTTP server (measured on a live build). The payload above,
`GET /health`, is the one that reports the running transport.

The `/v1/performance` endpoint (authenticated) is where uptime and throughput are reported.

### Shared Memory Mode

```text
→ {"jsonrpc":"2.0","method":"health/check","id":1}
← {"jsonrpc":"2.0","id":1,"result":{"status":"healthy","mode":"shmem","version":"3.2.0"}}
```

Over NDA-native framing the same call reports `"mode":"shmem-nda"`.

---

## 11. Logging and Diagnostics

### Log Levels

Two surfaces set verbosity and they accept different names: the live filter is built from `RUST_LOG` / `VELOCITY_LOG_LEVEL` using the table below, while `logging/setLevel` validates the eight MCP level names — `warn` and `trace` are env-var names and are refused by the method with `-32602`:

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

Logs are written as plain-text lines to **stderr** by `tracing_subscriber::fmt()`; there is no JSON formatter option — no `format` key exists under `[logging]`.

The live filter is built once, after the config has been validated (`src/main.rs`), and the first of these that is set wins:

1. `RUST_LOG` — a full directive string (`debug`, or `velocity_mcp=debug,tower_http=warn`);
2. `VELOCITY_LOG_LEVEL` — a single level name;
3. `[logging].level` in the config file — the same five names, defaulting to `info`.

`VELOCITY_LOG_LEVEL` and `[logging].level` accept exactly the five names `validate()` whitelists (`src/config.rs:517`), so a typo aborts startup with `Config error: Invalid log level: …` rather than silently leaving the server at `info`.

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

Raw: `bench_nda_2026-09-17.txt` (500 iterations × 3 rounds, median kept). These tables come from the **`bench_nda` transport harness** (`cargo run --release --manifest-path bench_nda/Cargo.toml`, run from the repository root), not from the `velocity_mcp --benchmark` command above — that suite prints none of these rows. The capture itself is a local artifact (raw benchmark output is not committed), so this table is the durable record; see [COMPARISON.md](COMPARISON.md) for the same run in full.

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
| 9 | **Rate Limiting** | Token bucket, 20 req/sec burst 100 — one bucket per client IP in HTTP mode, process-wide on stdio/shmem; HTTP mode gated by `[http].enable_rate_limit` |
| 10 | **Audit Logging** | 10K entry ring buffer, JSON/CSV export, poisoning-tolerant mutex |
| 11 | **Error Sanitization** | Strips internal paths (Win/Unix), truncates at 500 chars |
| 12 | **CORS Protection** | Origin restrictions via `[http].cors_origins` |
| 13 | **API Key Authentication** | Timing-safe comparison of `Authorization: Bearer <key>` against `[http].api_key` / `VELOCITY_API_KEY` |
| 14 | **Dependency Audit** | 0 vulnerabilities across all crate dependencies (`cargo audit` exits 0; re-measured 2026-10-13 the same run also prints 3 allowed warnings, which the gate does not fail on -- two unmaintained crates and one unsound transitive `memmap2` 0.6.2 that this lock cannot reach, with the reachability measurement recorded in the CHANGELOG security section) |
| 15 | **CI/CD Automation** | GitHub Actions: lint + build + test + cargo audit on every push or pull request **targeting `main`** -- the workflow's only two triggers both read `branches: [main]`, so a push to a feature branch starts no run |

### Path Validation

All file paths are validated before execution (cross-platform):

| Check | Rejects | Example |
|-------|---------|--------|
| Empty path | `""` | Missing parameter |
| Relative path | `"documents/file.nda"` | Not anchored |
| Path traversal | `"/home/user/../../etc/passwd"` | Directory traversal |
| Must be absolute | `"./relative/file"` | Relative with `./` |
| Reparse-point ancestor | `"C:\Users\me\linked\file.nda"` (`linked` points outside) | Symlink on POSIX, junction or other reparse point on Windows |

**How the four checks behave on Windows.** The `..` test is a substring match anywhere in the path, so a legal filename such as `report..final.txt` is refused as well. Both `\\host\share\file` and `\\?\C:\dir\file` satisfy the absolute-path check and are handed to the ordinary filesystem call, which lets the OS resolve the name; only a drive-relative path such as `\dir\file` is refused as not absolute. An ancestor whose metadata read fails is skipped, so a path that does not exist yet -- a write target -- is admitted. The check is per-component, not per-root: there is no configured root, so an absolute path anywhere on the host passes unless one of its components is a reparse point.

### Capability-Based Sandbox

Only `src/nda_executor.rs` constructs a `Sandbox`, on its two `execute_nda` paths, so the profile below applies to NDA execution and not to the built-in `shell_exec` tool:

**ProcessCapabilities (restricted profile):**
- No network access
- File system isolated to sandbox temp directory
- 7 approved interpreters: python, python3, node, powershell, bash, cmd.exe, dotnet (`.py` documents resolve `python` first, then `python3` when only the versioned name is on PATH)
- Binary payload execution allowed
- 256 MB memory cap

**Platform-specific enforcement:**
- **Windows**: Job Object memory limits, process isolation
- **Linux**: seccomp-bpf filter definitions exist (whitelist blocking network syscalls, process creation, mount, ptrace, kernel module operations), but nothing installs them at spawn time yet, so kernel-level enforcement is not active in this release

**Violation tracking:**
- Categories: FileSystem, Network, Interpreter, Memory, Timeout
- All violations logged to the global audit trail

### Ed25519 NDA Signatures

- **Signing**: `compile_signed()` signs the entire NDA binary with an Ed25519 key
- **Verification**: `verify_signature()` authenticates the document and detects tampering
- **Backward compatible**: Unsigned documents still parse correctly
- **Status reporting**: `read_nda` shows signature status: VERIFIED / UNSIGNED / FAILED

### Rate Limiting

Two limiters live in `src/rate_limit.rs`, and which one a request hits depends on the transport:

- Process-wide (`check_rate_limit`): stdio and shared memory, which each serve exactly one peer for the life of the process
- Per-peer (`check_rate_limit_for_peer`, HTTP mode): every client IP gets its own bucket, so one client exhausting its allowance cannot throttle the others; buckets are kept LRU-bounded at 4 096 peers (`MAX_PEER_BUCKETS`), and a request with no connection info falls into one shared `unknown` bucket
- 20 tokens per second refill rate, burst capacity 100 tokens (defaults in `src/rate_limit.rs`), per bucket — both kinds read the same configured limits
- Tuned through the `VELOCITY_RATE_LIMIT` / `VELOCITY_RATE_BURST` environment variables — there are no per-second/burst keys in the config file
- HTTP mode is toggled by the boolean `[http].enable_rate_limit` (default `true`)
- The `velocity-mcp-edge` crate keys its own limiter per client IP
- Exceeded requests receive a JSON-RPC error with HTTP status 429

### Input Size Limits

| Limit | Value | Behavior |
|-------|-------|----------|
| Max JSON-RPC request size (stdio, and `/v1/ws` text frames) | 1 MB | stdio: rejected before JSON parsing. `/v1/ws`: refused before the per-connection rate-limit tokens are accounted, so oversize frames are never throttled, and the reply is `-32700` with `"id": null` |
| Max HTTP request size | 10 MB (`[http].max_request_size`) | Rejected with 413 and a plain-text body, not a JSON-RPC error object |
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

**Supported methods:** the JSON-RPC dispatcher (`src/protocol/json_rpc.rs`) recognises twenty names -- `initialize`, `notifications/initialized`, `notifications/cancelled`, `notifications/progress`, `ping`, `logging/setLevel`, `tools/list`, `tools/call`, `health/check`, `resources/list`, `resources/read`, `resources/templates/list`, `resources/subscribe`, `resources/unsubscribe`, `prompts/list`, `prompts/get`, `sampling/createMessage`, `elicitation/create`, `roots/list`, plus `connector/call` only when built with `--features oauth2` -- while the shared-memory JSON dispatcher (`src/protocol/nmcp_binary.rs`) recognises seventeen of them: it has no `connector/call`, `elicitation/create` or `roots/list` arm, and phrases its refusal as `Method 'X' not supported` rather than `not found`.

### HTTP: Connection refused

**Cause:** The HTTP server is not running or listening on a different port.

**Fix:** Start with `--mode http --addr 0.0.0.0:3000` and verify the port is not in use.

### Graceful shutdown not working

**Cause:** The Ctrl+C handler may have failed to install.

**Fix:** Check startup logs for "Failed to set Ctrl+C handler". The server will still exit when stdin reaches EOF (stdio mode).

---

## 15. Frequently Asked Questions

### Q: What MCP protocol version does this server support?

A: Protocol version `2024-11-05`. The server reports this in the `initialize` response. Supports `ping`, `logging/setLevel`, `notifications/cancelled`, cursor pagination on `tools/list`, and `notifications/tools/list_changed` on the HTTP/SSE channel whenever a tool is registered at runtime. It also answers `elicitation/create` and `roots/list` if a client sends them, but both are server→client requests in the spec: the server never initiates them, and `roots` is not advertised in its capabilities because that map describes what the *server* can do.

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

A: Yes. The server is fully cross-platform. Linux ships seccomp filter definitions that no spawn path installs yet, so enforcement there is the same capability sandbox used everywhere else. Shared memory mode uses file-based memory mapping on all platforms. Path validation works with both Windows and Unix paths.

### Q: What's the maximum file size that can be converted?

A: For built-in NDA tools, file size is limited by available memory. For external engine tools, the server passes the file path without reading the file itself.

### Q: How do I enable verbose logging?

A: Set `RUST_LOG=debug` or `RUST_LOG=trace` before starting the server. In HTTP mode, use structured JSON logging for log aggregation.

### Q: How do I deploy in production?

A: See the [Deployment Guide](DEPLOYMENT.md) for Docker, Kubernetes, and bare metal deployment instructions. Pre-built Docker images are available.

### Q: Can I monitor the server with Prometheus?

A: Yes. In HTTP mode, Prometheus metrics are exposed at `/v1/metrics/prometheus` (the same counters as JSON at `/v1/metrics`). A Grafana dashboard ships at `deploy/grafana/dashboards/velocity-mcp.json` and the compose `monitoring` profile provisions it through `deploy/grafana/provisioning/dashboards/velocity-mcp.yml`; its seven panels cover all seven exported series and query no series the server does not export. Alerting rules ship at `deploy/prometheus/alerts.yml` but are **not loaded** (no `rule_files:` entry), and five of their eleven rules key on metrics that do not exist — see the Prometheus Alerting Rules section above.
