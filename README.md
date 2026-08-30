# V.E.L.O.C.I.T.Y. Neural Model Context Protocol (NMCP) Server

[![CI](https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/actions/workflows/ci.yml/badge.svg)](https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/actions/workflows/ci.yml)
[![Version](https://img.shields.io/badge/version-3.0.0-blue.svg)](https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/releases)
[![License](https://img.shields.io/badge/license-MIT%20|%20Apache%202.0-green.svg)](LICENSE)
[![Tests](https://img.shields.io/badge/tests-274%20passing-brightgreen.svg)]()
[![Dependencies](https://img.shields.io/badge/dependencies-95%20crates%20|%200%20vulns-brightgreen.svg)]()

A high-performance, production-hardened Model Context Protocol (MCP) server written in Rust. Designed to replace slow, bloated Node.js/Python MCP servers with a highly optimized, self-contained executable.

## 🚀 Quick Start

**New to VELOCITY-MCP?** Start here:

- 📖 [Getting Started Guide](GETTING_STARTED.md) - Install and run in 5 minutes
- 🔄 [Migration Guide](MIGRATION.md) - Moving from Node.js MCP? We've got you covered
- 🔌 [Client Integration](CLIENT_INTEGRATION.md) - Setup for Claude Desktop, Cursor, Windsurf, and more
- 💡 [Examples](examples/) - Working code samples for common use cases
- 📊 [Performance Comparison](COMPARISON.md) - See why we're 3.8x faster

**30-second install:**
```bash
# Download and run
curl -L https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/releases/latest/download/velocity_mcp -o velocity_mcp
chmod +x velocity_mcp
./velocity_mcp
```

That's it! Your MCP server is running. Now configure your client (see [Client Integration](CLIENT_INTEGRATION.md)).

---

## Why VELOCITY-MCP?

| Feature | Node.js MCP | VELOCITY-MCP | You Win |
|---------|-------------|--------------|---------|
| **Speed** | Baseline | **3.8x faster** | Lower latency, higher throughput |
| **Memory** | ~120 MB | **~15 MB** | 8x smaller footprint |
| **Startup** | ~500ms | **<50ms** | 10x faster startup |
| **Security** | Basic | **Production-hardened** | Timeouts, rate limits, validation |
| **Config** | Complex | **Zero-config** | Works out of the box |
| **Protocol** | JSON only | **JSON + NDA binary** | 90x faster parsing |

---

> **Deep dive?** Read the [User Guide](docs/USER_GUIDE.md) for complete technical documentation — architecture, shared memory integration, protocol details, and troubleshooting.

---

## Architecture

The server uses a **native Rust architecture** with all NDA (Neural Document Archive) operations implemented directly in Rust — no C# delegation required. The MCP protocol layer, tool registration, NDA compilation/parsing/execution, sandbox isolation, and security enforcement all run in a single self-contained executable.

### Dual-Protocol Execution Modes

| Mode | Transport | Use Case |
|------|-----------|----------|
| **Stdio JSON-RPC v2.0** (`--mode stdio`) | Standard input/output | Compatible with MCP clients (Claude Desktop, Cursor, IDE plugins) |
| **Shared Memory** (`--mode shmem`) | Memory-mapped file IPC | Zero-copy, lowest-latency communication for custom hosts |

**Stdio mode** uses a reader thread + channel architecture so that stdin reads never block shutdown checks. Full MCP spec compliance: `ping`, `logging/setLevel`, `notifications/cancelled`, cursor pagination on `tools/list`.

**Shared memory mode** supports two wire formats with auto-detection:
- **NDA-native** (binary): Frames with `NMCP` magic + SHA-256 Merkle root + TLV-encoded payloads. Zero JSON parsing on the hot path.
- **JSON-RPC** (backwards-compatible): Standard JSON-RPC strings for existing clients.

The server detects the frame type by checking for the `NMCP` magic bytes. NDA-native frames bypass JSON serialization entirely — method types are single bytes, arguments use TLV binary encoding, and Merkle roots verify integrity.

Cross-process synchronization uses `AtomicU8` with Acquire/Release ordering on the state byte, `SeqCst` fences on length fields, and **Win32 Events** (`CreateEventW`/`WaitForSingleObject`/`SetEvent`) for zero-poll blocking waits on Windows.

---

## Tools

The server registers four built-in tools:

| Tool | Description | Required Params |
|------|-------------|------------------|
| `convert_to_nda_document` | Convert any file (source code, PDF, CSV, Excel, Image) into an `.nda` binary document with semantic triples and Ed25519 signature | `filePath` (absolute) |
| `convert_to_nda_tool` | Convert a JSON-RPC tool call into native NDA binary format for faster binary parsing | `jsonRequest` |
| `read_nda` | Read and parse a compiled `.nda` binary — shows triples, display commands, Merkle verification, and Ed25519 signature status | `ndaPath` (absolute) |
| `execute_nda` | Execute a runnable `.nda` container in a capability-based sandbox with process isolation | `ndaPath` (absolute) |

All file paths are validated before execution: empty paths, relative paths, and path traversal sequences (`..`) are rejected.

---

## Security

The server implements **12 defense layers** providing comprehensive protection for a local MCP server:

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

### NDA Document Security

NDA documents support **Ed25519 cryptographic signatures** for authenticity:
- `compile_signed()` signs the entire NDA binary with an Ed25519 key
- `verify_signature()` authenticates the document and detects tampering
- Signature section is backward-compatible (unsigned documents still parse)
- `read_nda` reports signature status: VERIFIED / UNSIGNED / FAILED

### Sandbox Security

All NDA execution goes through a capability-based sandbox (adapted from Velocity-IDE's TabSandbox):
- **ProcessCapabilities** defines what a sandboxed process may access
- **Violation tracking** with categories: FileSystem, Network, Interpreter, Memory, Timeout
- **Isolated temp directory** per execution, cleaned up after completion
- **Panic catching** during sandbox setup
- All violations logged to the global audit trail

---

## Testing

**233 tests** across 4 test suites — 0 failures:

| Suite | Tests | Coverage |
|-------|:-----:|----------|
| Unit tests | 178 | Parser, sandbox, signatures, Merkle, rate limiter, audit, error sanitization, NDA-native protocol, MCP spec compliance, HTTP/SSE transport, OAuth2 flow, streaming state, resource subscriptions, sampling conversations, proc macro type inference |
| Integration tests | 27 | Full pipeline, path validation, registry dispatch, 15 adversarial tests |
| Property-based fuzz (proptest) | 17 | 3,400+ random cases: round-trips, random bytes, Unicode, signature corruption, NDA-native frame integrity, TLV encoding, Merkle tampering, truncation |
| Proc macro tests | 11 | Type-safe tool registration, Vec<T> support, constraint validation, auto-registration |
| **Total** | **233** | |

### Adversarial Tests

15 integration tests specifically target attack vectors:
- **XML Parsing**: malformed XLSX, deeply nested XML, empty workbooks
- **Sandbox Escape**: path traversal (6 patterns), network blocking, interpreter control, violation recording
- **Signature Verification**: tamper detection, registry dispatch with signed documents
- **Rate Limiter**: burst exhaustion and throttling
- **Audit Log**: ring buffer overflow (1,000 entries)
- **Error Sanitization**: path stripping, truncation
- **NDA Parser Robustness**: corrupted headers (byte-by-byte), truncation, garbage append

### Running Tests

```bash
# All tests
cargo test

# Fuzz tests only
cargo test --test fuzz_tests

# Integration tests only
cargo test --test integration

# Dependency audit
cargo audit
```

---

## Building

### Prerequisites
- Rust toolchain (Cargo/Rustc)

### Release Build
```bash
cargo build --release
```
Produces an optimized executable at `./target/release/velocity_mcp.exe`.

Release profile: `opt-level=3`, LTO enabled, `codegen-units=1`, `panic=abort`, symbols stripped.

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
| `--mode <stdio\|shmem\|http>` | Protocol mode | `stdio` |
| `--buffer-path <path>` | Shared memory buffer file path (shmem mode only) | `nmcp_buffer.bin` |
| `--addr <address>` | HTTP listen address (http mode only) | `0.0.0.0:3000` |
| `--benchmark` | Run the performance benchmark suite | — |
| `-h, --help` | Print help screen | — |

### Feature Flags

Build with optional features using `--features`:

| Feature | Description |
|---------|-------------|
| `http` | HTTP/SSE transport with session management and streaming |
| `oauth2` | OAuth2 connector framework with token refresh |

Example: `cargo build --release --features http,oauth2`

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `VELOCITY_CSHARP_PATH` | Override path to the C# `NdaMcpServer.exe` (for dynamic tool hosting) | Hardcoded debug path |
| `RUST_LOG` | Set log level (`error`, `warn`, `info`, `debug`, `trace`) | `info` |

---

## Features

### Native NDA Operations
All NDA compile/read/execute operations run natively in Rust — no external process delegation:
- **NDA Compiler**: Builds binary documents with semantic triples, display commands, string pool, and Merkle tree
- **NDA Parser**: Zero-copy reads with bounds checking, Merkle verification, and Ed25519 signature verification
- **NDA Executor**: Runs binary payloads and scripts in a capability-based sandbox

### File Format Support
Converts files to NDA binary format:
- **CSV**: Spreadsheet grid with cell triples and visual layout
- **XLSX**: Excel workbook parsing via zip + spec-compliant XML (quick-xml)
- **DOCX**: Word document parsing via zip + spec-compliant XML
- **PDF**: Raw text stream extraction
- **Images** (PNG, JPG, WebP): Base64 data URL with DrawImage command
- **Source code** (20+ languages): Syntax-colored code editor layout
- **Binary**: Hex dump viewer with base64 payload recovery

### Dynamic Tool Hosting
The server automatically discovers and hosts tools from the C# backend engine. On first `tools/list` request, it queries the C# engine for available tools, caches them, and merges with the built-in NDA tools (deduplicating by name). Any tool added to the C# engine is immediately available through the Rust server — no configuration needed.

### Graceful Shutdown
Ctrl+C sets an atomic shutdown flag. Both stdio and shmem loops poll this flag and exit cleanly. The stdio loop uses a reader thread + `recv_timeout` pattern so stdin reads never block shutdown. The shmem loop removes the buffer file on exit.

### Health Checks
Both modes support the `health/check` JSON-RPC method, returning server status, mode, version, and (in shmem mode) the buffer path.

### Structured Logging
Uses `tracing` + `tracing-subscriber` with env-filter support. All significant events (startup, tool dispatch, errors, shutdown) are logged with structured fields.

---

## Advanced Features (v3.0)

### HTTP/SSE Transport
Full HTTP transport with session management and Server-Sent Events for real-time streaming:
- **Session Management**: Automatic session creation, tracking, and cleanup
- **Streamable HTTP**: POST requests with SSE responses for streaming large results
- **SSE Streaming**: Real-time event streaming for tool progress and updates
- **Request ID Correlation**: Track requests across HTTP connections
- **Connection Lifecycle**: Connect/disconnect event handling
- **Endpoints**: `/mcp` (JSON-RPC), `/mcp/stream` (Streamable HTTP), `/sse` (SSE), `/sessions` (management)

### Type-Safe Tool Registration (Proc Macros)
Compile-time tool registration with automatic JSON schema generation:
```rust
#[mcp_tool(
    name = "read_file",
    description = "Read a file from disk",
    param_constraints = {
        "path": { "min_length": 1 },
        "offset": { "minimum": 0 }
    }
)]
fn read_file(path: String, offset: Option<i64>) -> Result<String, String> {
    // implementation
}
```
- **Automatic Schema Generation**: Converts Rust types to JSON Schema
- **Constraint Validation**: min/max length, patterns, defaults
- **Vec<T> Support**: Array parameters with item type schemas
- **Nested Structs**: Recursive schema generation for complex types
- **Auto-Registration**: Tools automatically registered in global registry

### Resources & Prompts
MCP Resources and Prompts with advanced features:
- **Resource Types**: File, database, and API resources
- **Resource Subscriptions**: Real-time change notifications via `resources/subscribe`
- **URI Template Expansion**: Parameterized resource URIs
- **Structured Prompts**: Multi-message prompts with text and resource content blocks
- **Database Adapters**: Query execution with connection pooling (SQLite, PostgreSQL)
- **API Adapters**: HTTP client integration with authentication

### Sampling Protocol
Server-initiated LLM sampling with full conversation support:
- **Model Preferences**: Hints, cost/speed/intelligence priorities
- **System Prompts**: Context injection for sampling requests
- **Multi-Turn Conversations**: Automatic history tracking and management
- **Metadata Support**: Progress tokens, conversation IDs, custom metadata
- **Conversation API**: `add_to_conversation()`, `get_conversation()`, `clear_conversation()`

### Streaming Responses
Real-time streaming with progress tracking:
- **Result Chunking**: Split large results into manageable chunks
- **Streaming State**: Track progress, chunks sent, completion status
- **Progress Notifications**: `notifications/progress` with progress tokens
- **Backpressure**: Channel-based flow control
- **SSE Integration**: Stream chunks via Server-Sent Events

### OAuth2 Connector Framework
Complete OAuth2 implementation for external service integration:
- **Authorization Flow**: Authorize URL generation, code exchange, token refresh
- **Token Management**: Expiration tracking, automatic refresh, secure storage
- **Pre-built Connectors**: GitHub, Google templates with common scopes
- **Webhook Support**: Event notifications with signature verification
- **State Validation**: CSRF protection with state parameter validation

### Performance Characteristics
- **HTTP Throughput**: ~10K req/s (single connection)
- **SSE Latency**: <1ms for event delivery
- **OAuth2 Token Refresh**: <100ms with automatic retry
- **Streaming Chunk Size**: Configurable (default 10KB)
- **Session Limit**: 1000 concurrent sessions (configurable)

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

### Wire Formats

The server auto-detects the wire format by checking the first 4 bytes of the input buffer:

**NDA-native** (starts with `NMCP`):
```
[4 bytes: magic "NMCP"]
[32 bytes: Merkle root (SHA-256 of payload)]
[1 byte:  method type (0x01=initialize, 0x02=tools/list, 0x03=tools/call, 0x04=ping, ...)]
[TLV:     request id]
[TLV:     method-specific data]
```

**JSON-RPC** (anything else): Standard JSON-RPC v2.0 string.

### Synchronization Protocol

```
WRITER: write data → write length → SeqCst fence → set state (Release) → signal event
READER: wait for event → read state (Acquire) → SeqCst fence → read length → read data
```

On Windows, `CreateEventW`/`WaitForSingleObject`/`SetEvent` provide zero-poll blocking waits. On other platforms, a 100μs sleep fallback is used. The Acquire/Release pair on the state byte combined with SeqCst fences ensures that length field writes are globally visible before state transitions.

---

## Performance Benchmarks

Built-in micro-benchmark suite (`--benchmark`) comparing protocol parsing and IPC latency.

### Benchmark Results (Intel Core i5-14400F, release build)

**Protocol Parsing (apples-to-apples, same tool call, full field extraction):**

| Operation | Mean Latency | Throughput |
|-----------|:------------:|:----------:|
| JSON-RPC parse + extract | 722.6 ns | ~1.38M req/s |
| NDA-native parse + Merkle + extract | 459.1 ns | ~2.18M req/s |
| **NDA speedup** | | **1.6x faster** |

**Shared Memory Throughput:**

| Operation | Mean Latency | Throughput |
|-----------|:------------:|:----------:|
| JSON-in-shmem R/W | 38.8 ns | 25.8M ops/s |
| NDA-native shmem R/W (raw bytes) | 12.6 ns | 79.2M ops/s |
| **NDA speedup** | | **3.1x faster** |

**Concurrency (NDA-native dispatch, req/s):**

| Threads | Throughput |
|:-------:|:----------:|
| 1 | 2.86M req/s |
| 4 | 7.01M req/s |
| 8 | 12.7M req/s |

**Rust vs Node.js MCP Server (stdio, 200 req/method):**

| Method | Node.js avg | Rust avg | Speedup |
|--------|:-----------:|:--------:|:-------:|
| ping | 0.573 ms | 0.157 ms | **3.6x** |
| tools/list | 1.050 ms | 0.154 ms | **6.8x** |
| tools/call | 0.546 ms | 0.136 ms | **4.0x** |
| **Overall** | 0.627 ms | 0.164 ms | **3.8x** |

The NDA-native protocol (with SHA-256 Merkle integrity verification) is 1.6x faster than JSON for parse + field extraction. The legacy zero-copy binary frame parser (no Merkle verification) measures at ~3 ns but this includes payload iteration — the parse itself is pointer arithmetic only. The real-world wins are in shmem throughput (3.1x) and concurrent scaling (12.7M req/s at 8 threads).

---

## Repository Structure

```
├── src/
│   ├── lib.rs               # Library crate root (public API, VERSION constant)
│   ├── main.rs              # CLI entry point, arg parsing, shutdown
│   ├── registry.rs          # Tool registration, dispatch, path validation, TLV encoding
│   ├── nda_document.rs      # NDA binary format (compile, read, Merkle tree, Ed25519 signatures)
│   ├── nda_converter.rs     # File-to-NDA converters (CSV, XLSX, DOCX, PDF, Image, Code, Binary)
│   ├── nda_executor.rs      # NDA payload execution (BinaryPayload, SourceCode)
│   ├── sandbox.rs           # Capability-based sandbox (ProcessCapabilities, violation tracking, Job Objects)
│   ├── audit.rs             # Audit logging (ring buffer, global instance)
│   ├── rate_limit.rs        # Token bucket rate limiter
│   ├── benchmark.rs         # Performance benchmark suite
│   ├── resources.rs         # MCP Resources & Prompts (subscriptions, DB/API adapters, structured prompts)
│   ├── sampling.rs          # MCP Sampling protocol (model preferences, conversations, metadata)
│   ├── streaming.rs         # Streaming responses (chunking, state management, progress tokens)
│   ├── oauth2.rs            # OAuth2 connector framework (auth flow, token refresh, webhooks)
│   ├── ipc/
│   │   ├── mod.rs           # IPC module
│   │   └── shmem.rs         # Memory-mapped buffer, atomic state machine, Win32 Events
│   ├── protocol/
│   │   ├── mod.rs           # Protocol module
│   │   ├── json_rpc.rs      # Stdio JSON-RPC handler (MCP spec compliant)
│   │   ├── nmcp_binary.rs   # Shared memory protocol loop (auto-detect NDA/JSON)
│   │   └── nda_native.rs    # NDA-native binary protocol (frames, TLV, Merkle)
│   └── transport/
│       ├── mod.rs           # Transport module
│       └── http.rs          # HTTP/SSE transport (session management, streaming, endpoints)
├── macros/
│   ├── Cargo.toml           # Proc-macro crate configuration
│   └── src/
│       └── lib.rs           # Type-safe tool registration macro (#[mcp_tool])
├── tests/
│   ├── integration.rs       # Cross-module integration + adversarial tests (27 tests)
│   ├── fuzz_tests.rs        # Property-based fuzz tests with proptest (17 tests)
│   ├── macro_test.rs        # Proc macro tests (7 tests)
│   └── macro_enhanced_test.rs # Enhanced proc macro tests (4 tests)
├── bench_nodejs/            # Node.js comparison benchmark
│   ├── server.js            # Node.js MCP server for comparison
│   └── benchmark.js         # Benchmark harness
├── .github/
│   └── workflows/
│       ├── ci.yml           # GitHub Actions CI (build, test, audit, fuzz)
│       └── release.yml      # Automated release binary build on version tags
├── docs/
│   └── USER_GUIDE.md        # Comprehensive user guide
├── Cargo.toml               # Dependencies and build configuration
├── Cargo.lock               # Locked dependency versions (reproducible builds)
├── CHANGELOG.md             # Release history (Keep a Changelog format)
├── LICENSE                  # Apache-2.0 / MIT dual license
├── .gitignore               # Git exclusions
└── README.md                # This file
```

---

## CI/CD

Two GitHub Actions workflows run automatically:

| Workflow | Trigger | Description |
|----------|---------|-------------|
| **CI** (`ci.yml`) | Every push/PR to `main` | Build + run all 233 tests + cargo audit + fuzz tests |
| **Release** (`release.yml`) | Version tags (`v*`) | Build release binary + test + audit + create GitHub Release |

Three CI jobs run on every push:

| Job | Description |
|-----|-------------|
| **Build & Test** | Compile + run all 233 tests on Windows |
| **Security Audit** | `cargo audit` for dependency vulnerabilities |
| **Fuzz Tests** | Run all proptest property-based tests |

---

## Dependencies

| Crate | Purpose |
|-------|---------|
| `serde` / `serde_json` | JSON serialization |
| `memmap2` | Memory-mapped files for shared memory IPC |
| `tracing` / `tracing-subscriber` | Structured logging |
| `ctrlc` | Graceful shutdown handler |
| `sha2` | SHA-256 for Merkle tree |
| `base64` | Base64 encoding for binary payloads |
| `zip` | XLSX/DOCX archive parsing |
| `regex` | Error sanitization, PDF text extraction |
| `quick-xml` | Spec-compliant XML parsing for XLSX/DOCX |
| `ed25519-dalek` | Ed25519 signature generation and verification |
| `rand` | Cryptographic random number generation |
| `uuid` | Session ID generation (HTTP transport) |
| `tokio` | Async runtime (HTTP/SSE transport) |
| `axum` | Web framework (HTTP transport) |
| `tower` / `tower-http` | Middleware stack (HTTP transport) |
| `tokio-stream` | Stream utilities (SSE streaming) |
| `ureq` | HTTP client (OAuth2, API resources) |
| `syn` / `quote` / `proc-macro2` | Proc macro support (type-safe tools) |

**Dev dependencies:** `proptest` (property-based fuzz testing)

---

## License

Licensed under either of [Apache License, Version 2.0](LICENSE) or [MIT License](LICENSE), at your option.
