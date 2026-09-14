# Comprehensive WASM + Transport Protocol Benchmark Guide

This guide explains how to run and interpret the comprehensive benchmark suite that measures performance across all 7 production WASM language runtimes (and 6 planned runtimes) and transport protocols.

## Overview

The `comprehensive_benchmark` binary provides unified performance measurement for:

- **7 Production WASM Language Runtimes**: QuickJS (JavaScript), TypeScript (QuickJS), MicroPython (Python), Lua, mruby (Ruby), Rust WASI, TinyGo (Go)
- **6 Planned WASM Language Runtimes** (currently toy interpreters): PHP, C#/.NET, Java/TeaVM, R/WebR, Julia, Perl
- **Transport Protocols**: JSON-RPC stdio, with extensibility for HTTP, NDA shmem, and JSON shmem
- **Multiple Metrics**: Cold start, hot call latency, cached module reuse, P50/P95/P99 percentiles

## Running the Benchmark

```bash
# Run full benchmark suite
cargo bench --bench comprehensive_benchmark

# Run with release optimizations
cargo bench --release --bench comprehensive_benchmark
```

## Prerequisites

Before running the benchmark, ensure you have:

1. **WASM modules compiled** in `bench_tools/`:

   *Production runtimes:*
   - `quickjs_wasm/quickjs.wasm`
   - `micropython_wasm/wasi-reactor/build/micropython.wasm`
   - `lua_wasm/lua.wasm`
   - `tinygo_wasm/tool.wasm`
   - `ruby_wasm/mruby.wasm`
   - `rust_wasm/tool.wasm`

   *Planned runtimes (toy interpreters):*
   - `php_wasm/php.wasm`
   - `csharp_wasm/dotnet.wasm`
   - `java_wasm/teavm.wasm`
   - `r_wasm/webr.wasm`
   - `julia_wasm/julia.wasm`
   - `perl_wasm/perl.wasm`

2. **Node.js installed** (for transport protocol benchmarks)

3. **Native tool scripts** in `bench_tools/`:
   - `node_tool.js` (for JSON-RPC stdio baseline)

## Understanding the Output

### WASM Runtime Section

For each runtime, you'll see:

```
─── JavaScript ────────────────────────────────────────────────
  Cold start: 45.2 ms
  Correctness: OK
  Hot call: 88.2 µs/call (11.3K calls/s)
    P50: 82.1 µs, P95: 95.3 µs, P99: 102.7 µs
```

**Metrics explained:**
- **Cold start**: Time to compile WASM module, instantiate runtime, register tool, and make first call (includes compilation overhead)
- **Hot call**: Average latency per call after warmup (500 iterations), excluding cold start
- **P50/P95/P99**: Latency percentiles showing distribution (P99 = worst 1% of calls)
- **Correctness**: Verification that output matches expected values (word_count=29, char_count=159)

### Go (TinyGo) Special Case

Go shows two measurements:
- **Per-call (no cache)**: Full compilation + instantiation per call (production path without caching)
- **Cached module**: Module compiled once, reused for subsequent calls (~15x faster cold starts via the production module cache; see `benches/metering_cache_bench.rs`)

### Transport Protocol Section

```
─── Transport: JSON-RPC stdio ─────────────────────────────
  Ping: 29.1 µs
  Tools/list: 80.3 µs
  Tools/call: 29.4 µs
```

**Metrics explained:**
- **Ping**: Round-trip time for empty request/response
- **Tools/list**: Time to enumerate available tools
- **Tools/call**: Time to execute a tool with arguments

### Summary Tables

Two consolidated tables are printed:

1. **WASM Runtime Summary**: All runtimes side-by-side with cold/hot/cached metrics
2. **Transport Protocol Summary**: All transport protocols with ping/list/call times

### Recommendations Section

The benchmark outputs actionable recommendations based on measured data:

```
═══ Recommendations ════════════════════════════════════════════════
  Fastest WASM runtime: JavaScript (88.2 µs/call)
  Fastest transport: JSON-RPC stdio (29.4 µs)

  For low-latency tools (<100µs): Use QuickJS, MicroPython, or Lua
  For complex computations: Use Rust WASI or TinyGo
  For maximum compatibility: Use Node.js native with JSON-RPC stdio
  For highest throughput: Use NDA binary protocol with shmem transport
```

## Interpreting Results

### When to Use Each Runtime

**QuickJS (JavaScript)**: Best for general-purpose scripting, fastest cold start among interpreter-based runtimes. Ideal for string manipulation, JSON processing, and lightweight logic.

**MicroPython (Python)**: Excellent Python compatibility with minimal overhead. Best when you need Python ecosystem familiarity but can't afford CPython's startup cost.

**Lua**: Ultra-lightweight with predictable performance. Best for embedded scenarios or when memory footprint is critical.

**mruby (Ruby)**: Ruby syntax with WASM-native execution. Good for Ruby developers who want familiar syntax without MRI overhead.

**Rust WASI**: Highest raw performance for compute-intensive tasks. Best for numerical computations, cryptography, or when you need type safety guarantees.

**TinyGo (Go)**: Best balance of performance and developer productivity. Excellent concurrency model via goroutines (though limited in WASM). Cached module mode recommended for production.

**PHP/C#/Java/R/Julia/Perl** (planned, not production-ready): Tree-walk interpreters sharing a common C core. Currently toy interpreters with hardcoded demo wrappers, not suitable for production workloads. Slower than native WASM and provide only basic language coverage. These are targets for future development toward production readiness.

### Performance Trade-offs

| Factor | Interpreter WASM | Compiled WASM | Native Process |
|--------|------------------|---------------|----------------|
| Cold Start | 50-200ms | 100-500ms | 200-1000ms |
| Hot Latency | 5-100µs | 1-50µs | 100-500µs |
| Memory | 2-10MB | 5-20MB | 50-200MB |
| Language Support | Limited subset | Full language | Full ecosystem |

### Caching Impact

TinyGo demonstrates the power of module caching:
- **Uncached**: ~200ms per call (compile + instantiate + execute)
- **Cached**: ~0.3ms per call (instantiate + execute only)
- **Speedup**: 670x improvement

The same principle applies to all WASM runtimes - if you can keep the compiled module in memory and reuse it, cold start costs disappear.

### Metering & Module-Cache Measurements (production paths)

Measured with `cargo bench --bench metering_cache_bench` (release profile, Windows, Wasmer/Cranelift):

| Metric | QuickJS | Lua |
|--------|---------|-----|
| Compile (unmetered) | 471 ms | 206 ms |
| Compile (metered, 10M limit) | 1028 ms (+118%) | 445 ms (+117%) |
| Deserialize cached module | 33.6 ms (14.0x) | 11.5 ms (17.9x) |
| Runtime cold start, cache hit | n/a (no cache path) | 15.3 ms (15.4x vs 235 ms compile) |
| Hot call, metered | 28.4 µs mean | 12.2 µs mean |
| Hot call, unmetered | 11.6 µs mean | 4.7 µs mean |

Key findings:
- **Module caching is worth ~15x on cold start** for runtimes that use the cache path (Lua; the cache covers runtimes built through `create_wasm_instance`).
- **Metering roughly doubles compile time** (~+117% from instrumentation) and **roughly doubles per-call latency on minimal tools** (+145% QuickJS, +158% Lua through `WasmRuntimeRegistry::call_tool`). Through the full JSON-RPC stdio stack the dilution brings E2E overhead to ~+19% (208 µs vs 175 µs mean on `js_string_transform`).
- Metering is enforced at compile time — the instruction limit is part of the module cache key, so cached modules always match the active limit.

Scope note: `MODULE_CACHE` is currently consulted only by the Lua runtime construction path; other runtimes compile directly on init. Wiring them through the same cache is tracked as follow-up work.

## Extending the Benchmark

To add new measurements:

1. **Add a new WASM runtime**:
   - Implement `WasmRuntime` trait in `src/wasm_runtime/<lang>.rs`
   - Add tool source constant in benchmark
   - Add benchmark call in `main()` function

2. **Add a new transport protocol**:
   - Create `bench_transport_<name>()` function
   - Measure ping, list, and call operations
   - Add to `transport_results` vector

3. **Add custom workloads**:
   - Define new tool source with different computational patterns
   - Add separate benchmark section with descriptive name

## Troubleshooting

**"WASM module not found"**: Ensure you've built all WASM modules using the build scripts in `bench_tools/`. Run `./bench_tools/build_all.sh` (Linux/macOS) or equivalent batch files.

**"Node.js not found"**: Install Node.js v16+ and ensure it's in your PATH. The benchmark uses it for transport protocol baselines.

**High variance in results**: Close other applications, disable CPU throttling, and run multiple times. The benchmark uses median-of-3 to reduce noise, but system load can still affect results.

**Out of memory**: Some WASM runtimes (especially Java/TeaVM) require significant memory. Ensure you have at least 4GB free RAM.

## Comparison with Other Benchmarks

- **`wasm_vs_native.rs`**: Focused comparison of 4 languages (JS, Python, Lua, Go) with native counterparts. Use for deep dives into specific language performance.
- **`wasm_tool_latency.rs`**: Detailed percentile analysis for individual runtimes. Use for understanding latency distributions.
- **`wasm_isolation.rs`**: Measures Wasmer engine overhead in isolation. Use for understanding base WASM costs.
- **`comprehensive_benchmark.rs`**: This benchmark. Use for holistic view across all runtimes and transports.

## Future Work

Planned enhancements:
- Memory footprint measurement per runtime
- Concurrent call throughput testing
- Network I/O benchmarks (for WASI networking)
- Power consumption profiling
- Cross-platform comparison (Windows/Linux/macOS)
