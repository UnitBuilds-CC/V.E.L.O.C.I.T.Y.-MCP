# Rust Cargo Single Crate with Release Optimization

## Classification
- **Category**: Build System
- **Files**: Cargo.toml, Cargo.lock
- **Criticality**: High — build infrastructure and optimization profile

## Summary

The V.E.L.O.C.I.T.Y. NMCP Server is a single Rust binary crate (not a workspace). It uses an aggressively optimized release profile designed for maximum runtime performance at the cost of compile time.

## Dependencies

| Crate | Version | Features | Purpose |
|-------|---------|----------|---------|
| `serde` | 1.0 | `derive` | Serialization framework with derive macros |
| `serde_json` | 1.0 | — | JSON parsing and generation |
| `memmap2` | 0.9 | — | Cross-platform memory-mapped files |
| `sha2` | 0.10 | — | SHA-256 hashing for Merkle signatures |
| `once_cell` | 1.18 | — | Lazy static initialization |

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

```powershell
cargo check              # Fast typecheck (no codegen)
cargo build              # Debug build
cargo build --release    # Release build (fully optimized)
cargo run -- --benchmark # Run benchmarks in debug mode
cargo run --release -- --mode stdio  # Run optimized server
```

## Key Constraints

- Single binary output — no dynamic library dependencies
- Release builds are slow due to `lto = true` + `codegen-units = 1`
- Debug builds are fast — use for development iteration
- `cargo clean` recommended before release builds to avoid stale artifacts
