# WASM Runtime Benchmark Analysis (2026-09-17)

## Executive Summary

Benchmarks re-run on 2026-09-17 confirm that **7 runtimes use real language engines** while **6 are toy interpreters** that cannot execute real language syntax: in `comprehensive_benchmark` all 6 toy interpreters return empty results (`wc=0 cc=0`) for the regex-based tool. Where a number below has no fresh measurement, it is marked as not re-measured rather than carried over from an older run.

Three source runs back this document:

- `cargo bench --bench wasm_vs_native` — same `text_analyze` tool, WASM vs the native CLI (JavaScript, Python, Lua, Go).
- `cargo bench --bench metering_cache_bench` — module compile / metered compile / cached deserialize, per-runtime cold start with the module cache, and metering overhead.
- `cargo bench --bench comprehensive_benchmark` — correctness of the regex tool across all runtimes, plus the JSON-RPC stdio transport.

## WASM vs Native (same tool, `wasm_vs_native`, 2026-09-17)

| Language | Engine | WASM hot (µs/call) | Native hot (µs/call) | WASM vs native | WASM cold (ms) | Native cold (ms) |
|----------|--------|--------------------|----------------------|----------------|----------------|------------------|
| **JavaScript** | QuickJS | **15.7** (63.8K calls/s) | 68.3 (Node.js) | **4.36x** | 149.9 | 70.6 |
| **Python** | MicroPython | **15.1** (66.0K calls/s) | 47.8 (CPython) | **3.16x** | 80.5 | 249.4 |
| **Lua** | Lua 5.4 | **11.1** (90.3K calls/s) | 40.8 (Lua 5.4 CLI) | **3.69x** | 82.5 | 45.7 |
| **Go** | TinyGo | 257.3 cached / 191,575.0 uncached | not measured | — | 184.7 | not measured |

- On the hot path WASM beats the native CLI for all three scripting languages measured (4.36x JavaScript, 3.69x Lua, 3.16x Python).
- Cold start goes the other way for JavaScript (149.9 ms vs 70.6 ms) and Lua (82.5 ms vs 45.7 ms); only Python wins cold (80.5 ms vs 249.4 ms CPython).
- Go instantiates per call and has no persistent instance in this harness; reusing the compiled module gives **744.5x** over the uncached path (191,575.0 → 257.3 µs). Native `go` was not installed, so no Go ratio and no Go native cold start exist.
- **Not re-measured this run** (no fresh hot/cold numbers, so none are stated): Rust, TypeScript, Ruby, PHP, C#, Java, R, Julia, Perl.

## Module Compile, Metering Cost, and Cache Reuse (`metering_cache_bench`, 2026-09-17)

Part A — module primitives, unmetered compile vs metered compile vs deserializing the cached artifact:

| Module | Compile (µs) | Metered compile (µs) | Deserialize cached (µs) | Compile→deserialize |
|--------|--------------|----------------------|-------------------------|---------------------|
| quickjs.wasm | 168,573 | 419,535 | 15,905 | 10.6x |
| lua.wasm | 80,792 | 188,043 | 8,050 | 10.0x |
| micropython.wasm | 76,712 | 165,026 | 11,472 | 6.7x |
| ruby.wasm | 324,541 | 905,221 | 22,129 | 14.7x |
| php.wasm | 30,056 | 65,984 | 2,646 | 11.4x |
| dotnet.wasm | 23,958 | 67,837 | 2,448 | 9.8x |
| java.wasm | 24,970 | 61,331 | 3,079 | 8.1x |
| r.wasm | 28,186 | 59,193 | 2,072 | 13.6x |
| julia.wasm | 24,027 | 74,116 | 3,739 | 6.4x |
| perl.wasm | 30,960 | 83,261 | 3,317 | 9.3x |
| rust (example_tool.wasm) | 11,378 | 23,799 | 1,087 | 10.5x |
| tinygo.wasm | 191,412 | 366,570 | 6,744 | 28.4x |

Part B — full runtime creation, first (compile) vs subsequent (cache hit). Unmetered, so the comparison isolates compile-vs-deserialize:

| Runtime | First create (µs) | Cache hit (µs) | Speedup |
|---------|-------------------|----------------|---------|
| JavaScript (QuickJS) | 168,862 | 13,751 | 12.3x |
| TypeScript | 17,239 | 14,623 | 1.2x |
| Python (MicroPython) | 92,598 | 12,756 | 7.3x |
| Lua | 112,316 | 8,133 | 13.8x |
| Ruby (mruby) | 314,226 | 19,555 | 16.1x |
| PHP | 33,571 | 2,410 | 13.9x |
| C# (.NET) | 28,834 | 3,070 | 9.4x |
| Java (TeaVM) | 30,381 | 2,737 | 11.1x |
| R (WebR) | 40,199 | 3,150 | 12.8x |
| Julia | 32,640 | 2,356 | 13.9x |
| Perl | 30,533 | 2,261 | 13.5x |
| Rust (WASI) | 9,791 | 1,181 | 8.3x |
| Go (TinyGo) | 189,500 | 6,351 | 29.8x |

Every runtime in the table hits the cache, including the toy-interpreter modules — `MODULE_CACHE` is shared by all of them through `create_wasm_instance` (plus `compile_module_cached` for Rust and Go). TypeScript's 1.2x is the outlier; the likely reason, inferred from the shared `quickjs.wasm` bytes and the run order rather than measured directly, is that the JavaScript row already put that module in the cache.

### Key Findings

- **WASM beats native on the hot path** for all three scripting languages measured: JavaScript 4.36x, Lua 3.69x, Python 3.16x.
- **Lua is the fastest measured WASM hot path** at 11.1 µs/call (90.3K calls/s), with QuickJS at 15.7 µs and MicroPython at 15.1 µs.
- **Compilation dominates cold start**: deserializing the cached artifact is 6.4x–28.4x cheaper than compiling, and full runtime creation on a cache hit is 1.2x–29.8x cheaper.
- **Metering costs more on some engines than others**: through `WasmRuntimeRegistry::call_tool` at a 10M instruction limit, QuickJS goes 5,341.6 → 11,891.2 ns mean (**+122.6%**, p99 9.7 → 26.8 µs) — more than double — while Lua goes 3,556.6 → 5,909.1 ns mean (**+66.1%**, p99 15.2 → 24.7 µs).
- **Metering is not measurable above noise end-to-end**: through the real server binary over JSON-RPC stdio (300 calls, `js_string_transform`), metered mean is 88,043.7 ns (p50 81.9, p99 197.3 µs) vs unmetered 92,861.7 ns (p50 77.8, p99 266.8 µs). Transport and dispatch dominate the WASM cost at this tool size; a heavier tool would show the Part C overhead instead.
- **Metering is enforced at compile time** and the instruction limit is part of the cache key, so a cached module always matches the active limit.

## Toy Interpreters — FAILING Correctness (6/13)

All 6 share the same C interpreter core (`interp_core/interp.c`, 277,630–277,747 bytes per module). Given real language syntax by `comprehensive_benchmark` (2026-09-17) they return empty results:

| Language | Result | Failed calls / attempts |
|----------|--------|-------------------------|
| PHP | `wc=0 cc=0` | 364 / 500 |
| C# | `wc=0 cc=0` | 384 / 500 |
| Java | `wc=0 cc=0` | 378 / 500 |
| R | `wc=0 cc=0` | 386 / 500 |
| Julia | `wc=0 cc=0` | 390 / 500 |
| Perl | `wc=0 cc=0` | 370 / 500 |

The "hot call" figures this benchmark prints for them (and for JavaScript, TypeScript, Python and Lua) are computed over the surviving calls of a failing run and are therefore not usable as latency data — `wasm_vs_native` is the authoritative source instead. They cannot parse `preg_split()`, `String.Split()`, `HashMap`, `strsplit()`, or any language-specific syntax; they only support a shared DSL.

JavaScript and TypeScript failed 500/500 on this harness's regex tool (`RuntimeError: unreachable` in `js_regexp_exec`) while the same engines pass `text_analyze` in `wasm_vs_native` — a limitation of the regex workload in this benchmark, not a hot-path latency result.

See [WASM Runtime Status](wasm_runtime_status.md) for the integration roadmap.

## Transport Protocol Benchmarks

Measured 2026-09-17 by `comprehensive_benchmark` against the Node.js tool server over stdio:

| Protocol | Ping | tools/list | tools/call |
|----------|------|------------|------------|
| JSON-RPC stdio (Node.js) | 594.5 µs | 115.2 µs | 125.1 µs |

For the eight-pipeline Rust transport comparison (NDA/JSON over shmem/stdio/HTTP), see [Performance Comparison](COMPARISON.md) and the `bench_results_core5.txt`-sourced tables in the README.

## Recommendations

- **Low-latency scripting tools**: Lua (11.1 µs), then MicroPython (15.1 µs) and QuickJS (15.7 µs) — all on the `text_analyze` hot path.
- **Complex computations**: Rust WASI or TinyGo; Rust has the cheapest compile (11,378 µs) and TinyGo the largest cache payoff (29.8x on runtime creation, 744.5x on `text_analyze` reuse).
- **Turn metering on without latency fear**: it doubles minimal-tool WASM time but is unmeasurable end-to-end over stdio.
- **Highest throughput**: NDA binary protocol with shmem transport.

## Benchmark Methodology

- **Environment**: Windows 10 x64, Intel Core 5 210H (12 cores), Wasmer 5.0.6 with the Cranelift backend, release profile.
- **WASM vs native**: cold start = compile/instantiate + register + first tool call; hot call = repeated `call_tool()` on a persistent instance, with the native CLI on the same input for comparison.
- **Metering/cache**: Part A times module compile (unmetered and metered) vs deserialization of the serialized artifact; Part B times full runtime creation first-vs-cached; Part C times 5,000 calls after 200 warmups through `WasmRuntimeRegistry::call_tool`; Part D times 300 calls through the real server binary over stdio.
- **Correctness**: the `text_analyze` / regex tool output is checked against expected word and char counts.

---

*Measured 2026-09-17, Core 5 210H, wasmer 5.0.6. Numbers from earlier runs were removed rather than reused when no raw output for them exists.*
