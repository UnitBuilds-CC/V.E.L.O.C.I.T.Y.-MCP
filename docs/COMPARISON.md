# Performance Comparison

Benchmark results comparing VELOCITY-MCP against the Node.js reference implementation across 8 pipeline combinations.

## Methodology

All benchmarks run on the same hardware with:
- **CPU:** Intel i5-14400F
- **OS:** Windows 11
- **Build:** Rust release profile (`opt-level = 3`, `lto = true`)
- **Node.js:** v24.18.0
- **Payload:** 64-byte tool arguments (unless noted)
- **Iterations:** 100 samples per benchmark via Criterion

## NDA/shmem Transport (Primary Path)

The highest-performance pipeline, using NDA binary encoding over shared memory IPC:

| Method | Latency | Throughput | vs JSON/stdio |
|--------|---------|------------|---------------|
| ping | 0.001 ms (1us) | 1,657,825 r/s | 34.3x faster |
| tools/list (16 tools) | 0.006 ms | 165,981 r/s | 29.7x faster |
| tools/call (64B) | 0.001 ms | 750,413 r/s | 18.3x faster |
| health/check | 0.000 ms | 2,190,101 r/s | 46.3x faster |

**Overall: 27.7x faster average, 40.8x faster at p99** (NDA/shmem vs JSON/stdio)

## Node.js vs Rust (Fair Comparison)

Same transport (stdio), same encoding (JSON-RPC) — isolates the runtime difference:

| Method | Node.js avg | Rust avg | Speedup |
|--------|------------|----------|---------|
| ping | 0.061 ms | 0.034 ms | 1.8x |
| tools/list | 0.075 ms | 0.128 ms | 0.6x* |
| tools/call | 0.039 ms | 0.018 ms | 2.2x |
| health/check | 0.040 ms | 0.038 ms | 1.0x |

*tools/list: Node.js returns a static array; Rust dynamically assembles from 5 sources (built-in tools, plugins, NDA tools, proc macros, database). Rust wins at p99.

**Overall: 1.0x avg (tied), 1.7x p99** (Rust wins on tail latency)

## 4-Pipeline Comparison

| Pipeline | Ping avg | tools/list avg | tools/call avg |
|----------|----------|----------------|----------------|
| Node.js JSON/stdio | 0.046 ms | 0.110 ms | 0.042 ms |
| Rust JSON/stdio | 0.035 ms | 0.195 ms | 0.034 ms |
| Rust NDA-wrapped JSON/stdio | 0.027 ms | 0.186 ms | 0.035 ms |
| Rust NDA/shmem | 0.001 ms | 0.006 ms | 0.002 ms |

**Key finding:** Transport is the dominant factor. Shared memory is an order of magnitude faster than stdio. Encoding format (JSON vs NDA) has negligible impact when transport is the same.

## Phase Timing

All 8 pipelines instrument write/wait/read phases. The "wait" phase isolates server turnaround time:

| Pipeline | write | wait | read | Total |
|----------|-------|------|------|-------|
| NDA/shmem | 0.0us | 0.5us | 0.1us | ~1us |
| JSON/shmem | 0.3us | 6.3us | 0.3us | ~7us |

The 12x difference in "wait" phase (0.5us vs 6.3us) shows the JSON parse+stringify cost on the server side. With NDA encoding, the server reads binary directly — no parsing needed.

## Tail Latency (p99)

| Method | Rust p99 | Node.js p99 | Improvement |
|--------|----------|-------------|-------------|
| tools/call | 0.2ms | 5.0ms | 25x |
| ping | 0.1ms | 1.2ms | 12x |
| tools/list | 0.3ms | 3.5ms | 11.7x |

Tail latency matters most in production — this is what users feel when the system is under load.

## Scaling

Concurrent dispatch throughput (flat binary, shared memory):

| Threads | Throughput |
|---------|------------|
| 1 | 1.2M req/s |
| 2 | 2.4M req/s |
| 4 | 4.1M req/s |
| 8 | 5.3M req/s |

Near-linear scaling to 8 threads, limited by memory bandwidth rather than CPU.

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
