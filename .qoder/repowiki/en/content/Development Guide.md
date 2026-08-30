# Development Guide

<cite>
**Referenced Files in This Document**
- [Cargo.toml](file://Cargo.toml)
- [src/main.rs](file://src/main.rs)
- [src/lib.rs](file://src/lib.rs)
</cite>

## Table of Contents
1. [Introduction](#introduction)
2. [Crate Organization](#crate-organization)
3. [Build System and Toolchain](#build-system-and-toolchain)
4. [Feature Flags](#feature-flags)
5. [Testing Procedures](#testing-procedures)
6. [Code Style and Conventions](#code-style-and-conventions)
7. [Adding New Features](#adding-new-features)
8. [High-Risk Areas](#high-risk-areas)

## Introduction

This guide covers development conventions, build workflows, and code organization for the V.E.L.O.C.I.T.Y. MCP Server v3.0.0. The project is a single Rust crate with a proc-macro sub-crate, organized into protocol handlers, transport layer, tool registry, sandbox, NDA operations, plugins, and observability.

## Crate Organization

The project is a single binary crate with a proc-macro sub-crate (`macros/`):

| Module | Purpose |
|--------|---------|
| `src/main.rs` | CLI argument parsing, config loading, mode dispatch |
| `src/lib.rs` | Library root with module declarations |
| `src/protocol/` | Protocol handlers (json_rpc, nmcp_binary, nda_native) |
| `src/transport/` | Transport layer (http.rs — Axum HTTP/SSE/WebSocket) |
| `src/ipc/` | IPC subsystem (shmem.rs — memory-mapped buffer) |
| `src/registry.rs` | Tool registration and dispatch (8 built-in tools) |
| `src/sandbox.rs` + `src/sandbox/` | Capability sandbox + Linux seccomp |
| `src/nda_converter.rs` | NDA binary document compiler |
| `src/nda_document.rs` | NDA parser with Merkle/signature verification |
| `src/nda_executor.rs` | NDA payload executor in sandbox |
| `src/plugins/` | Plugin system + marketplace |
| `src/observability/` | OpenTelemetry integration |
| `src/config.rs` | TOML configuration management |
| `src/audit.rs` | Global audit log (10K ring buffer) |
| `src/rate_limit.rs` | Token bucket rate limiter |
| `src/middleware.rs` | HTTP middleware (auth, CORS, logging, validation) |
| `src/resources.rs` | MCP Resources & Prompts with DB adapters |
| `src/sampling.rs` | MCP Sampling protocol |
| `src/streaming.rs` | Streaming responses with progress tokens |
| `src/oauth2.rs` | OAuth2 connector framework |
| `src/error.rs` | Error types and sanitization |
| `src/benchmark.rs` | Performance benchmarks |
| `macros/` | Proc-macro crate for `#[mcp_tool]` type-safe registration |

### Module Dependencies

```
main.rs
├── config (TOML loading)
├── protocol::json_rpc ──→ registry
├── protocol::nmcp_binary ──→ ipc::shmem, registry
├── protocol::nda_native ──→ ipc::shmem, registry
├── transport::http ──→ registry, middleware, plugins
├── registry ──→ sandbox, nda_converter, nda_executor
├── observability (optional)
└── benchmark ──→ protocol::*, ipc::shmem
```

## Build System and Toolchain

### Rust Configuration

```toml
[profile.release]
opt-level = 3       # Maximum optimization
lto = true          # Full link-time optimization
codegen-units = 1   # Single codegen unit (best optimization)
panic = "abort"     # No unwind tables — smaller binary
strip = true        # Strip all debug symbols
```

### Key Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| `serde` + `serde_json` | 1.0 | Serialization framework |
| `memmap2` | 0.9 | Cross-platform memory-mapped files |
| `sha2` | 0.10 | SHA-256 hashing (Merkle signatures) |
| `ed25519-dalek` | 2 | Ed25519 signatures for NDA documents |
| `axum` | 0.7 | HTTP/WebSocket framework (optional) |
| `tokio` | 1 | Async runtime (optional) |
| `tracing` + `tracing-subscriber` | 0.1 | Structured logging |
| `quick-xml` | 0.41 | Spec-compliant XML parsing (XLSX, DOCX) |
| `zip` | 2 | Archive handling (XLSX, DOCX, ZIP) |
| `toml` | 0.8 | Configuration file parsing |
| `thiserror` | 2 | Error type derivation |
| `chrono` | 0.4 | Date/time handling |

### Essential Commands

```bash
# Fast typecheck
cargo check

# Debug build
cargo build

# Release build (fully optimized)
cargo build --release

# Build with all features
cargo build --release --all-features

# Run with stdio mode
cargo run -- --mode stdio

# Run with HTTP mode
cargo run -- --mode http --addr 0.0.0.0:3000

# Run with config file
cargo run -- --config config.toml

# Run benchmarks
cargo run --release -- --benchmark

# Run all tests (284)
cargo test --all-features

# Format check
cargo fmt --all -- --check

# Lint
cargo clippy --all-features -- -W clippy::all

# Dependency audit
cargo audit
```

## Feature Flags

| Feature | Description | Key Dependencies |
|---------|-------------|------------------|
| `http` | HTTP/SSE/WebSocket transport | axum, tokio, tower, tower-http |
| `oauth2` | OAuth2 connector framework | ureq, aes-gcm, hmac |
| `database` | Database resource adapters | rusqlite |
| `observability` | OpenTelemetry tracing/metrics | opentelemetry, tracing-opentelemetry |

## Testing Procedures

### Test Suites (284 total)

```bash
# All tests
cargo test --all-features

# Unit tests only (210)
cargo test --lib

# Integration tests (43)
cargo test --test integration

# Fuzz tests (17, 3400+ random cases)
cargo test --test fuzz_tests

# Proc macro tests (8)
cargo test -p velocity_mcp_macros

# Criterion benchmarks
cargo bench
```

### Manual Testing

Test the stdio mode with a manual JSON-RPC request:

```bash
echo '{"jsonrpc":"2.0","method":"initialize","id":1}' | cargo run -- --mode stdio
```

Test tool listing:

```bash
echo '{"jsonrpc":"2.0","method":"tools/list","id":2}' | cargo run -- --mode stdio
```

Test HTTP mode:

```bash
# Start server in background
cargo run -- --mode http --addr 127.0.0.1:3000 &

# Health check
curl http://127.0.0.1:3000/health

# List tools
curl -X POST http://127.0.0.1:3000/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"tools/list","id":1}'
```

## Code Style and Conventions

### Formatting
- Use `cargo fmt` — all code must be formatted
- 4-space indentation (Rust default)

### Linting
- `cargo clippy --all-features -- -W clippy::all` — address all warnings
- Zero warnings policy for production code

### Naming
- `snake_case` for functions, variables, modules
- `PascalCase` for types, traits, enums
- `UPPER_SNAKE_CASE` for constants

### Error Handling
- Use `Result<T, String>` or `thiserror`-derived error types
- Use `tracing::error!` / `tracing::warn!` for diagnostic output
- Sanitize error messages before returning to clients (strip paths, truncate)
- Avoid `unwrap()` in production code — use `?` or explicit error handling

### Unsafe Code
- The NMCP binary parser uses `unsafe` for zero-copy slice-to-array pointer casts
- All `unsafe` blocks must have a clear justification comment
- Bounds checks must precede all `unsafe` pointer operations

## Adding New Features

### Adding a New Built-in Tool
1. Define the tool in `src/registry.rs` → `get_tools()` with name, description, and JSON Schema
2. Add the dispatch arm in `call_tool()` match block
3. Implement the logic natively in Rust
4. Add tests in the unit test module

### Adding a Tool via Proc Macro
```rust
#[mcp_tool(
    name = "my_tool",
    description = "Does something useful",
    param_constraints = {
        "path": { "min_length": 1 }
    }
)]
fn my_tool(path: String, offset: Option<i64>) -> Result<String, String> {
    // implementation
}
```
The tool is automatically registered in the global registry.

### Adding a New Transport Mode
1. Create a new file in `src/transport/` or `src/protocol/`
2. Add `pub mod` declaration in the module's `mod.rs`
3. Implement the mode's main loop function
4. Add the mode string to the `match mode` block in `src/main.rs`
5. Update `print_help()` with the new mode description

### Adding a New CLI Flag
1. Add the match arm in the argument parsing loop in `src/main.rs`
2. Handle the argument (with bounds checking for value args)
3. Update `print_help()` with the new option
4. Add to `ServerConfig` if it should be configurable via TOML

## High-Risk Areas

Changes in these areas require extra care:

1. **Shared Memory State Machine** (`src/ipc/shmem.rs`)
   - 5-state protocol with Acquire/Release ordering
   - Incorrect state transitions can deadlock host-server communication
   - `SeqCst` fences must be placed correctly between length and state operations

2. **Unsafe Binary Parsing** (`src/protocol/nmcp_binary.rs`, `src/protocol/nda_native.rs`)
   - Uses `unsafe` pointer casts for zero-copy slice-to-array references
   - Incorrect bounds checking can cause memory safety issues
   - Magic signature validation must be exact

3. **Sandbox Enforcement** (`src/sandbox.rs`, `src/sandbox/linux_seccomp.rs`)
   - Capability violations must be recorded and enforced
   - Seccomp filter must not accidentally allow dangerous syscalls
   - Job Object limits must be applied before process starts executing

4. **HTTP Security** (`src/transport/http.rs`, `src/middleware.rs`)
   - API key comparison must be timing-safe
   - CORS policy must be restrictive by default
   - Request size limits must be enforced before body parsing

5. **Buffer Size Limits** (`src/ipc/shmem.rs`)
   - Total buffer: 64KB
   - Input buffer: ~4KB, Output buffer: ~61KB
   - Exceeding limits returns errors but does not panic

**Section sources**
- [Cargo.toml](file://Cargo.toml)
- [src/main.rs](file://src/main.rs)
- [src/lib.rs](file://src/lib.rs)
