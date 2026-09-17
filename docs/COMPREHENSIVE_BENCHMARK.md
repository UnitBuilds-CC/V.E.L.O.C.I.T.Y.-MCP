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

1. **WASM modules compiled** in `bench_tools/` (paths as checked by `benches/comprehensive_benchmark.rs`, sizes on disk 2026-09-17):

   *Production runtimes:*
   - `quickjs_wasm/quickjs.wasm` (754,433 bytes — also used by TypeScript)
   - `micropython_wasm/wasi-reactor/build/micropython.wasm` (587,776 bytes)
   - `lua_wasm/lua.wasm` (680,094 bytes)
   - `tinygo_wasm/tool.wasm` (909,213 bytes)
   - `mruby_wasm/ruby.wasm` (1,583,417 bytes)
   - `rust_wasm/target/wasm32-wasip1/release/rust_wasm_tool.wasm` (56,310 bytes)

   *Planned runtimes (toy interpreters):*
   - `php_wasm/php.wasm` (277,664 bytes)
   - `csharp_wasm/dotnet.wasm` (277,747 bytes)
   - `java_wasm/java.wasm` (277,713 bytes)
   - `r_wasm/r.wasm` (277,630 bytes)
   - `julia_wasm/julia.wasm` (277,698 bytes)
   - `perl_wasm/perl.wasm` (277,681 bytes)

2. **Node.js installed** (for transport protocol benchmarks)

3. **Native tool scripts** in `bench_tools/`:
   - `node_tool.js` (for JSON-RPC stdio baseline)

## Understanding the Output

### WASM Runtime Section

For each runtime, you'll see:

```
─── <Language> ──────────────────────────────────────────
  Cold start: <n> ms
  Correctness: OK
  Hot call: <n> µs/call (<n> calls/s)
    P50: <n> µs, P95: <n> µs, P99: <n> µs
```

**Metrics explained:**
- **Cold start**: Time to compile WASM module, instantiate runtime, register tool, and make first call (includes compilation overhead)
- **Hot call**: Average latency per call after warmup (500 iterations), excluding cold start
- **P50/P95/P99**: Latency percentiles showing distribution (P99 = worst 1% of calls)
- **Correctness**: Verification that output matches expected values (word_count=29, char_count=159)

A hot-call line is only usable when the run reports zero call failures. The benchmark still prints a mean for a partially failing run (it averages only the surviving calls), so always check the "had N call failures out of 500 attempts" line first — the 2026-09-17 run fails the regex-based tool on JavaScript/TypeScript (500/500) and on all six toy interpreters (`wc=0 cc=0`), and reports 481/500 and 436/500 failures for Python and Lua. For authoritative WASM hot-path latency use `wasm_vs_native` instead (see [WASM Runtime Benchmark Analysis](benchmark_analysis_wasm_runtimes.md)).

### Go (TinyGo) Special Case

Go shows two measurements:
- **Per-call (no cache)**: Full compilation + instantiation per call (production path without caching)
- **Cached module**: Module compiled once, reused for subsequent calls. Freshly measured: `wasm_vs_native` (2026-09-17) gets 191,575.0 µs/call uncached vs 257.3 µs/call cached — **744.5x** for `text_analyze`; `metering_cache_bench` Part B (2026-09-17) gets 189,500 µs on first runtime create vs 6,351 µs on a module-cache hit — **29.8x**. The two ratios measure different things (whole per-call work vs module compile-vs-deserialize), so quote the benchmark name with the number.

### Transport Protocol Section

```
─── Transport: JSON-RPC stdio ─────────────────────────────
  Ping: 594.5 µs
  Tools/list: 115.2 µs
  Tools/call: 125.1 µs
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
  Fastest WASM runtime: <runtime> (<n> µs/call)
  Fastest transport: JSON-RPC stdio (125.1 µs)

  For low-latency tools (<100µs): Use QuickJS, MicroPython, or Lua
  For complex computations: Use Rust WASI or TinyGo
  For maximum compatibility: Use Node.js native with JSON-RPC stdio
  For highest throughput: Use NDA binary protocol with shmem transport
```

The "Fastest WASM runtime" line ranks by this benchmark's own hot-call numbers, so it is meaningless in a run where calls failed. The fixed advice lines are generated by the same code path and are not measured claims about your workload.

## Interpreting Results

### When to Use Each Runtime

**QuickJS (JavaScript)**: Best for general-purpose scripting — the fastest measured WASM hot path relative to its native counterpart (15.7 µs vs 68.3 µs Node.js, 4.36x). It has the *slowest* cold start of the three in-process scripting engines measured (149.9 ms vs Lua 82.5 ms and MicroPython 80.5 ms), so it is a poor fit for one-shot invocations.

**MicroPython (Python)**: Excellent Python compatibility with minimal overhead. The only engine measured that beat its native runtime on both axes: 15.1 µs vs 47.8 µs hot, 80.5 ms vs 249.4 ms cold against CPython.

**Lua**: Fastest WASM hot call measured on this machine (11.1 µs, 90.3K calls/s). Memory footprint is not currently measured by any of these benchmarks, so treat size-related claims as unverified.

**mruby (Ruby)**: Ruby syntax with WASM-native execution. Good for Ruby developers who want familiar syntax without MRI overhead. Not covered by the 2026-09-17 WASM-vs-native run, so no latency figure is stated here.

**Rust WASI**: Cheapest module to compile in `metering_cache_bench` Part A (11,378 µs, vs 76,712–324,541 µs for MicroPython/QuickJS/Lua/Ruby) and the cheapest runtime create (9,791 µs first, 1,181 µs cached). No 2026-09-17 Rust hot-call figure exists — the `comprehensive_benchmark` WASM numbers from that run are unusable, so no ranking claim is made.

**TinyGo (Go)**: Compiles ahead of time to a real WASM module, but has no persistent instance in this harness, so every call pays instantiation: 257.3 µs/call cached (3.9K calls/s) vs 11.1–15.7 µs/call for the in-process scripting engines. Cached module mode is mandatory, and goroutine-style concurrency is limited under WASM.

**PHP/C#/Java/R/Julia/Perl** (planned, not production-ready): Tree-walk interpreters sharing a common C core. Currently toy interpreters with hardcoded demo wrappers, not suitable for production workloads: in the 2026-09-17 run all six returned `wc=0 cc=0` for real language syntax. No performance comparison against the production runtimes is meaningful when the tool never produces a result. These are targets for future development toward production readiness.

### Performance Trade-offs

| Factor | Interpreter WASM | Compiled WASM | Native Process |
|--------|------------------|---------------|----------------|
| Cold Start | 80.5–149.9 ms (MicroPython/Lua/QuickJS, `wasm_vs_native`) | 184.7 ms (TinyGo) | 45.7–249.4 ms (Lua/Node/CPython) |
| Hot Latency | 11.1–15.7 µs (`text_analyze`, persistent instance) | 257.3 µs cached (per-call instantiation) | 40.8–68.3 µs |
| Language Support | Limited subset | Full language | Full ecosystem |

**Memory is not currently measured.** The benchmark prints a `Mem (KB)` column and every row is `—`; per-runtime memory footprint is still open work (see Future Work). MB figures in earlier revisions of this document had no instrumented source and have been removed.

### Caching Impact

TinyGo demonstrates the power of caching, at two different scopes — read the label with the number, because both are quoted as "the Go cache speedup":

- **Whole per-call work** (`wasm_vs_native`, 2026-09-17, `text_analyze`): 191,575.0 µs/call uncached → 257.3 µs/call with the compiled module reused = **744.5x**.
- **Module compile vs deserialize** (`metering_cache_bench` Part B, 2026-09-17, Go/TinyGo runtime creation): 189,500 µs on first create → 6,351 µs on a cache hit = **29.8x**.

The same principle applies to all WASM runtimes — every runtime in Part B below hits the cache, from 1.2x (TypeScript, whose QuickJS module is already cached by the time it runs) to 29.8x (Go). Cold start costs don't disappear, but compilation, the dominant term, happens once per process.

### Metering & Module-Cache Measurements (production paths)

Measured with `cargo bench --bench metering_cache_bench` on 2026-09-17 (release profile, Windows, Core 5 210H, Wasmer 5.0.6 / Cranelift). Module primitives (Part A) and runtime creation (Part B):

| Metric | QuickJS | Lua |
|--------|---------|-----|
| Compile (unmetered) | 168,573 µs | 80,792 µs |
| Compile (metered, 10M limit) | 419,535 µs | 188,043 µs |
| Deserialize cached module | 15,905 µs (10.6x vs compile) | 8,050 µs (10.0x vs compile) |
| Runtime create, first | 168,862 µs | 112,316 µs |
| Runtime create, cache hit | 13,751 µs (12.3x) | 8,133 µs (13.8x) |

Per-call metering overhead (Part C, 200 warmup + 5,000 timed calls through `WasmRuntimeRegistry::call_tool`) and end-to-end effect (Part D, real server binary over stdio, 300 calls of `js_string_transform`):

| Metric | QuickJS | Lua |
|--------|---------|-----|
| Hot call, metered (10M limit) | 11,891.2 ns mean (p50 11,500 / p99 26,800) | 5,909.1 ns mean (p50 5,000 / p99 24,700) |
| Hot call, unmetered | 5,341.6 ns mean (p50 4,600 / p99 9,700) | 3,556.6 ns mean (p50 3,400 / p99 15,200) |
| Overhead | **+122.6% mean** | **+66.1% mean** |

Part D is not per-runtime: it drives the real server binary over stdio with 300 `js_string_transform` calls at `VELOCITY_WASM_INSTRUCTION_LIMIT=10000000` vs `0`, and reports 88,043.7 ns mean (p50 81,900 / p99 197,300) metered against 92,861.7 ns mean (p50 77,800 / p99 266,800) unmetered.

Key findings:
- **Module caching is worth 1.2x–29.8x on runtime creation** across the runtimes measured, and 6.4x–28.4x on the raw compile→deserialize step (Part A).
- **Metering more than doubles compile time** (QuickJS 168,573 → 419,535 µs, Lua 80,792 → 188,043 µs) and **raises per-call latency on minimal tools by +122.6% (QuickJS) and +66.1% (Lua)**. Through the full JSON-RPC stdio stack it is not measurable above noise: 88.0 µs mean metered vs 92.9 µs mean unmetered, with the metered p99 actually lower (197.3 µs vs 266.8 µs).
- Metering is enforced at compile time — the instruction limit is part of the module cache key, so cached modules always match the active limit.

Cache scope: `MODULE_CACHE` is consulted by **every** runtime, not just one. Runtimes built on the shared `create_wasm_instance` helper hit it directly, and the Rust and Go runtimes, which manage their own `Store`/`Engine`, go through `compile_module_cached`. All 13 Part B rows show a cache-hit time. The cache is process-local, so each new server process pays the compile once.

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

**Out of memory**: The benchmark holds every module's compiled artifact in the process module cache while measuring, so peak usage scales with how many modules are loaded at once. Memory per runtime is not measured by these benchmarks (the `Mem` column prints `—`); if a run dies part-way through, reduce the set of modules present in `bench_tools/` and re-run.

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
