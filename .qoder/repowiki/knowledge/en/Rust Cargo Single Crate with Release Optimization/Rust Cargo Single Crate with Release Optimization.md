# Rust Cargo Single Crate with Release Optimization

## Classification
- **Category**: Build System
- **Files**: Cargo.toml, Cargo.lock
- **Criticality**: High — build infrastructure and optimization profile

## Summary

The V.E.L.O.C.I.T.Y. MCP Server v3.0.0 is a single Rust binary crate with a proc-macro sub-crate (`macros/`). It uses an aggressively optimized release profile designed for maximum runtime performance at the cost of compile time. The project has 4 feature flags for optional functionality.

## Dependencies

| Crate | Version | Features | Purpose |
|-------|---------|----------|---------|
| `serde` | 1.0 | `derive` | Serialization framework with derive macros |
| `serde_json` | 1.0 | — | JSON parsing and generation |
| `memmap2` | 0.9 | — | Cross-platform memory-mapped files |
| `sha2` | 0.10 | — | SHA-256 hashing for Merkle signatures |
| `ed25519-dalek` | 2 | `rand_core` | Ed25519 signatures for NDA documents |
| `tracing` | 0.1 | — | Structured logging |
| `tracing-subscriber` | 0.3 | `env-filter` | Log filtering and formatting |
| `quick-xml` | 0.41 | — | Spec-compliant XML parsing (XLSX, DOCX) |
| `zip` | 2 | `deflate` | Archive handling |
| `toml` | 0.8 | — | Configuration file parsing |
| `thiserror` | 2 | — | Error type derivation |
| `chrono` | 0.4 | `serde` | Date/time handling |
| `regex` | 1 | — | Pattern matching |
| `axum` | 0.7 | `ws` | HTTP/WebSocket framework (optional) |
| `tokio` | 1 | `full` | Async runtime (optional) |

## Feature Flags

| Feature | Description | Key Dependencies |
|---------|-------------|------------------|
| `http` | HTTP/SSE/WebSocket transport | axum, tokio, tower, tower-http, uuid |
| `oauth2` | OAuth2 connector framework | ureq, aes-gcm, hmac |
| `database` | Database resource adapters | rusqlite |
| `observability` | OpenTelemetry tracing/metrics | opentelemetry, tracing-opentelemetry |

## Release Profile

```toml
[profile.release]
opt-level = 3       # Maximum optimization (speed)
lto = true          # Full link-time optimization across all crates
codegen-units = 1   # Single codegen unit — best optimization, slowest compile
panic = "abort"     # No unwind tables — smaller binary, no catch_unwind
strip = true        # Strip all debug symbols from output
```

## Build Commands

```bash
cargo check                          # Fast typecheck (no codegen)
cargo build                          # Debug build
cargo build --release                # Release build (fully optimized)
cargo build --release --all-features # Release with all features
cargo test --all-features            # Run all 284 tests
cargo bench                          # Criterion benchmarks
cargo audit                          # Dependency vulnerability check
cargo run --release -- --benchmark   # Run built-in benchmarks
cargo run --release -- --mode http   # Run optimized HTTP server
```

## Test Suites (284 total)

| Suite | Tests | Description |
|-------|:-----:|-------------|
| Unit tests | 210 | Parser, sandbox, signatures, protocol, MCP compliance |
| Integration tests | 43 | Full pipeline, path validation, 15 adversarial tests |
| Property-based fuzz | 17 | 3,400+ random cases via proptest |
| Proc macro tests | 8 | Type-safe registration, constraints |
| Doc tests | 6 | Observability module examples |

## Key Constraints

- Single binary output — no dynamic library dependencies
- Release builds are slow due to `lto = true` + `codegen-units = 1`
- Debug builds are fast — use for development iteration
- `cargo clean` recommended before release builds to avoid stale artifacts
- Cross-platform: Windows, Linux, macOS
- `--all-features` required to run the full test suite
