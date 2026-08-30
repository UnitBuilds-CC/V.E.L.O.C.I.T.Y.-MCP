# VELOCITY-MCP Benchmarks

This directory contains Criterion-based benchmarks for measuring VELOCITY-MCP performance.

## Running Benchmarks

### Run all benchmarks
```bash
cargo bench
```

### Run specific benchmark suite
```bash
# Protocol benchmarks (JSON-RPC operations)
cargo bench --bench protocol_bench

# NDA benchmarks (binary document operations)
cargo bench --bench nda_bench
```

### Run specific benchmark function
```bash
cargo bench --bench protocol_bench -- json_rpc_initialize
cargo bench --bench nda_bench -- nda_compile_with_triples
```

### Compare with baseline
```bash
# Save current results as baseline
cargo bench -- --save-baseline current

# Make changes, then compare
cargo bench -- --baseline current
```

## Benchmark Suites

### protocol_bench

Measures JSON-RPC protocol operation performance:

- `json_rpc_initialize` - Initialize handshake
- `json_rpc_tools_list` - List available tools
- `json_rpc_ping` - Ping/pong latency
- `json_rpc_health_check` - Health check endpoint

### nda_bench

Measures NDA (Neural Document Archive) binary format performance:

- `nda_compile_empty` - Compile empty document
- `nda_compile_with_triples` - Compile document with semantic triples
- `nda_read` - Parse compiled NDA document
- `nda_verify_merkle` - Verify Merkle tree integrity

## Understanding Results

Criterion generates detailed statistical reports including:

- **Mean time**: Average execution time
- **Median time**: Middle value (less affected by outliers)
- **Standard deviation**: Variability in measurements
- **Confidence intervals**: 95% confidence bounds
- **Outlier detection**: Identifies anomalous measurements

Results are saved in `target/criterion/` with HTML reports for easy visualization.

## Performance Targets

Based on our Node.js comparison benchmarks:

| Operation | Target | Node.js Equivalent | Speedup |
|-----------|--------|-------------------|---------|
| JSON-RPC initialize | <0.2ms | ~0.6ms | 3x |
| JSON-RPC tools/list | <0.2ms | ~0.6ms | 3x |
| NDA compile (3 triples) | <0.1ms | N/A | Native only |
| NDA read | <0.05ms | N/A | Native only |

## Continuous Benchmarking

To track performance over time:

1. Run benchmarks in CI on every PR
2. Compare against main branch baseline
3. Flag regressions >10%
4. Track improvements in release notes

Example CI integration:
```yaml
- name: Run benchmarks
  run: cargo bench -- --save-baseline pr
  
- name: Compare with main
  run: cargo bench -- --baseline main
```

## Profiling

For detailed profiling, use:

```bash
# CPU profiling with perf (Linux)
perf record cargo bench --bench protocol_bench
perf report

# Memory profiling with valgrind
valgrind --tool=massif cargo bench --bench nda_bench

# Flame graphs
cargo flamegraph --bench protocol_bench
```

## Adding New Benchmarks

1. Create a new file in `benches/` directory
2. Add `[[bench]]` entry to `Cargo.toml`
3. Use Criterion's benchmarking API
4. Include setup/teardown in the benchmark function

Example:
```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn my_benchmark(c: &mut Criterion) {
    c.bench_function("my_operation", |b| {
        b.iter(|| {
            // Setup (not measured)
            let data = setup();
            
            // Operation (measured)
            black_box(operation(&data))
        })
    });
}

criterion_group!(benches, my_benchmark);
criterion_main!(benches);
```

## Tips for Accurate Benchmarks

1. **Close other applications** - Reduce system noise
2. **Disable CPU frequency scaling** - Use performance governor
3. **Run multiple iterations** - Criterion handles this automatically
4. **Warm up the CPU** - Run benchmarks twice, use second run
5. **Check for outliers** - Review Criterion's outlier detection

## Troubleshooting

### Benchmarks are slow
- Close other applications
- Check CPU thermal throttling
- Disable power saving modes

### Inconsistent results
- Increase sample size: `cargo bench -- --sample-size 200`
- Check for background processes
- Ensure stable CPU frequency

### Gnuplot not found
- Criterion falls back to plotters backend
- Install gnuplot for better plots: `apt-get install gnuplot` (Linux)

## Resources

- [Criterion User Guide](https://bheisler.github.io/criterion.rs/book/)
- [Rust Performance Book](https://nnethercote.github.io/perf-book/)
- [Criterion API Docs](https://docs.rs/criterion/)
