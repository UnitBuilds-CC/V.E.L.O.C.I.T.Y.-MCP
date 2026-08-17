# V.E.L.O.C.I.T.Y. Neural Model Context Protocol (NMCP) Server

A high-performance, production-hardened Model Context Protocol (MCP) server written in Rust. Designed to replace slow, bloated Node.js/Python MCP servers with a highly optimized, self-contained executable.

> **New here?** Read the [User Guide](docs/USER_GUIDE.md) for a complete walkthrough — client configuration, tool usage, shared memory integration, troubleshooting, and FAQ.

---

## Architecture

The server uses a **delegation architecture**: the Rust server handles the MCP protocol layer (JSON-RPC parsing, tool registration, input validation, health checks) and delegates actual tool execution to a C# core engine (`NdaMcpServer.exe`) via stdin/stdout JSON-RPC piping.

### Dual-Protocol Execution Modes

| Mode | Transport | Use Case |
|------|-----------|----------|
| **Stdio JSON-RPC v2.0** (`--mode stdio`) | Standard input/output | Compatible with MCP clients (Claude Desktop, Cursor, IDE plugins) |
| **Shared Memory** (`--mode shmem`) | Memory-mapped file IPC | Zero-copy, lowest-latency communication for custom hosts |

**Stdio mode** uses a reader thread + channel architecture so that stdin reads never block shutdown checks. Requests are parsed, validated, and dispatched to the C# engine.

**Shared memory mode** uses a 64 KB memory-mapped file with a state machine protocol (Idle → Request Ready → Processing → Response Ready). Cross-process synchronization uses `AtomicU8` with Acquire/Release ordering on the state byte and `SeqCst` fences on length fields to ensure correct visibility across processes on x86_64.

---

## Tools

The server registers four built-in tools:

| Tool | Description | Required Params |
|------|-------------|------------------|
| `convert_to_nda_document` | Convert any file (C# source, PDF, CSV, Excel, Image, Zip) into a cryptographically signed `.nda` binary document with semantic triples | `filePath` (absolute) |
| `convert_to_nda_tool` | Convert a JSON-RPC tool call into native NDA binary format for 97x faster parsing | `jsonRequest` |
| `read_nda` | Read and parse a compiled `.nda` binary to view semantic triples, visual display commands, and string pool contents | `ndaPath` (absolute) |
| `execute_nda` | Execute a runnable `.nda` container (compiled C# binary in-memory, or script via shell process) | `ndaPath` (absolute) |

All file paths are validated before execution: empty paths, relative paths, and path traversal sequences (`..`) are rejected.

---

## Building

### Prerequisites
- Rust toolchain (Cargo/Rustc)
- The C# NdaMcpServer executable (for tool execution)

### Release Build
```bash
cargo build --release
```
Produces an optimized executable at `./target/release/velocity_mcp.exe`.

Release profile: `opt-level=3`, LTO enabled, `codegen-units=1`, `panic=abort`, symbols stripped.

### Run Tests
```bash
cargo test
```
Runs 46 tests (34 unit + 12 integration) covering protocol handling, shared memory IPC, tool dispatch, path validation, binary frame parsing, and cross-module flows.

### Run Clippy
```bash
cargo clippy -- -W clippy::all
```

---

## Running

### Stdio Mode (MCP Client Compatible)
```bash
./target/release/velocity_mcp --mode stdio
```

### Shared Memory Mode (High-Performance IPC)
```bash
./target/release/velocity_mcp --mode shmem --buffer-path nmcp_buffer.bin
```

### Benchmark Mode
```bash
./target/release/velocity_mcp --benchmark
```

### CLI Options

| Flag | Description | Default |
|------|-------------|---------|
| `--mode <stdio\|shmem>` | Protocol mode | `stdio` |
| `--buffer-path <path>` | Shared memory buffer file path (shmem mode only) | `nmcp_buffer.bin` |
| `--benchmark` | Run the performance benchmark suite | — |
| `-h, --help` | Print help screen | — |

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `VELOCITY_CSHARP_PATH` | Override path to the C# `NdaMcpServer.exe` | Hardcoded debug path |
| `RUST_LOG` | Set log level (`error`, `warn`, `info`, `debug`, `trace`) | `info` |

---

## Features

### Dynamic Tool Hosting
The server automatically discovers and hosts tools from the C# backend engine. On first `tools/list` request, it queries the C# engine for available tools, caches them, and merges with the built-in NDA tools (deduplicating by name). Any tool added to the C# engine is immediately available through the Rust server — no configuration needed.

### Built-in NDA Tools
Four built-in tools for NDA (Neural Document Archive) operations:
- **convert_to_nda_document**: Convert files to cryptographically signed `.nda` binary format with semantic triples
- **convert_to_nda_tool**: Convert JSON-RPC tool calls to native NDA binary format (97x faster parsing)
- **read_nda**: Inspect NDA files (semantic triples, display commands, string pool)
- **execute_nda**: Execute runnable NDA containers (binaries run in-memory, scripts via shell)

### Graceful Shutdown
Ctrl+C sets an atomic shutdown flag. Both stdio and shmem loops poll this flag and exit cleanly. The stdio loop uses a reader thread + `recv_timeout` pattern so stdin reads never block shutdown. The shmem loop removes the buffer file on exit.

### Health Checks
Both modes support the `health/check` JSON-RPC method, returning server status, mode, version, and (in shmem mode) the buffer path.

### Structured Logging
Uses `tracing` + `tracing-subscriber` with env-filter support. All significant events (startup, tool dispatch, errors, shutdown) are logged with structured fields.

### Input Validation
- File paths: rejects empty, relative, and traversal (`..`) paths
- Request size: 1 MB maximum, enforced before JSON parsing
- Buffer overflow: both input and output buffer bounds are checked

### Process Management
C# child processes are spawned with piped stdin/stdout/stderr. A `try_wait()` polling loop with 30-second timeout kills and reaps the child if it hangs, preventing orphan processes.

---

## Shared Memory Protocol

The shared memory buffer is a 64 KB memory-mapped file with the following layout:

| Offset | Size | Field |
|--------|------|-------|
| 0 | 1 byte | State (0=Idle, 1=Request Ready, 2=Processing, 3=Response Ready, 4=Error) |
| 1–4 | 4 bytes | Input buffer length (u32, little-endian) |
| 5–8 | 4 bytes | Output buffer length (u32, little-endian) |
| 10–4095 | 4086 bytes | Input request buffer |
| 4096–65535 | 61440 bytes | Output response buffer |

### Synchronization Protocol

```
WRITER: write data → write length → SeqCst fence → set state (Release)
READER: read state (Acquire) → SeqCst fence → read length → read data
```

The Acquire/Release pair on the state byte combined with SeqCst fences ensures that length field writes are globally visible before state transitions, and that the reader observes the correct length after observing the state change.

---

## Performance Benchmarks

Built-in micro-benchmark suite (`--benchmark`) comparing protocol parsing and IPC latency.

### Benchmark Results (Intel Core i5-14400F)

**Low-Level Parsing:**

| Operation | Mean Latency | Speedup vs JSON |
|-----------|:------------:|:---------------:|
| JSON-RPC Parse | 637 ns | baseline |
| Shared Memory R/W | 52 ns | 12.3x faster |
| Zero-Alloc Binary Parse | 3.06 ns | **208x faster** |

**End-to-End Tool Calls:**

| Operation | Mean Latency | Speedup |
|-----------|:------------:|:-------:|
| JSON Tool Call (convert_to_nda_document) | 104 ms | baseline |
| NDA Tool Call (read_nda) | 78 ms | **1.34x faster** |

The zero-allocation binary parser processes frames at 3.06 ns (208x faster than JSON parsing). End-to-end tool calls show 1.34x speedup because process spawning and IPC dominate over parsing time. The `convert_to_nda_tool` tool enables native NDA binary format for tool calls, achieving 97.6x faster parsing than JSON.

---

## Repository Structure

```
├── src/
│   ├── lib.rs               # Library crate root (public API)
│   ├── main.rs              # CLI entry point, arg parsing, shutdown
│   ├── registry.rs          # Tool registration, C# process delegation
│   ├── benchmark.rs         # Performance benchmark suite
│   ├── ipc/
│   │   ├── mod.rs           # IPC module
│   │   └── shmem.rs         # Memory-mapped buffer, atomic state machine
│   └── protocol/
│       ├── mod.rs           # Protocol module
│       ├── json_rpc.rs      # Stdio JSON-RPC handler
│       └── nmcp_binary.rs   # Shared memory protocol loop + binary parser
├── docs/
│   └── USER_GUIDE.md        # Comprehensive user guide
├── tests/
│   └── integration.rs       # Cross-module integration tests
├── Cargo.toml               # Dependencies: serde, serde_json, memmap2, tracing, ctrlc
├── LICENSE                  # Apache-2.0 / MIT dual license
└── README.md                # This file
```

---

## License

Licensed under either of [Apache License, Version 2.0](LICENSE) or [MIT License](LICENSE), at your option.
