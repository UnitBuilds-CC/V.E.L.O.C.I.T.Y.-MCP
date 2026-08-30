# Built-in Micro-Benchmark Suite

## Classification
- **Category**: Performance / Testing
- **Files**: src/benchmark.rs, benches/protocol_bench.rs, benches/nda_bench.rs
- **Criticality**: Medium — development tool for performance validation

## Summary

The project includes two benchmarking systems:
1. **Built-in micro-benchmark suite** (`--benchmark` CLI flag) — 8 sections covering protocol parsing, shared memory throughput, concurrent scaling, and Rust vs Node.js comparison
2. **Criterion benchmark suite** (`cargo bench`) — statistical benchmarks with HTML reports

## Built-in Benchmark Sections

| Section | Description |
|---------|-------------|
| JSON-RPC Parsing | serde_json parse latency |
| NDA-Native Parsing | Binary frame parse + SHA-256 Merkle verify |
| Protocol Overhead | Same tool call: JSON vs NDA-native (full extraction) |
| TLV Encoding | Binary encode/decode vs JSON parse |
| Shmem Throughput (JSON) | JSON string write+read in shared memory |
| Shmem Throughput (NDA) | Raw binary write+read in shared memory |
| Concurrent Dispatch | Multi-threaded (1-8 threads) |
| Rust vs Node.js | Head-to-head stdio comparison |

## Reference Results (Intel Core i5-14400F)

**Protocol Parsing:**

| Operation | Latency | Throughput |
|-----------|:-------:|:----------:|
| JSON-RPC parse + extract | 722.6 ns | ~1.38M req/s |
| NDA-native parse + Merkle + extract | 459.1 ns | ~2.18M req/s |
| **NDA speedup** | | **1.6x** |

**Shared Memory:**

| Operation | Latency | Throughput |
|-----------|:-------:|:----------:|
| JSON-in-shmem R/W | 38.8 ns | 25.8M ops/s |
| NDA-native shmem R/W | 12.6 ns | 79.2M ops/s |
| **NDA speedup** | | **3.1x** |

**Rust vs Node.js (stdio, 200 req/method):**

| Method | Node.js | Rust | Speedup |
|--------|:-------:|:----:|:-------:|
| **Overall** | 0.627 ms | 0.164 ms | **3.8x** |

## Methodology

- `black_box()` prevents dead code elimination
- `Instant::now()` + `elapsed()` for timing (built-in)
- Criterion provides statistical analysis with HTML reports
- Reference hardware: Intel Core i5-14400F

## Invocation

```bash
# Built-in benchmarks
cargo run --release -- --benchmark

# Criterion benchmarks
cargo bench

# Criterion with HTML report
cargo bench -- --html
```
