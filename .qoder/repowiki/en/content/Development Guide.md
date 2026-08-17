# Development Guide

<cite>
**Referenced Files in This Document**
- [Cargo.toml](file://Cargo.toml)
- [src/main.rs](file://src/main.rs)
- [src/protocol/mod.rs](file://src/protocol/mod.rs)
- [src/ipc/mod.rs](file://src/ipc/mod.rs)
</cite>

## Table of Contents
1. [Introduction](#introduction)
2. [Crate Organization](#crate-organization)
3. [Build System and Toolchain](#build-system-and-toolchain)
4. [Testing Procedures](#testing-procedures)
5. [Code Style and Conventions](#code-style-and-conventions)
6. [Adding New Features](#adding-new-features)
7. [High-Risk Areas](#high-risk-areas)

## Introduction

This guide covers development conventions, build workflows, and code organization for the V.E.L.O.C.I.T.Y. NMCP Server. The project is a single Rust crate with ~570 LOC across 7 source files, organized into protocol handlers, IPC subsystem, tool registry, and benchmarks.

## Crate Organization

The project is a single binary crate (not a workspace):

| Module | LOC | Purpose |
|--------|-----|---------|
| `src/main.rs` | 92 | CLI argument parsing, mode dispatch |
| `src/protocol/json_rpc.rs` | 113 | Stdio JSON-RPC v2.0 request loop |
| `src/protocol/nmcp_binary.rs` | 141 | Shared memory polling loop + zero-alloc binary frame parser |
| `src/ipc/shmem.rs` | 117 | Memory-mapped file buffer with 5-state machine |
| `src/registry.rs` | 136 | Tool definitions + C# NdaMcpServer delegation |
| `src/benchmark.rs` | 74 | Performance micro-benchmarks |
| `src/protocol/mod.rs` + `src/ipc/mod.rs` | 5 | Module declarations |

### Module Dependencies

```
main.rs
├── protocol::json_rpc ──→ registry
├── protocol::nmcp_binary ──→ ipc::shmem, registry
├── registry (standalone)
└── benchmark ──→ protocol::nmcp_binary, ipc::shmem
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

### Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| `serde` | 1.0 (with `derive`) | Serialization framework |
| `serde_json` | 1.0 | JSON parsing and generation |
| `memmap2` | 0.9 | Cross-platform memory-mapped files |
| `sha2` | 0.10 | SHA-256 hashing (Merkle signatures) |
| `once_cell` | 1.18 | Lazy static initialization |

### Essential Commands

```powershell
# Fast typecheck
cargo check

# Debug build
cargo build

# Release build (fully optimized)
cargo build --release

# Run with stdio mode
cargo run -- --mode stdio

# Run with shared memory mode
cargo run -- --mode shmem --buffer-path nmcp_buffer.bin

# Run benchmarks
cargo run -- --benchmark

# Format check
cargo fmt --all -- --check

# Lint
cargo clippy -- -D warnings
```

## Testing Procedures

### Built-in Benchmarks

The project includes a built-in benchmark suite (not `criterion`-based, uses `std::time::Instant`):

```powershell
cargo run -- --benchmark
```

Benchmarks use `std::hint::black_box()` to prevent the compiler from optimizing away benchmark iterations.

### Manual Testing

Test the stdio mode with a manual JSON-RPC request:

```powershell
echo '{"jsonrpc":"2.0","method":"initialize","id":1}' | cargo run -- --mode stdio
```

Test tool listing:

```powershell
echo '{"jsonrpc":"2.0","method":"tools/list","id":2}' | cargo run -- --mode stdio
```

### Test Quality Standards

- Use `black_box()` for all benchmark iterations to prevent dead code elimination
- Verify round-trip consistency: write to shared memory → read back → compare
- Test error paths: invalid JSON, missing tool names, buffer overflow attempts

## Code Style and Conventions

### Formatting
- Use `cargo fmt` — all code must be formatted
- 4-space indentation (Rust default)

### Linting
- `cargo clippy -- -D warnings` — zero warnings policy
- Address all clippy suggestions before committing

### Naming
- `snake_case` for functions, variables, modules
- `PascalCase` for types, traits, enums
- `UPPER_SNAKE_CASE` for constants (e.g., `STATE_IDLE`, `TOTAL_BUFFER_SIZE`)

### Error Handling
- Use `Result<T, Box<dyn Error>>` for fallible operations
- Use `eprintln!` for diagnostic output to stderr
- Use `process::exit(1)` for fatal CLI errors

### Unsafe Code
- The NMCP binary parser uses `unsafe` for zero-copy slice-to-array pointer casts
- All `unsafe` blocks must have a clear justification comment
- Prefer safe alternatives where performance is not critical

## Adding New Features

### Adding a New Tool
1. Define the tool in `src/registry.rs` → `get_tools()` with name, description, and JSON Schema
2. Add the dispatch arm in `call_tool()` match block
3. If delegating to C# core, add the tool name to the `execute_csharp_mcp_tool()` call
4. If implementing natively, add the logic directly in the match arm

### Adding a New Protocol Mode
1. Create a new file in `src/protocol/`
2. Add `pub mod` declaration in `src/protocol/mod.rs`
3. Implement the mode's main loop function
4. Add the mode string to the `match mode` block in `src/main.rs`
5. Update `print_help()` with the new mode description

### Adding a New CLI Flag
1. Add the match arm in the `while` loop in `src/main.rs`
2. Handle the argument (with bounds checking for value args)
3. Update `print_help()` with the new option

## High-Risk Areas

Changes in these areas require extra care:

1. **Shared Memory State Machine** (`src/ipc/shmem.rs`)
   - 5-state atomic protocol (Idle → ReqReady → Processing → ResReady → Error)
   - Incorrect state transitions can deadlock the host-server communication
   - Buffer offset calculations must be exact (input at offset 10, output at offset 4096)

2. **Unsafe Binary Parsing** (`src/protocol/nmcp_binary.rs`)
   - Uses `unsafe` pointer casts for zero-copy slice-to-array references
   - Incorrect bounds checking (minimum 36 bytes for header) can cause memory safety issues
   - Magic signature validation (`NMCP`) must be exact

3. **C# Delegation Path** (`src/registry.rs`)
   - Hardcoded absolute path to C# NdaMcpServer.exe
   - Process spawn with stdin/stdout piping — errors in JSON-RPC envelope format break tool execution
   - Response parsing assumes specific JSON structure (`result.content[0].text`)

4. **Buffer Size Limits** (`src/ipc/shmem.rs`)
   - Total buffer: 64KB (`TOTAL_BUFFER_SIZE = 65536`)
   - Input buffer: ~4KB (offset 10 to 4096)
   - Output buffer: ~61KB (offset 4096 to 65536)
   - Exceeding these limits returns errors but does not panic

**Section sources**
- [Cargo.toml](file://Cargo.toml)
- [src/main.rs](file://src/main.rs)
- [src/protocol/mod.rs](file://src/protocol/mod.rs)
