# Built-in Micro-Benchmark Suite

<cite>
**Referenced Files in This Document**
- [src/benchmark.rs](file://src/benchmark.rs)
- [src/protocol/nmcp_binary.rs](file://src/protocol/nmcp_binary.rs)
- [src/ipc/shmem.rs](file://src/ipc/shmem.rs)
- [benches/protocol_bench.rs](file://benches/protocol_bench.rs)
- [benches/nda_bench.rs](file://benches/nda_bench.rs)
</cite>

## Overview

The project includes two benchmarking systems:
1. **Built-in micro-benchmark suite** (`--benchmark` CLI flag) — in-process benchmarks using `std::time::Instant`
2. **Criterion benchmark suite** (`cargo bench`) — statistical benchmarks in the `benches/` directory with HTML reports

## Built-in Benchmark Suite

Invoked via `--benchmark` CLI flag. Covers 8 sections:

### Benchmark Sections

| Section | Description |
|---------|-------------|
| JSON-RPC Parsing | serde_json parse latency for standard MCP requests |
| NDA-Native Parsing | Binary frame parse + SHA-256 Merkle verify |
| Protocol Overhead | Same tool call: JSON vs NDA-native (full field extraction) |
| TLV Encoding | Binary encode/decode vs JSON parse |
| Shmem Throughput (JSON) | JSON string write+read in shared memory |
| Shmem Throughput (NDA) | Raw binary write+read in shared memory |
| Concurrent Dispatch | Multi-threaded JSON vs NDA-native (1-8 threads) |
| Rust vs Node.js | Head-to-head stdio comparison |

### Reference Results (Intel Core i5-14400F, release build)

**Protocol Parsing (apples-to-apples, same tool call, full field extraction):**

| Operation | Mean Latency | Throughput |
|-----------|:---:|:---:|
| JSON-RPC parse + extract | 722.6 ns | ~1.38M req/s |
| NDA-native parse + Merkle + extract | 459.1 ns | ~2.18M req/s |
| **NDA speedup** | | **1.6x faster** |

**Shared Memory Throughput:**

| Operation | Mean Latency | Throughput |
|-----------|:---:|:---:|
| JSON-in-shmem R/W | 38.8 ns | 25.8M ops/s |
| NDA-native shmem R/W | 12.6 ns | 79.2M ops/s |
| **NDA speedup** | | **3.1x faster** |

**Concurrency (NDA-native dispatch):**

| Threads | Throughput |
|:-------:|:----------:|
| 1 | 2.86M req/s |
| 4 | 7.01M req/s |
| 8 | 12.7M req/s |

**Rust vs Node.js MCP Server (stdio, 200 req/method):**

| Method | Node.js avg | Rust avg | Speedup |
|--------|:---:|:---:|:---:|
| ping | 0.573 ms | 0.157 ms | **3.6x** |
| tools/list | 1.050 ms | 0.154 ms | **6.8x** |
| tools/call | 0.546 ms | 0.136 ms | **4.0x** |
| **Overall** | 0.627 ms | 0.164 ms | **3.8x** |

## Criterion Benchmark Suite

Located in `benches/` directory with two benchmark files:

### `protocol_bench.rs`
Benchmarks protocol parsing performance:
- JSON-RPC parsing with serde_json
- NDA-native binary frame parsing
- TLV encoding/decoding
- Shared memory read/write cycles

### `nda_bench.rs`
Benchmarks NDA operations:
- NDA document compilation
- NDA document parsing with Merkle verification
- Ed25519 signature generation and verification
- String pool operations

### Running Criterion Benchmarks

```bash
# All benchmarks
cargo bench

# Specific benchmark
cargo bench --bench protocol_bench

# With HTML report
cargo bench -- --html
```

Criterion provides:
- Statistical analysis (mean, median, standard deviation)
- Confidence intervals
- Outlier detection
- HTML reports with detailed charts
- Regression detection

## Benchmark Methodology

### `std::hint::black_box()`

All benchmark inputs and outputs are wrapped in `black_box()` to prevent the compiler from:
- Optimizing away the loop body (dead code elimination)
- Hoisting invariants out of the loop
- Pre-computing results at compile time

### Timing

Built-in benchmarks use `std::time::Instant` for high-resolution timing:
```rust
let start = Instant::now();
for _ in 0..iterations { /* benchmark body */ }
let duration = start.elapsed();
let avg_ns = (duration.as_nanos() as f64) / (iterations as f64);
```

Criterion benchmarks use its built-in statistical measurement with warmup iterations.

### Reference Hardware

Results are calibrated on Intel Core i5-14400F. Actual numbers vary by CPU, memory speed, and system load.

## Key Design Decisions

1. **Two benchmark systems**: Built-in for quick checks during development, Criterion for rigorous statistical analysis.
2. **Node.js comparison**: The built-in suite includes a head-to-head comparison with the Node.js reference implementation to quantify the performance advantage.
3. **Concurrent scaling**: Multi-threaded benchmarks (1-8 threads) demonstrate the server's ability to scale with CPU cores.
4. **Apples-to-apples**: Protocol overhead benchmark measures the same tool call with full field extraction in both JSON and NDA-native formats.
5. **No external dependencies for built-in**: Uses `std::time::Instant` instead of external frameworks for the built-in suite, keeping the binary self-contained.

**Section sources**
- [src/benchmark.rs](file://src/benchmark.rs)
- [benches/protocol_bench.rs](file://benches/protocol_bench.rs)
- [benches/nda_bench.rs](file://benches/nda_bench.rs)
