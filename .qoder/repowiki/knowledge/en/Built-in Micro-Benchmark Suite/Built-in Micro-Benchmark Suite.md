# Built-in Micro-Benchmark Suite

## Classification
- **Category**: Performance / Testing
- **Files**: src/benchmark.rs (74 LOC)
- **Criticality**: Medium — development tool for performance validation

## Summary

The benchmark module provides a built-in micro-benchmark suite that compares three operations: JSON-RPC parsing (serde_json), zero-allocation binary frame parsing (NMCP), and shared memory mapped R/W. Uses `std::time::Instant` and `std::hint::black_box()` — no external benchmark framework.

## Benchmarks

| Benchmark | Iterations | Reference Latency | Speedup |
|-----------|:----------:|:-----------------:|:-------:|
| JSON-RPC Parse (serde_json) | 500,000 | ~112.59 ns | 1.0x (baseline) |
| Mmapped Buffer R/W | 200,000 | ~74.05 ns | 1.52x |
| Zero-Alloc Binary Parse | 1,000,000 | ~1.54 ns | 73.1x |

## Methodology

- `black_box()` prevents dead code elimination
- `Instant::now()` + `elapsed()` for timing
- Mean latency computed as `total_nanos / iterations`
- Shmem benchmark creates temp file, cleans up after
- Reference hardware: Intel Core i5-14400F

## Invocation

```powershell
cargo run -- --benchmark
```
