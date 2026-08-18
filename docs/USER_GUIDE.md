# V.E.L.O.C.I.T.Y.-MCP User Guide

A complete guide to installing, configuring, and using the V.E.L.O.C.I.T.Y. NMCP Server.

---

## Table of Contents

1. [Getting Started](#1-getting-started)
2. [Configuring MCP Clients](#2-configuring-mcp-clients)
3. [Using the Tools](#3-using-the-tools)
4. [NDA Format and Tool Structuring Guide](#4-nda-format-and-tool-structuring-guide)
5. [Shared Memory Mode](#5-shared-memory-mode)
6. [Health Checks](#6-health-checks)
7. [Logging and Diagnostics](#7-logging-and-diagnostics)
8. [Performance Benchmarks](#8-performance-benchmarks)
9. [Security Model](#9-security-model)
10. [Troubleshooting](#10-troubleshooting)
11. [Frequently Asked Questions](#11-frequently-asked-questions)

---

## 1. Getting Started

### Prerequisites

- **Rust toolchain** (rustc 1.70+, cargo) — [Install via rustup](https://rustup.rs/)
- **Windows** — The server uses Windows-specific file paths and memory-mapped files.

### Building from Source

```bash
# Clone the repository
git clone <repository-url>
cd V.E.L.O.C.I.T.Y.-MCP

# Build in release mode (optimized)
cargo build --release

# The executable is at:
# ./target/release/velocity_mcp.exe
```

### Verifying the Build

```bash
# Run the test suite (146 tests)
cargo test

# Run the benchmark suite
./target/release/velocity_mcp.exe --benchmark

# Print help
./target/release/velocity_mcp.exe --help
```

### First Run (Stdio Mode)

```bash
./target/release/velocity_mcp.exe --mode stdio
```

The server starts and waits for JSON-RPC requests on stdin. Press `Ctrl+C` to shut down gracefully.

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

If your C# engine is not at the default path, set the environment variable:

```json
{
  "mcpServers": {
    "velocity-mcp": {
      "command": "C:\\path\\to\\target\\release\\velocity_mcp.exe",
      "args": ["--mode", "stdio"],
      "env": {
        "VELOCITY_CSHARP_PATH": "C:\\path\\to\\NdaMcpServer.exe"
      }
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
      "command": "C:\\path\\to\\target\\release\\velocity_mcp.exe",
      "args": ["--mode", "stdio"],
      "env": {
        "VELOCITY_CSHARP_PATH": "C:\\path\\to\\NdaMcpServer.exe"
      }
    }
  }
}
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
3. Send `tools/list` — discover available tools
4. Send `tools/call` — invoke a tool

---

## 3. Using the Tools

### Dynamic Tool Hosting

The V.E.L.O.C.I.T.Y. server provides four **built-in NDA tools** implemented natively in Rust, and can also automatically discover additional tools from a C# backend engine. When a client sends a `tools/list` request:

1. The server returns the 4 built-in NDA tools (always available)
2. It queries the C# engine for additional tools (if present)
3. Results are cached for subsequent requests
4. Built-in and discovered tools are merged (deduplicated by name)

**What this means for users:**
- The 4 core NDA tools run natively in Rust — no external process needed
- Additional tools from the C# engine are immediately available through the Rust server
- The Rust server provides high-performance protocol handling and native NDA operations

### Built-in NDA Tools

The server provides four built-in tools for NDA (Neural Document Archive) operations. These tools are always available and provide the core functionality for converting files to the faster binary format and converting JSON tool calls to native NDA binary format.

### convert_to_nda_document

Convert any file into a cryptographically signed `.nda` binary document with semantic triples and visual display commands.

**Parameters:**

| Parameter | Required | Description |
|-----------|----------|-------------|
| `filePath` | Yes | Absolute path to the input file |
| `outputPath` | No | Absolute path for the output `.nda` file. Defaults to input path with `.nda` extension. |

**Example request:**
```json
{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": {
    "name": "convert_to_nda_document",
    "arguments": {
      "filePath": "C:\\Users\\me\\documents\\source_code.cs"
    }
  },
  "id": 1
}
```

**Supported input types:** C# source code, PDF, CSV, Excel, Image, Zip archives, and other file formats.

### convert_to_nda_tool

Convert a JSON-RPC tool call into native NDA binary format for 97x faster parsing. This tool takes a JSON tool call and returns the equivalent NDA binary representation.

**Parameters:**

| Parameter | Required | Description |
|-----------|----------|-------------|
| `jsonRequest` | Yes | JSON-RPC tool call request to convert |
| `outputPath` | No | Path to write the NDA binary file. If omitted, returns base64-encoded binary data. |

**Example request:**
```json
{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": {
    "name": "convert_to_nda_tool",
    "arguments": {
      "jsonRequest": "{\"jsonrpc\":\"2.0\",\"method\":\"tools/call\",\"params\":{\"name\":\"hello_world\",\"arguments\":{\"message\":\"Hello\"}},\"id\":1}",
      "outputPath": "C:\\temp\\tool_call.nda"
    }
  },
  "id": 2
}
```

**NDA binary format:**
- `[4 bytes: magic "NMCP"]`
- `[32 bytes: merkle root (SHA-256 of payload)]`
- `[1 byte: method type (1=tools/call)]`
- `[2 bytes: tool name length]`
- `[N bytes: tool name]`
- `[2 bytes: arguments length]`
- `[M bytes: arguments as binary key-value pairs]`

The native NDA binary format enables **97.6x faster parsing** compared to JSON, as it uses zero-copy pointer casts instead of string parsing.

### read_nda

Read and inspect a compiled `.nda` binary file.

**Parameters:**

| Parameter | Required | Description |
|-----------|----------|-------------|
| `ndaPath` | Yes | Absolute path to the `.nda` file |

**Example request:**
```json
{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": {
    "name": "read_nda",
    "arguments": {
      "ndaPath": "C:\\Users\\me\\documents\\source_code.nda"
    }
  },
  "id": 2
}
```

**Returns:** Semantic triples, visual display commands, and string pool contents.

### execute_nda

Execute a runnable `.nda` container.

**Parameters:**

| Parameter | Required | Description |
|-----------|----------|-------------|
| `ndaPath` | Yes | Absolute path to the runnable `.nda` file |
| `arguments` | No | Array of command-line arguments to pass to the executable or script |

**Example request:**
```json
{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": {
    "name": "execute_nda",
    "arguments": {
      "ndaPath": "C:\\Users\\me\\documents\\app.nda",
      "arguments": ["--verbose", "--output", "result.txt"]
    }
  },
  "id": 3
}
```

**Execution modes:**
- Compiled C# binary → runs in-memory
- Script (Python, Node.js, PowerShell, Bash) → executes via the corresponding shell process

---

## 4. NDA Format and Tool Structuring Guide

The NDA (Neural Document Archive) format is the core data format that the V.E.L.O.C.I.T.Y. server operates on. This section explains what NDA files are, how the conversion pipeline works, and how to structure your tools and content for full NDA support.

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

The three tools form a complete lifecycle:

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
                                       │  (C# binary  │
                                       │   or script) │
                                       └──────────────┘
```

### Supported Source File Types

| File Type | Extension | What Gets Encapsulated |
|-----------|-----------|----------------------|
| **C# Source Code** | `.cs` | Compiled assembly + semantic analysis triples + type hierarchy |
| **PDF Documents** | `.pdf` | Extracted text triples + page layout commands + embedded fonts |
| **CSV Data** | `.csv` | Column schema triples + row data triples + statistical metadata |
| **Excel Workbooks** | `.xlsx` | Sheet structure triples + cell data + formula dependency graph |
| **Images** | `.png`, `.jpg` | Visual display commands + metadata triples + pixel data |
| **Zip Archives** | `.zip` | File manifest triples + embedded file payloads |
| **Other formats** | Any | Raw binary payload + file-type metadata triples |

### Structuring C# Tools for NDA Support

If you are building C# tools that will be invoked through the V.E.L.O.C.I.T.Y. server, follow these guidelines to ensure full NDA compatibility:

#### 1. Standard Input/Output Contract

Your C# tool must communicate via **JSON-RPC over stdin/stdout**. The server sends a JSON-RPC request to your tool's stdin and reads the response from stdout.

**Request format (from server to your tool):**
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

**Expected response format (from your tool to server):**
```json
{
  "jsonrpc": "2.0",
  "id": 999,
  "result": {
    "content": [
      {
        "type": "text",
        "text": "Your tool's output here"
      }
    ],
    "isError": false
  }
}
```

**Error response:**
```json
{
  "jsonrpc": "2.0",
  "id": 999,
  "result": {
    "content": [
      {
        "type": "text",
        "text": "Description of what went wrong"
      }
    ],
    "isError": true
  }
}
```

#### 2. Tool Registration

Register your tool in the Rust server's `registry.rs` by adding a `Tool` entry:

```rust
Tool {
    name: "your_tool_name".to_string(),
    description: "Clear description of what the tool does.".to_string(),
    input_schema: json!({
        "type": "object",
        "properties": {
            "param1": { "type": "string", "description": "What this param does" },
            "param2": { "type": "integer", "description": "What this param does" }
        },
        "required": ["param1"]
    }),
},
```

Then add a dispatch arm in `call_tool_with_csharp_path()`:

```rust
"your_tool_name" => {
    let param1 = arguments["param1"].as_str().ok_or("param1 is required")?;
    validate_file_path(param1)?;
    execute_csharp_mcp_tool("your_tool_name", arguments, csharp_path)
}
```

#### 3. File Path Requirements

All file paths passed to your tool **must be absolute**. The server validates this before your tool ever receives the request:

| Valid | Invalid | Reason |
|-------|---------|--------|
| `C:\Users\me\file.nda` | `file.nda` | Relative path |
| `D:\Projects\output.nda` | `.\output.nda` | Relative with `.` |
| `\\server\share\file.nda` | `C:\Users\..\Windows\f.txt` | Path traversal (`..`) |

#### 4. Producing NDA-Compatible Output

When your tool generates output that should be convertible to NDA format:

- **Structured data**: Return data as JSON or well-structured text. The NDA converter extracts semantic triples from structured formats more accurately than from free-form text.
- **Binary output**: If your tool produces a binary file, write it to the path specified in the arguments. Return the output file path in the response text.
- **Error reporting**: Always set `"isError": true` and provide a descriptive error message. Never write errors to stderr — the server only reads stdout for the JSON-RPC response.

#### 5. Creating Runnable NDAs

To create an NDA that can be executed with `execute_nda`:

1. **Compile your C# code** to an executable or DLL
2. **Convert it** using `convert_to_nda_document` with the compiled binary as input
3. **Execute it** using `execute_nda` — the binary runs in-memory without disk extraction

```bash
# Via MCP client JSON-RPC:

# Step 1: Convert compiled binary to runnable NDA
{"jsonrpc":"2.0","method":"tools/call","params":{"name":"convert_to_nda_document","arguments":{"filePath":"C:\\tools\\my_app.exe"}},"id":1}

# Step 2: Execute the NDA
{"jsonrpc":"2.0","method":"tools/call","params":{"name":"execute_nda","arguments":{"ndaPath":"C:\\tools\\my_app.nda","arguments":["--flag","value"]}},"id":2}
```

#### 6. Script-Based NDAs

You can also package scripts into NDAs for execution:

| Script Type | Runtime Required | Example Use |
|-------------|-----------------|-------------|
| PowerShell | Windows PowerShell | System administration, file processing |
| Python | Python runtime | Data analysis, ML pipelines |
| Node.js | Node.js runtime | Web scraping, API integration |
| Bash | Git Bash / WSL | Shell automation |

### NDA File Inspection with read_nda

The `read_nda` tool lets you inspect the internal structure of any `.nda` file:

**What you get back:**

```
Semantic Triples:
  Subject          Predicate           Object
  ─────────────────────────────────────────────────
  :document        :hasTitle           "My Document"
  :document        :hasAuthor          "UnitBuilds"
  :class:MyClass   :hasMethod          :method:Process
  :method:Process  :returnType         "System.Void"

Visual Display Commands:
  [PAGE 1] Layout: A4, Portrait
  [BLOCK 1] TextBlock at (72, 72) size (468, 600)
  [FONT] Segoe UI, 11pt

String Pool:
  0: "My Document"
  1: "UnitBuilds"
  2: "Process"
  3: "System.Void"
  ... (N unique strings, deduplicated)
```

This is useful for:
- **Debugging** — Verify that conversion produced correct triples
- **Auditing** — Check what semantic content was extracted
- **Integration** — Understand the internal structure before building tools that consume NDA data

### End-to-End Workflow Example

Here's a complete workflow converting a C# source file, inspecting it, and running it:

```json
// 1. Convert C# source to NDA
{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": {
    "name": "convert_to_nda_document",
    "arguments": {
      "filePath": "C:\\Projects\\MyApp\\Program.cs",
      "outputPath": "C:\\Projects\\MyApp\\Program.nda"
    }
  },
  "id": 1
}

// 2. Inspect the NDA to verify conversion
{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": {
    "name": "read_nda",
    "arguments": {
      "ndaPath": "C:\\Projects\\MyApp\\Program.nda"
    }
  },
  "id": 2
}

// 3. Execute the NDA
{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": {
    "name": "execute_nda",
    "arguments": {
      "ndaPath": "C:\\Projects\\MyApp\\Program.nda",
      "arguments": ["--config", "production"]
    }
  },
  "id": 3
}
```

### Best Practices

1. **Always use absolute paths** — The server rejects relative paths for security
2. **Validate inputs early** — Your C# tool should validate arguments before processing
3. **Return structured output** — JSON or well-formatted text converts to better NDA triples
4. **Handle timeouts** — The server kills your process after 30 seconds. Design long-running tools to checkpoint progress
5. **Keep stdout clean** — Only write the JSON-RPC response to stdout. Use stderr or log files for debug output
6. **Test with read_nda** — After converting any file, use `read_nda` to verify the triples and display commands are correct
7. **Version your NDAs** — Include version metadata in your source files so converted NDAs carry version information in their triples

---

## 5. Shared Memory Mode

Shared memory mode provides the lowest-latency communication path by using a memory-mapped file instead of stdin/stdout. This is designed for custom host applications that need maximum throughput.

### Starting in Shared Memory Mode

```bash
./target/release/velocity_mcp.exe --mode shmem --buffer-path C:\temp\mcp_buffer.bin
```

### Buffer Layout

The shared memory buffer is a 64 KB file with this layout:

```
Offset      Size     Field
─────────────────────────────────────────
0           1 byte   State byte
1–4         4 bytes  Input length (u32 LE)
5–8         4 bytes  Output length (u32 LE)
10–4095     4086 B   Input request buffer
4096–65535  61440 B  Output response buffer
```

### State Machine Protocol

```
┌──────────┐     Host writes request      ┌──────────────┐
│  IDLE(0) │ ──── and sets REQ_READY ───▶ │ REQ_READY(1) │
└──────────┘                              └──────┬───────┘
                                                 │ Server reads request
                                                 │ and sets PROCESSING
                                                 ▼
┌──────────┐     Host reads response    ┌──────────────┐
│RES_READY │ ◀──── and sets IDLE ────── │ PROCESSING(2)│
│   (3)    │                            └──────────────┘
└──────────┘
```

### Host Integration Steps

1. **Create/open** the buffer file at the agreed path
2. **Write** your JSON-RPC request string into the input buffer (offset 10)
3. **Set** the input length field (offset 1, u32 LE)
4. **Set** state to `1` (REQ_READY) with a Release store
5. **Poll** the state byte until it reads `3` (RES_READY) with an Acquire load
6. **Read** the output length field (offset 5, u32 LE)
7. **Read** the response string from the output buffer (offset 4096)
8. **Set** state back to `0` (IDLE) or `1` (REQ_READY) for the next request

### Synchronization Requirements

Cross-process correctness requires proper memory ordering:

- **State byte**: Use atomic load/store with Acquire/Release ordering
- **Length fields**: Issue a `SeqCst` memory fence between reading/writing length fields and checking/setting the state byte
- **Writer sequence**: Write data → write length → `SeqCst` fence → set state (Release)
- **Reader sequence**: Read state (Acquire) → `SeqCst` fence → read length → read data

### Error State

If the server encounters an error (e.g., invalid JSON, internal error), it sets state to `4` (ERROR). The host should read the output buffer for the error response, then reset state to `IDLE`.

---

## 6. Health Checks

Both modes support the `health/check` JSON-RPC method for monitoring.

### Stdio Mode

```json
→ {"jsonrpc":"2.0","method":"health/check","id":1}
← {"jsonrpc":"2.0","id":1,"result":{"status":"healthy","mode":"stdio","version":"2.0.0"}}
```

### Shared Memory Mode

```json
→ {"jsonrpc":"2.0","method":"health/check","id":1}
← {"jsonrpc":"2.0","id":1,"result":{"status":"healthy","mode":"shmem","version":"2.0.0","buffer_path":"nmcp_buffer.bin"}}
```

Use health checks to verify the server is responsive before sending tool calls, or for periodic monitoring in production deployments.

---

## 7. Logging and Diagnostics

### Log Levels

The server uses structured logging via the `tracing` crate. Set the log level with the `RUST_LOG` environment variable:

| Level | Usage |
|-------|-------|
| `error` | Only errors (tool failures, process crashes) |
| `warn` | Errors + warnings (unknown methods, path rejections) |
| `info` | Errors + warnings + info (startup, tool dispatch, shutdown) — **default** |
| `debug` | All above + debug (per-request method names, tool dispatch details) |
| `trace` | Maximum verbosity |

### Examples

```bash
# Default (info level)
./velocity_mcp.exe --mode stdio

# Debug logging for troubleshooting
RUST_LOG=debug ./velocity_mcp.exe --mode stdio

# Only errors
RUST_LOG=error ./velocity_mcp.exe --mode shmem
```

### Log Output Format

Logs are written to stderr in this format:

```
2026-08-17T20:00:00.123456Z  INFO Starting V.E.L.O.C.I.T.Y. NMCP Server...
2026-08-17T20:00:00.234567Z  INFO csharp_path=C:\path\to\NdaMcpServer.exe C# core engine path resolved
2026-08-17T20:00:01.345678Z  INFO method=initialize Processing JSON-RPC request
2026-08-17T20:00:02.456789Z  INFO tool=read_nda Delegating to C# core engine
```

### Startup Diagnostics

At startup, the server logs:
- The selected protocol mode (stdio/shmem)
- The resolved C# engine path (from env var or default)
- The shared memory buffer path (shmem mode only)

---

## 8. Performance Benchmarks

Run the built-in benchmark suite:

```bash
./target/release/velocity_mcp.exe --benchmark
```

### What's Measured

| Benchmark | Description | Iterations |
|-----------|-------------|------------|
| JSON-RPC Parse | Full `serde_json` parse of a tool call request | 500,000 |
| Mmapped Buffer R/W | Write to input buffer + read back from input buffer | 200,000 |
| Zero-Alloc Binary Parse | NMCP binary frame header parse (magic + merkle root) | 1,000,000 |

### Reference Results (Intel Core i5-14400F)

**Low-Level Parsing:**

| Operation | Mean Latency | Throughput |
|-----------|:------------:|:----------:|
| JSON-RPC Parse | 637 ns | ~1.57M req/s |
| Shared Memory R/W | 52 ns | ~19.2M ops/s |
| Binary Frame Parse | 3.06 ns | ~327M frames/s |

**End-to-End Tool Calls:**

| Operation | Mean Latency | Notes |
|-----------|:------------:|:------|
| JSON Tool Call | 104 ms | convert_to_nda_document (includes process spawn) |
| NDA Tool Call | 78 ms | read_nda (1.34x faster) |

The binary parser is **208x faster** than JSON parsing because it performs zero-copy pointer casts instead of string parsing. End-to-end tool calls show 1.34x speedup because process spawning and IPC overhead dominate. The `convert_to_nda_tool` tool enables native NDA binary format for tool calls, achieving 97.6x faster parsing than JSON. Converted tools are automatically registered and immediately callable by name.

---

## 9. Security Model

The V.E.L.O.C.I.T.Y. server implements 12 defense layers providing comprehensive protection for a local MCP server.

### Defense Layers

| # | Layer | Description |
|---|-------|-------------|
| 1 | **Input Validation** | Bounds-checked NDA parser, spec-compliant XML (quick-xml), path traversal rejection |
| 2 | **Capability-Based Sandbox** | Restricted profile: no network, filesystem isolated to work dir, 6 approved interpreters |
| 3 | **OS-Level Resource Limits** | Windows Job Object memory cap (256 MB) for sandboxed processes |
| 4 | **Execution Timeout** | 30-second hard deadline with process kill |
| 5 | **Output Size Limits** | stdout: 1 MB, stderr: 256 KB — prevents OOM from runaway output |
| 6 | **Ed25519 Signatures** | Sign and verify NDA documents for authenticity and tamper detection |
| 7 | **Merkle Tree Integrity** | SHA-256 root verification of NDA binary content |
| 8 | **Rate Limiting** | Token bucket: 20 req/sec, burst 100 — prevents abuse |
| 9 | **Audit Logging** | 10K entry ring buffer, poisoning-tolerant mutex, global instance |
| 10 | **Error Sanitization** | Strips internal paths (Win/Unix), truncates at 500 chars |
| 11 | **Dependency Audit** | 0 vulnerabilities across 95 crate dependencies (`cargo audit`) |
| 12 | **CI/CD Automation** | GitHub Actions: build + test + cargo audit on every push/PR |

### Path Validation

All file paths provided to tools are validated before execution:

| Check | Rejects | Example |
|-------|---------|--------|
| Empty path | `""` | Missing parameter |
| Relative path | `"documents\file.nda"` | Not anchored to a drive |
| Path traversal | `"C:\Users\..\..\Windows\System32"` | Directory traversal attack |
| Must be absolute | `"./relative/file"` | Relative with `./` prefix |

### Capability-Based Sandbox

All NDA execution goes through a capability-based sandbox adapted from Velocity-IDE's TabSandbox:

**ProcessCapabilities (restricted profile):**
- No network access
- File system isolated to sandbox temp directory
- 6 approved interpreters: python, node, powershell, bash, cmd.exe, dotnet
- Binary payload execution allowed
- 256 MB memory cap via Windows Job Objects

**Violation tracking:**
- Categories: FileSystem, Network, Interpreter, Memory, Timeout
- All violations logged to the global audit trail
- Each violation records category, detail, and timestamp

### Ed25519 NDA Signatures

NDA documents support cryptographic signing for authenticity:

- **Signing**: `compile_signed()` signs the entire NDA binary with an Ed25519 key
- **Verification**: `verify_signature()` authenticates the document and detects tampering
- **Backward compatible**: Unsigned documents still parse correctly
- **Status reporting**: `read_nda` shows signature status: VERIFIED / UNSIGNED / FAILED

The signature section (100 bytes) is appended after the string pool:
- 4 bytes: signature length
- 64 bytes: Ed25519 signature
- 32 bytes: public key

### Merkle Tree Integrity

Every NDA document contains a SHA-256 Merkle tree root in its header:
- Each leaf is hashed from `"S|P|O"` (subject|predicate|object)
- Pair-wise hashing builds up to the root
- `verify_merkle()` recomputes and compares against the header
- Detects any content modification

### Rate Limiting

Token bucket rate limiter protects against abuse:
- 20 tokens per second refill rate
- Burst capacity: 100 tokens
- Exceeded requests receive a clear error message
- Global across all tool calls

### Audit Logging

Every tool execution is recorded in a global audit log:
- Ring buffer: 10,000 entries maximum
- Each entry: sequence number, timestamp, tool name, duration, outcome
- Poisoning-tolerant mutex (recovers from panics in other threads)
- `recent(N)` retrieves the N most recent entries

### Error Sanitization

Error messages are sanitized before being returned to clients:
- Windows paths (`C:\Users\...`) stripped to `<path>`
- Unix paths (`/home/...`, `/etc/...`) stripped to `<path>`
- Long errors truncated at 500 characters
- Prevents leaking internal file system structure

### Input Size Limits

| Limit | Value | Behavior |
|-------|-------|----------|
| Max JSON-RPC request size (stdio) | 1 MB | Rejected with parse error before JSON parsing |
| Max shared memory input | 4,086 bytes | Rejected with buffer overflow error |
| Max shared memory output | 61,440 bytes | Rejected with buffer overflow error |
| Max captured stdout | 1 MB | Truncated to prevent OOM |
| Max captured stderr | 256 KB | Truncated to prevent OOM |
| Max process memory | 256 MB | Windows Job Object limit |

### Testing and Verification

146 tests verify all security layers:
- **109 unit tests**: Parser bounds checking, sandbox capabilities, signature verification
- **27 integration tests**: 15 adversarial tests covering path traversal, network blocking, XML attacks, tamper detection
- **10 property-based fuzz tests**: 2,250+ random cases proving parser never panics and signatures always verify

### Recommendations

- Run the server with the minimum required file system permissions
- Set `VELOCITY_CSHARP_PATH` to a trusted, integrity-verified executable
- Do not expose the server's stdin to untrusted network input without additional validation
- In shared memory mode, restrict buffer file permissions to the server and trusted host processes
- Run `cargo audit` periodically to check for dependency vulnerabilities

---

## 10. Troubleshooting

### "C# core engine not found at expected path"

**Cause:** The C# NdaMcpServer executable is not at the configured path.

**Fix:** Set the `VELOCITY_CSHARP_PATH` environment variable to the correct path:
```bash
set VELOCITY_CSHARP_PATH=C:\correct\path\to\NdaMcpServer.exe
```

### "C# process timed out after 30s"

**Cause:** The C# engine took longer than 30 seconds to process the request.

**Fix:** This may indicate the C# engine is hung or processing a very large file. Check the C# engine's logs. The server automatically kills the timed-out process.

### "File path contains traversal sequence '..'"

**Cause:** The provided file path contains `..` sequences, which are rejected for security.

**Fix:** Use absolute paths without directory traversal.

### "File path must be absolute"

**Cause:** A relative path was provided instead of an absolute path.

**Fix:** Provide the full absolute path (e.g., `C:\Users\me\file.nda`).

### Server doesn't respond to MCP client

**Cause:** The client may not be sending the initialization sequence correctly.

**Fix:** Ensure the client sends:
1. `initialize` request
2. `notifications/initialized` notification
3. Then `tools/list` or `tools/call`

### Shared memory: host sees stale data

**Cause:** Missing memory fence between length field writes and state transitions.

**Fix:** Ensure your host implementation follows the synchronization protocol:
- Writer: data → length → `SeqCst fence` → state (Release)
- Reader: state (Acquire) → `SeqCst fence` → length → data

### "Method 'X' not found"

**Cause:** The requested method is not supported by this server.

**Supported methods:** `initialize`, `notifications/initialized`, `tools/list`, `tools/call`, `health/check`

### Graceful shutdown not working

**Cause:** The Ctrl+C handler may have failed to install.

**Fix:** Check startup logs for "Failed to set Ctrl+C handler". This is rare and usually indicates a platform issue. The server will still exit when stdin reaches EOF (stdio mode).

---

## 11. Frequently Asked Questions

### Q: What MCP protocol version does this server support?

A: Protocol version `2024-11-05`. The server reports this in the `initialize` response.

### Q: Can I use this server with multiple MCP clients simultaneously?

A: In stdio mode, each client needs its own server process (one stdin/stdout pair). In shared memory mode, only one host can use the buffer at a time (the protocol is single-request single-response).

### Q: What happens if the C# engine crashes?

A: The server captures stderr from child processes and returns an error response to the MCP client. The error includes the child's exit status and stderr output. For built-in NDA tools, errors are handled natively in Rust.

### Q: How do I update the server?

A: Rebuild from source with `cargo build --release` and replace the executable. Restart any MCP clients that are connected.

### Q: Can I add custom tools?

A: Tools are defined in `src/registry.rs`. Add a new `Tool` entry to `get_tools()` and a new match arm in `call_tool_with_csharp_path()`. Built-in NDA tools (convert_to_nda_document, read_nda, etc.) are implemented natively in Rust.

### Q: Is the server compatible with Linux or macOS?

A: The server is designed for Windows. The shared memory mode uses Windows file-based memory mapping. The stdio mode may work on other platforms with the Rust toolchain, but file path validation assumes Windows paths.

### Q: What's the maximum file size that can be converted?

A: For built-in NDA tools, file size is limited by available memory. For C# engine tools, the file size is limited by the C# engine's capabilities — the server passes the file path without reading the file itself.

### Q: How do I enable verbose logging?

A: Set `RUST_LOG=debug` or `RUST_LOG=trace` before starting the server. See [Logging and Diagnostics](#6-logging-and-diagnostics) for details.
