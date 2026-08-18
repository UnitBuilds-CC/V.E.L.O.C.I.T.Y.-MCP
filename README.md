# V.E.L.O.C.I.T.Y. Neural Model Context Protocol (NMCP) Server

[![CI](https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/actions/workflows/ci.yml/badge.svg)](https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/actions/workflows/ci.yml)
[![Version](https://img.shields.io/badge/version-2.0.0-blue.svg)](https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/releases)
[![License](https://img.shields.io/badge/license-MIT%20|%20Apache%202.0-green.svg)](LICENSE)
[![Tests](https://img.shields.io/badge/tests-146%20passing-brightgreen.svg)]()
[![Dependencies](https://img.shields.io/badge/dependencies-95%20crates%20|%200%20vulns-brightgreen.svg)]()

A high-performance, production-hardened Model Context Protocol (MCP) server written in Rust. Designed to replace slow, bloated Node.js/Python MCP servers with a highly optimized, self-contained executable.

> **New here?** Read the [User Guide](docs/USER_GUIDE.md) for a complete walkthrough — client configuration, tool usage, shared memory integration, troubleshooting, and FAQ.

---

## Architecture

The server uses a **native Rust architecture** with all NDA (Neural Document Archive) operations implemented directly in Rust — no C# delegation required. The MCP protocol layer, tool registration, NDA compilation/parsing/execution, sandbox isolation, and security enforcement all run in a single self-contained executable.

### Dual-Protocol Execution Modes

| Mode | Transport | Use Case |
|------|-----------|----------|
| **Stdio JSON-RPC v2.0** (`--mode stdio`) | Standard input/output | Compatible with MCP clients (Claude Desktop, Cursor, IDE plugins) |
| **Shared Memory** (`--mode shmem`) | Memory-mapped file IPC | Zero-copy, lowest-latency communication for custom hosts |

**Stdio mode** uses a reader thread + channel architecture so that stdin reads never block shutdown checks. Requests are parsed, validated, and dispatched natively.

**Shared memory mode** uses a 64 KB memory-mapped file with a state machine protocol (Idle → Request Ready → Processing → Response Ready). Cross-process synchronization uses `AtomicU8` with Acquire/Release ordering on the state byte and `SeqCst` fences on length fields to ensure correct visibility across processes on x86_64.

---

## Tools

The server registers four built-in tools:

| Tool | Description | Required Params |
|------|-------------|------------------|
| `convert_to_nda_document` | Convert any file (source code, PDF, CSV, Excel, Image) into an `.nda` binary document with semantic triples and Ed25519 signature | `filePath` (absolute) |
| `convert_to_nda_tool` | Convert a JSON-RPC tool call into native NDA binary format for 97x faster parsing | `jsonRequest` |
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

**146 tests** across 4 test suites — 0 failures, 0 warnings:

| Suite | Tests | Coverage |
|-------|:-----:|----------|
| Unit tests | 109 | Parser, sandbox, signatures, Merkle, rate limiter, audit, error sanitization |
| Integration tests | 27 | Full pipeline, path validation, registry dispatch, 15 adversarial tests |
| Property-based fuzz (proptest) | 10 | 2,250+ random cases: round-trips, random bytes, Unicode, signature corruption |
| **Total** | **146** | |

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
| `--mode <stdio\|shmem>` | Protocol mode | `stdio` |
| `--buffer-path <path>` | Shared memory buffer file path (shmem mode only) | `nmcp_buffer.bin` |
| `--benchmark` | Run the performance benchmark suite | — |
| `-h, --help` | Print help screen | — |

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
│   ├── lib.rs               # Library crate root (public API, VERSION constant)
│   ├── main.rs              # CLI entry point, arg parsing, shutdown
│   ├── registry.rs          # Tool registration, dispatch, path validation
│   ├── nda_document.rs      # NDA binary format (compile, read, Merkle tree, Ed25519 signatures)
│   ├── nda_converter.rs     # File-to-NDA converters (CSV, XLSX, DOCX, PDF, Image, Code, Binary)
│   ├── nda_executor.rs      # NDA payload execution (BinaryPayload, SourceCode)
│   ├── sandbox.rs           # Capability-based sandbox (ProcessCapabilities, violation tracking, Job Objects)
│   ├── audit.rs             # Audit logging (ring buffer, global instance)
│   ├── rate_limit.rs        # Token bucket rate limiter
│   ├── benchmark.rs         # Performance benchmark suite
│   ├── ipc/
│   │   ├── mod.rs           # IPC module
│   │   └── shmem.rs         # Memory-mapped buffer, atomic state machine
│   └── protocol/
│       ├── mod.rs           # Protocol module
│       ├── json_rpc.rs      # Stdio JSON-RPC handler
│       └── nmcp_binary.rs   # Shared memory protocol loop + binary parser
├── tests/
│   ├── integration.rs       # Cross-module integration + adversarial tests (27 tests)
│   └── fuzz_tests.rs        # Property-based fuzz tests with proptest (10 tests)
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
| **CI** (`ci.yml`) | Every push/PR to `main` | Build + run all 146 tests + cargo audit + fuzz tests |
| **Release** (`release.yml`) | Version tags (`v*`) | Build release binary + test + audit + create GitHub Release |

Three CI jobs run on every push:

| Job | Description |
|-----|-------------|
| **Build & Test** | Compile + run all 146 tests on Windows |
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

**Dev dependencies:** `proptest` (property-based fuzz testing)

---

## License

Licensed under either of [Apache License, Version 2.0](LICENSE) or [MIT License](LICENSE), at your option.
