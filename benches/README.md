# VELOCITY-MCP Benchmarks

`Cargo.toml` declares nine benchmark targets, all of them with `harness = false`. Three drive
Criterion: `protocol_bench`, `nda_bench` and `binary_protocol_bench`. The other six are custom
`fn main()` reporters that print their own tables and never touch Criterion: `wasm_isolation`,
`wasm_vs_native`, `wasm_tool_latency`, `comprehensive_benchmark`, `metering_cache_bench` and
`tool_compatibility_test`.

## Running Benchmarks

### Run all benchmarks
```bash
cargo bench
```

### Run specific benchmark suite
```bash
# Criterion suites
cargo bench --bench protocol_bench          # JSON-RPC handler dispatch
cargo bench --bench nda_bench               # NDA binary document operations
cargo bench --bench binary_protocol_bench   # TLV codec vs JSON codec

# Custom-reporter suites
cargo bench --bench wasm_isolation
cargo bench --bench wasm_vs_native
cargo bench --bench wasm_tool_latency
cargo bench --bench comprehensive_benchmark
cargo bench --bench metering_cache_bench
cargo bench --bench tool_compatibility_test
```

`nda_bench` is the one suite that touches a feature-gated module, and it is written so both
profiles compile: `middleware_cache_lookup` uses `velocity_mcp::middleware` under
`--features http`, and `Cargo.toml` ships `default = []`, so the plain `cargo bench` build
takes the other arm.

### Run specific benchmark function
```bash
cargo bench --bench protocol_bench -- json_rpc_initialize
cargo bench --bench nda_bench -- nda_compile_with_triples
```

### Compare with baseline
```bash
# Save current results as baseline
cargo bench --bench nda_bench -- --save-baseline current

# Make changes, then compare
cargo bench --bench nda_bench -- --baseline current
```

`--save-baseline` and `--baseline` are Criterion flags, and the six custom reporters read no
command-line arguments at all, so a bare `cargo bench -- --save-baseline current` runs those
six to completion for nothing and saves a baseline only for the three Criterion suites. Scope
the flag to one `--bench` target, as above.

## Benchmark Suites

### protocol_bench

Eight Criterion benchmark functions, all of which call the library directly:

- `json_rpc_initialize` - `handle_request` on an initialize request
- `json_rpc_tools_list` - `handle_request` on a tools/list request
- `json_rpc_ping` - `handle_request` on a ping request
- `json_rpc_health_check` - `handle_request` on a health/check request
- `json_rpc_parse_only` - `serde_json::from_str` on a ping request line, no dispatch
- `json_rpc_dispatch_only` - `handle_request` on the same ping request as `json_rpc_ping`, so
  these two functions time identical work under two names
- `registry_dispatch` - `registry::get_tools()`, the full tool list assembly
- `audit_record_throughput` - `audit::global_audit().record(...)`

The first four take an already-built `serde_json::Value`, so they time dispatch plus response
construction and exclude parsing; `json_rpc_parse_only` is the parse half of that split.

### nda_bench

Six functions over the NDA (Neural Document Archive) binary document format:

- `nda_compile_empty` - `NdaCompiler::new()` then `compile()`
- `nda_compile_with_triples` - same, with three triples added
- `nda_read` - `NdaDocument::read` over a pre-compiled document
- `nda_verify_merkle` - `verify_merkle` on a pre-parsed document
- `nda_compile_large` - compile with 1,000 triples
- `middleware_cache_lookup` - a `ResponseCache` miss lookup, and on a default-feature build
  there is no `ResponseCache` to construct: the `#[cfg(not(feature = "http"))]` arm of this
  function times `std::hint::black_box(0)` and publishes it under the same
  `middleware_cache_lookup` id, so the timing `cargo bench` reports for it on the default
  profile measures an empty loop, not a cache. Use `--features http` for the real measurement.

### binary_protocol_bench

Criterion groups over the TLV codec and its JSON counterpart, declared as four
`criterion_group!` macros (`encode_benches`, `decode_benches`, `json_comparison`,
`roundtrip_benches`) covering twelve target functions and thirteen named benchmark ids,
including `encode_2_fields`, `decode_10_fields`, `nested_2_levels`, `array_10_elements`,
`encode_20_params_1kb` and the `tlv_size` / `json_size` payload comparison. Ten of the twelve
benchmark groups set `Throughput::Elements(1)`, so their reports carry iterations per second
rather than bytes per second; `tlv_large_payload` is the exception at `Throughput::Bytes(1024)`,
and `size_comparison` sets no throughput at all.

### Custom-reporter suites

These six do not use Criterion and store no baseline; each `main()` runs its own sequence and
prints tables. They are the expensive ones -- several start real servers or compile WASM.

- `wasm_isolation` - Wasmer cost split four ways: memory-only, trampoline-only, execute-only,
  and the combined pipeline
- `wasm_vs_native` - the same `text_analyze` tool through WASM and against native interpreters,
  cold (spawn per call) and hot (one persistent stdio process)
- `wasm_tool_latency` - first-call (cold), second-call (warm) and P99 over 100 calls, for its 12
  WASM runtimes; the native baselines it prints at the end are transcribed estimates, not measures
- `comprehensive_benchmark` - the 12 languages this binary drives (mruby is skipped and
  TypeScript rides on the QuickJS module) over one pipeline, JSON-RPC stdio; its recommendations
  block names NDA/shmem throughput, a pipeline this harness never measures, and its header long
  claimed a per-runtime memory footprint that no field in the code is ever set
- `metering_cache_bench` - four parts: unmetered compile vs metered compile vs cached-artifact
  deserialize over the 12 modules, per-language runtime cold start, per-call metering overhead
  through `WasmRuntimeRegistry::call_tool`, and an E2E stdio run of the real server binary at two
  `VELOCITY_WASM_INSTRUCTION_LIMIT` settings
- `tool_compatibility_test` - its header records 16 tools across 10 languages, 20 cases each,
  WASM output against native

## Understanding Results

Criterion generates detailed statistical reports including:

- **Mean time**: Average execution time
- **Median time**: Middle value (less affected by outliers)
- **Standard deviation**: Variability in measurements
- **Confidence intervals**: 95% confidence bounds
- **Outlier detection**: Identifies anomalous measurements

Results are saved in `target/criterion/` with HTML reports for easy visualization.

## What these suites measure, and what the published numbers are for

The three Criterion suites call library functions in one process: `protocol_bench` hands a
`serde_json::Value` to `handle_request`, `nda_bench` drives `NdaCompiler` and `NdaDocument`,
`binary_protocol_bench` encodes and decodes TLV buffers. None of them crosses a transport, a
process boundary or a shared-memory ping-pong, so their per-iteration times are handler and
codec costs and are not comparable with the pipeline latencies published elsewhere.

End-to-end figures live in `docs/COMPARISON.md` and `docs/COMPREHENSIVE_BENCHMARK.md`, measured
by the `bench_nda/` harness and the `comprehensive_benchmark` reporter over stdio, shmem, HTTP
and WASM. `bench_nda/bench_full_run.txt` is a tracked run of that same harness, but not the run
behind the Node-versus-Rust rows below: it stamps v3.0.0 over 17 tools and puts Node/stdio
tools/list at 0.073 ms against Rust JSON/stdio at 0.165 ms. The dated root capture that
`docs/COMPARISON.md` names as its raw source is not tracked; that page records the same fact.

An earlier revision of this page carried a "Performance Targets" table timing initialize and
tools/list at `<0.2ms` against a `~0.6ms` "Node.js Equivalent" for a claimed "3x". The
repository's own recorded comparison contradicts it. That run has Node.js JSON/stdio at
0.149 ms ping and 0.197 ms tools/list, not ~0.6 ms, and it has Rust JSON/stdio tools/list at
0.878 ms, where Rust was slower than Node rather than three times faster; the same two rows are
published in `docs/COMPARISON.md` and `docs/USER_GUIDE.md`. A standalone 0.6 ms figure appears
nowhere else in the first-party docs. The table also compared unlike quantities, since these
suites time one in-process function call while those figures are request round trips. It was
removed rather than re-quoted. Targets for these suites should come from running them on the
tree being measured.

## Continuous Benchmarking

No CI step runs these benchmarks. Neither `cargo bench` nor `--bench` appears in any workflow
under `.github/workflows/`, so a regression in a suite is only caught by running it by hand.
The process below is the intention, not the current state; wiring it up, like wiring the doc
gates recorded in `CHANGELOG.md`, is an open owner decision.

1. Run benchmarks in CI on every PR (not wired)
2. Compare against main branch baseline
3. Flag regressions >10%
4. Track improvements in release notes

Example CI integration:
```yaml
- name: Run benchmarks
  run: cargo bench --bench protocol_bench -- --save-baseline pr
  
- name: Compare with main
  run: cargo bench --bench protocol_bench -- --baseline main
```

## Profiling

`perf` and `valgrind` are Linux-only tools and nothing in this repository has run them; the
commands below are the shapes to use on a Linux host.

```bash
# CPU profiling with perf (Linux)
perf record cargo bench --bench protocol_bench
perf report

# Memory profiling with valgrind (Linux)
valgrind --tool=massif cargo bench --bench nda_bench

# Flame graphs
cargo flamegraph --bench protocol_bench
```

## Adding New Benchmarks

1. Create a new file in `benches/` directory
2. Add a `[[bench]]` entry to `Cargo.toml` with `harness = false`; all nine existing targets set
   it, because `criterion_main!` supplies `main` and the reporter suites write their own
3. Use Criterion's benchmarking API
4. Do setup work outside `b.iter()`, since everything inside the closure is timed

Example:
```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn my_benchmark(c: &mut Criterion) {
    // Setup: runs once, outside the measurement loop
    let data = setup();

    c.bench_function("my_operation", |b| {
        b.iter(|| {
            // Operation: this is what the reported time measures
            black_box(operation(&data))
        })
    });
}

criterion_group!(benches, my_benchmark);
criterion_main!(benches);
```

An earlier revision of this example put `let data = setup();` inside `b.iter()` and labelled it
"not measured". Criterion times the whole closure, so that comment described the opposite of
what the code did.

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
- Increase sample size: `cargo bench --bench nda_bench -- --sample-size 200` (a Criterion id).
  Each suite also pins its own size in code: `protocol_bench` and `nda_bench` run
  `sample_size(100)` and `binary_protocol_bench` runs `sample_size(1000)`.
- Check for background processes
- Ensure stable CPU frequency

### Gnuplot not found
- Criterion falls back to plotters backend
- Install gnuplot for better plots: `apt-get install gnuplot` (Linux)

## Resources

- [Criterion User Guide](https://bheisler.github.io/criterion.rs/book/)
- [Rust Performance Book](https://nnethercote.github.io/perf-book/)
- [Criterion API Docs](https://docs.rs/criterion/)
