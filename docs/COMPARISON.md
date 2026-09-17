# Performance Comparison

Benchmark results comparing VELOCITY-MCP against the Node.js reference implementation across 8 pipeline combinations.

## Methodology

All benchmarks run on the same hardware with:
- **CPU:** Intel Core 5 210H (12 cores)
- **OS:** Windows 11
- **Build:** Rust release profile (`opt-level = 3`, `lto = true`)
- **Node.js:** v24.19.0
- **Payload:** 64-byte tool arguments (unless noted)
- **Harness:** `bench_nda` — 500 iterations × 3 rounds, median kept, per pipeline
- **Measured:** 2026-09-17 (raw: `bench_nda_2026-09-17.txt`, `velocity_mcp --benchmark`)

## NDA/shmem Transport (Primary Path)

The highest-performance pipeline, using NDA binary encoding over shared memory IPC (warm p50, 2026-09-17, Core 5 210H):

| Method | Latency | Throughput | vs JSON/stdio (avg) |
|--------|---------|------------|---------------|
| ping | 0.001 ms | 1,761,585 r/s | 82.0x faster |
| tools/list (40 tools) | 0.003 ms | 284,311 r/s | 249.7x faster |
| tools/call (64B) | 0.001 ms | 511,819 r/s | 36.2x faster |

**Speedups vs JSON/stdio:** ping 82.0x avg / 138.1x p99; tools/list 249.7x avg / 269.8x p99; tools/call 36.2x avg / 21.6x p99.

## Node.js vs Rust (Fair Comparison)

Same transport (stdio), same encoding (JSON-RPC) — isolates the runtime difference (warm p50, 2026-09-17):

| Method | Rust (JSON/stdio) | Node.js (stdio) | Result |
|--------|-------------------|-----------------|--------|
| ping | 0.044 ms | 0.098 ms | Rust 2.2x |
| tools/list | 0.807 ms | 0.175 ms | Node 4.6x |
| tools/call | 0.061 ms | 0.083 ms | Rust 1.4x |

On this run Rust wins ping and tools/call; **Node wins tools/list on both avg and p99** (0.551 ms vs 2.320 ms p99) — Rust's `get_tools()` dynamically assembles 40 tools from 5 registries while the Node stub returns a static array. Raw: `bench_nda_2026-09-17.txt`.

## 4-Pipeline Comparison

| Pipeline | Ping avg | tools/list avg | tools/call avg |
|----------|----------|----------------|----------------|
| Node.js JSON/stdio | 0.149 ms | 0.197 ms | 0.088 ms |
| Rust JSON/stdio | 0.047 ms | 0.878 ms | 0.071 ms |
| Rust NDA/stdio | 0.038 ms | 0.204 ms | 0.043 ms |
| Rust NDA/shmem | 0.001 ms | 0.004 ms | 0.002 ms |

**Key finding:** Transport is the dominant factor. Shared memory is an order of magnitude faster than stdio. Encoding format (JSON vs NDA) has negligible impact when transport is the same.

## Scaling

Concurrent dispatch throughput (NDA binary, shared memory; `velocity_mcp --benchmark`, 2026-09-17):

| Threads | Throughput |
|---------|------------|
| 1 | 1.7M req/s |
| 2 | 2.8M req/s |
| 4 | 4.2M req/s |
| 8 | 5.6M req/s |

Scales sub-linearly past 2 threads on this 12-core box (memory-bandwidth bound). Raw: `velocity_mcp --benchmark`.

## Reproducing

```bash
# Build release
cargo build --release

# Run benchmarks
cargo bench

# Run NDA-specific benchmark harness
cargo run --release --bin bench_nda

# Run comparison with Node.js
node benchmark.js
```

All benchmark code is in `benches/` (Criterion) and `bench_nda/` (custom E2E harness).
