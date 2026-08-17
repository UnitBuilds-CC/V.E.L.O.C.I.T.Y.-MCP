# V.E.L.O.C.I.T.Y.-MCP User Guide

A complete guide to installing, configuring, and using the V.E.L.O.C.I.T.Y. NMCP Server.

---

## Table of Contents

1. [Getting Started](#1-getting-started)
2. [Configuring MCP Clients](#2-configuring-mcp-clients)
3. [Using the Tools](#3-using-the-tools)
4. [Shared Memory Mode](#4-shared-memory-mode)
5. [Health Checks](#5-health-checks)
6. [Logging and Diagnostics](#6-logging-and-diagnostics)
7. [Performance Benchmarks](#7-performance-benchmarks)
8. [Security Model](#8-security-model)
9. [Troubleshooting](#9-troubleshooting)
10. [Frequently Asked Questions](#10-frequently-asked-questions)

---

## 1. Getting Started

### Prerequisites

- **Rust toolchain** (rustc 1.70+, cargo) — [Install via rustup](https://rustup.rs/)
- **C# NdaMcpServer executable** — Required for tool execution. The server delegates all tool operations to this C# engine.
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
# Run the test suite (46 tests)
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

### convert_to_nda

Convert any file into a cryptographically signed `.nda` binary document.

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
    "name": "convert_to_nda",
    "arguments": {
      "filePath": "C:\\Users\\me\\documents\\source_code.cs"
    }
  },
  "id": 1
}
```

**Supported input types:** C# source code, PDF, CSV, Excel, Image, Zip archives, and other file formats.

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

## 4. Shared Memory Mode

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

## 5. Health Checks

Both modes support the `health/check` JSON-RPC method for monitoring.

### Stdio Mode

```json
→ {"jsonrpc":"2.0","method":"health/check","id":1}
← {"jsonrpc":"2.0","id":1,"result":{"status":"healthy","mode":"stdio","version":"1.0.0"}}
```

### Shared Memory Mode

```json
→ {"jsonrpc":"2.0","method":"health/check","id":1}
← {"jsonrpc":"2.0","id":1,"result":{"status":"healthy","mode":"shmem","version":"1.0.0","buffer_path":"nmcp_buffer.bin"}}
```

Use health checks to verify the server is responsive before sending tool calls, or for periodic monitoring in production deployments.

---

## 6. Logging and Diagnostics

### Log Levels

The server uses structured logging via the `tracing` crate. Set the log level with the `RUST_LOG` environment variable:

| Level | Usage |
|-------|-------|
| `error` | Only errors (tool failures, process crashes) |
| `warn` | Errors + warnings (unknown methods, path rejections) |
| `info` | Errors + warnings + info (startup, tool dispatch, shutdown) — **default** |
| `debug` | All above + debug (per-request method names, C# delegation details) |
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

## 7. Performance Benchmarks

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

| Operation | Mean Latency | Throughput |
|-----------|:------------:|:----------:|
| JSON-RPC Parse | 653 ns | ~1.5M req/s |
| Shared Memory R/W | 59 ns | ~17M ops/s |
| Binary Frame Parse | 0.4 ns | ~2.5B frames/s |

The binary parser is **1,636x faster** than JSON parsing because it performs zero-copy pointer casts instead of string parsing.

---

## 8. Security Model

### Path Validation

All file paths provided to tools are validated before execution:

| Check | Rejects | Example |
|-------|---------|---------|
| Empty path | `""` | Missing parameter |
| Relative path | `"documents\file.nda"` | Not anchored to a drive |
| Path traversal | `"C:\Users\..\..\Windows\System32"` | Directory traversal attack |
| Must be absolute | `"./relative/file"` | Relative with `./` prefix |

### Process Isolation

- Tool execution is delegated to a separate C# child process
- Each tool call spawns a new process with piped stdin/stdout/stderr
- A 30-second timeout prevents hung processes
- Timed-out processes are killed (`SIGKILL`) and reaped to prevent orphans
- The child process runs with the same permissions as the server process

### Input Size Limits

| Limit | Value | Behavior |
|-------|-------|----------|
| Max JSON-RPC request size (stdio) | 1 MB | Rejected with parse error before JSON parsing |
| Max shared memory input | 4,086 bytes | Rejected with buffer overflow error |
| Max shared memory output | 61,440 bytes | Rejected with buffer overflow error |

### Recommendations

- Run the server with the minimum required file system permissions
- Set `VELOCITY_CSHARP_PATH` to a trusted, integrity-verified executable
- Do not expose the server's stdin to untrusted network input without additional validation
- In shared memory mode, restrict buffer file permissions to the server and trusted host processes

---

## 9. Troubleshooting

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

## 10. Frequently Asked Questions

### Q: What MCP protocol version does this server support?

A: Protocol version `2024-11-05`. The server reports this in the `initialize` response.

### Q: Can I use this server with multiple MCP clients simultaneously?

A: In stdio mode, each client needs its own server process (one stdin/stdout pair). In shared memory mode, only one host can use the buffer at a time (the protocol is single-request single-response).

### Q: What happens if the C# engine crashes?

A: The server captures stderr from the child process and returns an error response to the MCP client. The error includes the child's exit status and stderr output.

### Q: How do I update the server?

A: Rebuild from source with `cargo build --release` and replace the executable. Restart any MCP clients that are connected.

### Q: Can I add custom tools?

A: Tools are defined in `src/registry.rs`. Add a new `Tool` entry to `get_tools()` and a new match arm in `call_tool_with_csharp_path()`. The tool must be supported by the C# engine.

### Q: Is the server compatible with Linux or macOS?

A: The server is designed for Windows. The shared memory mode uses Windows file-based memory mapping. The stdio mode may work on other platforms with the Rust toolchain, but the C# engine path defaults and file path validation assume Windows paths.

### Q: What's the maximum file size that can be converted?

A: The file size is limited by the C# engine's capabilities, not the Rust server. The Rust server passes the file path to the C# engine without reading the file itself.

### Q: How do I enable verbose logging?

A: Set `RUST_LOG=debug` or `RUST_LOG=trace` before starting the server. See [Logging and Diagnostics](#6-logging-and-diagnostics) for details.
