# Performance Comparison

Benchmark results comparing VELOCITY-MCP against the Node.js reference implementation across 8 pipeline combinations.

## Methodology

All benchmarks run on the same hardware with:
- **CPU:** Intel Core 5 210H (8 cores / 12 logical processors)
- **OS:** Windows 11
- **Build:** Rust release profile (`opt-level = 3`, `lto = true`)
- **Node.js:** v24.19.0
- **Payload:** 64-byte tool arguments (unless noted)
- **Harness:** `bench_nda` — 500 iterations × 3 rounds, median kept, per pipeline
- **Measured:** 2026-09-17 — raw capture `bench_nda_2026-09-17.txt`, written by the `bench_nda` harness named above, **not** by `velocity_mcp --benchmark`. The producer is identified by its own output rather than a line number into code: `bench_nda` is the only harness in the tree that prints the row label `Warm throughput` (one occurrence in `bench_nda/src/main.rs`, zero in `src/benchmark.rs`; four in this capture), and it prints the banner `NDA-Native Benchmark: Binary-over-Shmem vs JSON-RPC-over-Stdio`. Captures taken before 2026-10-09 additionally carry a stale `VELOCITY-MCP v3.0.0` stamp, since corrected in the harness, so that stamp dates a capture instead of naming its author. This capture is a local artifact: raw benchmark output is not committed to the repo, so the tables in this file are the durable record and [Reproducing](#reproducing) is how to regenerate them.

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

Concurrent **parse** throughput — `velocity_mcp --benchmark` phase 7, 10 000 requests per thread, measured 2026-10-09 on this box (`target/release/velocity_mcp.exe`, release build):

| Threads | JSON-RPC parse | NDA frame parse | NDA vs JSON |
|---------|----------------|-----------------|-------------|
| 1 | 1.64M req/s | 2.37M req/s | 1.44x |
| 2 | 2.26M req/s | 3.58M req/s | 1.58x |
| 4 | 3.50M req/s | 6.19M req/s | 1.77x |
| 8 | 5.12M req/s | 9.51M req/s | 1.86x |

What this phase actually measures, because the label matters: each thread parses its own requests in-process — `serde_json::from_str` on the JSON-RPC side, `nda_native::parse_nda_request` on one pre-built binary frame on the NDA side (`src/benchmark.rs:451-525`). **No transport, no shared memory, no server** is involved, so these are encoding-and-parse scaling numbers, not end-to-end pipeline ones; the pipeline tables above are a different harness (`bench_nda`). The shmem primitives themselves are a separate phase of the same run: NDA write+read 13.1 ns/op (76.4M ops/s) vs JSON 38.7 ns/op (25.8M ops/s) over 200 000 iterations.

Scaling is sub-linear from the first doubling — 2 threads give 1.51x of 1 thread on the NDA arm, and 8 threads give 4.01x — which this run can attribute no further than "not linear on an 8-core / 12-logical-thread box"; the earlier note here blamed memory bandwidth without measuring it.

The four figures this section replaced (1.7M / 2.8M / 4.2M / 5.6M req/s, dated 2026-09-17, described as "NDA binary, shared memory") match no capture retained on this machine, and the "shared memory" description was wrong for the phase that prints them.

## Reproducing

```bash
# Build release (the HTTP/database/oauth2 features are what the tables above use)
cargo build --release --features http,database,oauth2

# Built-in micro-benchmark suite (source of the Scaling table)
target/release/velocity_mcp --benchmark

# NDA E2E transport harness (source of the pipeline tables)
# bench_nda/ declares its own [workspace], so `cargo run --bin bench_nda` from the
# repo root does not resolve it — point cargo at its manifest and stay in the root:
cargo run --release --manifest-path bench_nda/Cargo.toml
```

Run that from the repository root: the harness resolves both peers against CWD (`target/release/velocity_mcp.exe`, `bench_nodejs/server.js`). The Node.js side is not a separate benchmark — `bench_nda` spawns `bench_nodejs/server.js` and drives both through identical requests, which is what makes the "same transport, same encoding" row fair. `bench_nodejs/benchmark.js` is its own deprecated predecessor ("superseded by bench_nda (unified 7-pipeline benchmark)") and is kept for reference only.

All benchmark code is in `benches/` (9 custom `harness = false` targets — several load the WASM modules built by `bench_tools/`, so `cargo bench` as a whole needs that toolchain set) and `bench_nda/` (custom E2E transport harness, its own workspace).
