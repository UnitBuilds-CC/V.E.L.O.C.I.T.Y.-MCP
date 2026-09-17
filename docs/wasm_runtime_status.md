# WASM Runtime Status

## Overview

VELOCITY-MCP supports **7 production-ready WASM runtimes** using real language engines, with **6 additional languages planned** for future integration using full compiler toolchains.

## Production-Ready Runtimes (7)

These runtimes use real language engines compiled to WASM and pass the `text_analyze` correctness check in `wasm_vs_native` (JavaScript, Python, Lua all report OK there). One caveat from the 2026-09-17 `comprehensive_benchmark` pass: JavaScript and TypeScript cleared the one-shot correctness check but then failed all 500 repeated calls of that harness's regex workload, and Python and Lua failed 481/500 and 436/500 of them — see [WASM Runtime Benchmark Analysis](benchmark_analysis_wasm_runtimes.md) before relying on a regex-heavy tool in those engines.

| # | Language | Engine | WASM Size (bytes on disk) | Hot Call, `text_analyze` (µs) | Cold Start (ms) | Notes |
|---|----------|--------|---------------------------|--------------------------------|-----------------|-------|
| 1 | JavaScript | QuickJS | 754,433 (`quickjs_wasm/quickjs.wasm`) | **15.7** | 149.9 | ES feature set is whatever upstream QuickJS provides; no conformance suite is run here |
| 2 | TypeScript | QuickJS + line-based type stripper | 754,433 (shares `quickjs.wasm`) | not currently measured | not currently measured | `transpile_ts_to_js` deletes annotations/interfaces; no `tsc`, so no type checking happens |
| 3 | Python | MicroPython | 587,776 (`micropython.wasm`) | **15.1** | 80.5 | Python 3.8 subset; repeated calls on a persistent instance work (top-level GC collect fix) |
| 4 | Lua | Lua 5.4 | 680,094 (`lua.wasm`) | **11.1** | 82.5 | Full Lua 5.4 |
| 5 | Ruby | mruby | 1,583,417 (`ruby.wasm`) | not currently measured | not currently measured | Ruby 3.x subset; custom WASM build |
| 6 | Rust | wasm32-wasi | 56,310 (`rust_wasm/example_tool.wasm`) | not currently measured | not currently measured | Smallest module and cheapest compile (11,378 µs, `metering_cache_bench` Part A) |
| 7 | Go | TinyGo/WASM | 909,213 (`tool.wasm`) | 257.3 cached (191,575.0 uncached) | 184.7 | Instantiated per call; no persistent instance |

Hot-call and cold-start figures come from `cargo bench --bench wasm_vs_native` on 2026-09-17 (Core 5 210H, wasmer 5.0.6), which runs the same `text_analyze` tool in WASM and in the native CLI. "not currently measured" means exactly that — no fresh number exists for that runtime, and the figures from earlier runs were dropped rather than reused.

### Key Characteristics

- **Fastest measured WASM hot path**: Lua at 11.1 µs/call (90.3K calls/s), ahead of MicroPython (15.1 µs) and QuickJS (15.7 µs)
- **Largest gain over native**: JavaScript, 4.36x vs Node.js (15.7 vs 68.3 µs)
- **Only runtime that beat native on cold start**: Python, 80.5 ms vs 249.4 ms CPython
- **Smallest module**: Rust at 56,310 bytes; largest is mruby at 1,583,417 bytes
- **Go pays per-call instantiation**: 257.3 µs even with the compiled module cached

## Future Feature: Real Language Engines (6 Planned)

These languages currently have placeholder implementations using a shared tree-walk interpreter (`interp_core`). They are **not production-ready** and cannot execute real language syntax. Full engine integration is planned.

| Language | Current State | Planned Engine | Estimated Effort | Binary Size | Feasibility |
|----------|--------------|----------------|------------------|-------------|-------------|
| PHP | Toy interpreter | php-wasm (Emscripten, WordPress Playground) | 1-2 weeks | 11.6–18.7 MiB — the vendored `bench_tools/php_wasm/node_modules/php-wasm` dist builds measure 12,151,768 to 19,573,213 bytes | High - battle-tested in production |
| Perl | Toy interpreter | zeroperl (Emscripten) | ~1 week | est. ~9 MB (not built here) | High - WASI reactor already exists |
| C#/.NET | Toy interpreter | componentize-dotnet (NativeAOT) | 1-3 weeks | est. 5-20 MB (not built here) | Medium - Component Model, not raw WASI |
| R | Toy interpreter | WebR fork (Emscripten) | 4-8 weeks | 17.2 MiB — `bench_tools/r_wasm/webr.wasm` is 18,062,845 bytes | Low - deeply browser-coupled |
| Java | Toy interpreter | Kotlin/Wasm or TeaVM | 1-8 weeks | est. 3-50 MB (not built here) | Low - only Kotlin targets WASI |
| Julia | Toy interpreter | None available | 12-24+ weeks | est. 50-100+ MB (no engine exists to build) | Not feasible - research problem |

Sizes marked "est." are unverified guesses about an engine that has not been built here; the PHP and R figures are byte counts of builds already vendored under `bench_tools/`.

### Why These Aren't Production Yet

The `interp_core` implementation provides a shared DSL that looks like multiple languages but isn't any of them. It supports basic variable assignment, arithmetic, function calls, and control flow, but **cannot parse**:

- Language-specific standard library functions (PHP's `preg_split()`, R's `strsplit()`)
- Object-oriented syntax (Java generics, C# LINQ)
- Regex patterns and literals
- Native data structure methods
- Any syntax beyond the shared interpreter grammar

When tested with real language syntax, all 6 return empty results (`wc=0 cc=0`).

### Integration Roadmap

**Phase 1 (Near-term, 2-3 weeks):**
- Integrate [zeroperl](https://github.com/6over3/zeroperl) as drop-in Perl replacement (~1 week)
- Adapt PHP from [WordPress Playground](https://wordpress.github.io/wordpress-playground/) Emscripten build (1-2 weeks)

**Phase 2 (Medium-term, if demand warrants):**
- C# via [componentize-dotnet](https://github.com/bytecodealliance/componentize-dotnet) if Component Model wrappers are acceptable

**Deferred / Unlikely:**
- R: Requires major re-engineering of WebR to strip browser dependencies
- Java: No production-ready JVM-to-WASI path exists; only Kotlin/Wasm targets WASI
- Julia: Fundamentally hostile to static WASM compilation (JIT architecture); research-only

### Technical Challenges

| Challenge | Affected Languages | Details |
|-----------|-------------------|---------|
| `setjmp/longjmp` | PHP, R | Requires Wasm exception support; Wasmer support unclear |
| Browser coupling | R (WebR) | SharedArrayBuffer, Web Workers, JS I/O proxies |
| GC model mismatch | Java, C# | JVM/.NET GC doesn't map cleanly to Wasm GC |
| JIT dependency | Julia | Core value prop is runtime JIT; static compilation defeats purpose |
| Binary size | All | Candidate real engines measured so far are 11.6–18.7 MiB (PHP, R) vs 277,630–277,747 bytes for the current toy-interpreter modules |
| Cold start | All | Real-engine cold start is not measured. The 2026-09-17 toy-interpreter run gives no usable timing either: every call failed the regex tool, so only the failure counts are reported |

## Benchmark Methodology

Latest run: 2026-09-17 on an Intel Core 5 210H (12 cores), Windows 10 x64, release build, Wasmer 5.0.6 with the Cranelift backend.

- **Hot call / cold start vs native** (`cargo bench --bench wasm_vs_native`): same `text_analyze` tool and input run against the WASM engine and the native CLI; cold start is compile + instantiate + register + first call, hot call repeats `call_tool()` on a persistent instance
- **Module compile / metered compile / cached deserialize, runtime create** (`cargo bench --bench metering_cache_bench`)
- **Correctness across all 12 modules** (`cargo bench --bench comprehensive_benchmark`)
- **Correctness check**: output must match the expected `{word_count: 29, char_count: 159}` for the sample text
- **Percentiles**: P50/P95/P99 where the benchmark prints them; the 2026-09-17 WASM-vs-native run prints a single per-call figure with no percentile breakdown

Run benchmarks: `cargo bench --bench wasm_vs_native` and `cargo bench --bench metering_cache_bench` (plus `cargo bench --bench comprehensive_benchmark` for the correctness pass over all 12 modules).

## Architecture Notes

### Production Runtime Pattern

All production runtimes implement the `WasmRuntime` trait:

```rust
pub trait WasmRuntime {
    fn init(&mut self) -> Result<(), Box<dyn Error>>;
    fn register_tool(&mut self, name: &str, source: &str) -> Result<(), Box<dyn Error>>;
    fn call_tool(&mut self, name: &str, args_json: &str) -> Result<String, Box<dyn Error>>;
    fn destroy(&mut self) -> Result<(), Box<dyn Error>>;
}
```

### Toy Interpreter Architecture

The 6 future-feature runtimes share `interp_core/interp.c`, compiled into modules of 277,630–277,747 bytes each (the extra `*_wasi.wasm` builds sitting in the same directories measure 179,282–179,658 bytes but are not what the server loads). The implementation provides:
- Arena-based memory management with per-call reset
- TLV binary protocol for argument passing
- Basic tokenizer/parser/evaluator for shared DSL
- WASI stubs for host communication

The architecture is limited to the shared DSL. Its hot-call latency is not currently measured: in the 2026-09-17 correctness pass every one of these modules failed the regex tool (364–390 failed calls out of 500 attempts, `wc=0 cc=0`), so any mean the benchmark printed for them is an average over a failing run and is not usable.

---

*Last updated: 2026-09-17. Numbers from `wasm_vs_native`, `metering_cache_bench` and `comprehensive_benchmark` run that day on a Core 5 210H with wasmer 5.0.6; sizes are `ls` byte counts of the modules under `bench_tools/`. Earlier figures were dropped rather than reused.*
