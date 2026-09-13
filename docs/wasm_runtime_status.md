# WASM Runtime Status

## Overview

VELOCITY-MCP supports **7 production-ready WASM runtimes** using real language engines, with **6 additional languages planned** for future integration using full compiler toolchains.

## Production-Ready Runtimes (7)

These runtimes use real language engines compiled to WASM and pass all correctness tests:

| # | Language | Engine | WASM Size | Hot Call (P50) | Cold Start | Notes |
|---|----------|--------|-----------|-----------------|------------|-------|
| 1 | JavaScript | QuickJS | ~200 KB | 118.0 µs | 487.1 ms | Full ES2023 support |
| 2 | TypeScript | QuickJS + TS transpiler | ~200 KB | 118.0 µs | 373.0 ms | Type-checked via tsc |
| 3 | Python | MicroPython | ~400 KB | — | 181.5 ms | Python 3.8 subset; no repeated calls |
| 4 | Lua | Lua 5.4 | ~150 KB | 15.1 µs | 10.8 ms | Full Lua 5.4 |
| 5 | Ruby | mruby | ~500 KB | ~5 µs | ~10 ms | Ruby 3.x subset; custom WASM build |
| 6 | Rust | wasm32-wasi (native) | varies | 3.3 µs | 30.8 ms | Native compilation, fastest runtime |
| 7 | Go | TinyGo/WASM | ~300 KB | 240.3 µs* | 378.5 ms | *cached module instantiation |

### Key Characteristics

- **Fastest**: Rust at 3.3 µs P50 hot call latency
- **Smallest cold start**: Lua at 10.8 ms
- **Most compatible**: JavaScript/TypeScript with full ES2023
- **Best throughput**: Rust at 228K+ calls/sec

## Future Feature: Real Language Engines (6 Planned)

These languages currently have placeholder implementations using a shared tree-walk interpreter (`interp_core`). They are **not production-ready** and cannot execute real language syntax. Full engine integration is planned.

| Language | Current State | Planned Engine | Estimated Effort | Binary Size (est.) | Feasibility |
|----------|--------------|----------------|------------------|---------------------|-------------|
| PHP | Toy interpreter | php-wasm (Emscripten, WordPress Playground) | 1-2 weeks | 50-200 MB | High - battle-tested in production |
| Perl | Toy interpreter | zeroperl (Emscripten) | ~1 week | ~9 MB | High - WASI reactor already exists |
| C#/.NET | Toy interpreter | componentize-dotnet (NativeAOT) | 1-3 weeks | 5-20 MB | Medium - Component Model, not raw WASI |
| R | Toy interpreter | WebR fork (Emscripten) | 4-8 weeks | 40-100 MB | Low - deeply browser-coupled |
| Java | Toy interpreter | Kotlin/Wasm or TeaVM | 1-8 weeks | 3-50 MB | Low - only Kotlin targets WASI |
| Julia | Toy interpreter | None available | 12-24+ weeks | 50-100+ MB | Not feasible - research problem |

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
| Binary size | All | Real engines are 50-200 MB vs 272 KB toy interpreters |
| Cold start | All | Real engines take 100-500 ms vs 33-40 ms toy interpreters |

## Benchmark Methodology

All benchmarks run on i5-14400F, release build, Windows 10 x64, Wasmer Cranelift backend.

- **Cold start**: Fresh WASM module instantiation + initialization + first tool call
- **Hot call**: 500 repeated `call_tool()` invocations on persistent runtime instance
- **Correctness**: Verify output matches expected JSON `{word_count: 29, char_count: 159}`
- **Tool**: `text_analyze` function counting words, chars, lines in sample text
- **Percentiles**: P50, P95, P99 from sorted latency distribution
- **Runs**: 3 rounds, median reported

Run benchmarks: `cargo bench --bench comprehensive_benchmark`

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

The 6 future-feature runtimes share `interp_core/interp.c` (~176 KB WASM each), which provides:
- Arena-based memory management with per-call reset
- TLV binary protocol for argument passing
- Basic tokenizer/parser/evaluator for shared DSL
- WASI stubs for host communication

This architecture is fast (7-10 µs hot calls) but fundamentally limited to the shared DSL.

---

*Last updated: 2026-09-13*
