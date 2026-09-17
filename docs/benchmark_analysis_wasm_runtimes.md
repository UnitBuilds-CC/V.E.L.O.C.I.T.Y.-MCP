# WASM Runtime Benchmark Analysis (2026-09-13)

## Executive Summary

Comprehensive benchmarks of all 13 WASM runtimes confirm that **7 are production-ready** using real language engines, while **6 are toy interpreters** that cannot execute real language syntax. When tested with actual language-specific functions, all 6 return empty results (`wc=0 cc=0`).

## Production-Ready Runtimes (7/13)

All pass correctness validation with real language syntax:

| Language | Engine | Cold Start | Hot Call (avg) | P50 | P95 | P99 | Throughput |
|----------|--------|------------|-----------------|-----|-----|-----|------------|
| **Rust** | wasm32-wasi | 30.8 ms | 4.4 µs | 3.3 µs | 5.5 µs | 37.7 µs | 228K calls/s |
| **Lua** | Lua 5.4 | 257.5 ms | 47.9 µs | — | — | — | 20.9K calls/s |
| **JavaScript** | QuickJS | 519.1 ms | 116.9 µs | — | — | — | 8.6K calls/s |
| **TypeScript** | QuickJS+TS | 142.7 ms | ~117 µs | — | — | — | ~8.6K calls/s |
| **Go** | TinyGo | 378.5 ms | 240.3 µs* | — | — | — | 4.2K calls/s |
| **Python** | MicroPython | 282.5 ms | 43.6 µs | — | — | — | 23.0K calls/s |
| **Ruby** | mruby | ~10 ms | ~5 µs | — | — | — | — |

*Go uses cached module instantiation per call (no persistent instance).
All three in-process runtimes (QuickJS, MicroPython, Lua) use pre-compiled wrappers via `call_tool`.

### Key Findings

- **Rust is fastest**: 3.3 µs P50 hot call, 228K calls/sec throughput
- **MicroPython is fastest scripting runtime**: 43.6 µs/call, 23K calls/s — GC root fix resolved state pollution
- **Lua stable at 47.9 µs/call**: pre-compiled wrapper path, 20.9K calls/s
- **JavaScript/TypeScript have highest cold starts**: 142-519 ms due to QuickJS initialization
- **Go pays per-call instantiation cost**: 240 µs even with cached module compilation

## Toy Interpreters - FAILING Correctness (6/13)

All 6 share the same C interpreter core (`interp_core/interp.c`, ~272 KB each). They execute but produce zero results when given real language syntax:

| Language | Cold Start | Hot Call (avg) | P50 | P95 | P99 | Result |
|----------|------------|-----------------|-----|-----|-----|--------|
| PHP | 55.9 ms | 10.4 µs | 8.3 µs | 15.6 µs | 25.4 µs | wc=0 cc=0 |
| C# | 58.3 ms | 12.0 µs | 9.4 µs | 20.1 µs | 38.6 µs | wc=0 cc=0 |
| Java | 66.2 ms | 13.0 µs | 10.8 µs | 20.1 µs | 77.8 µs | wc=0 cc=0 |
| R | 60.4 ms | 13.0 µs | 12.2 µs | 17.4 µs | 33.2 µs | wc=0 cc=0 |
| Julia | 58.0 ms | 10.7 µs | 9.1 µs | 17.9 µs | 34.6 µs | wc=0 cc=0 |
| Perl | 61.8 ms | 14.4 µs | 11.8 µs | 27.0 µs | 76.8 µs | wc=0 cc=0 |

These are fast (8-14 µs) because they're simple C interpreters, but they cannot parse `preg_split()`, `String.Split()`, `HashMap`, `strsplit()`, or any language-specific syntax. They only support a shared DSL.

See [WASM Runtime Status](wasm_runtime_status.md) for the integration roadmap.

## Transport Protocol Benchmarks

| Protocol | Ping | tools/list | tools/call |
|----------|------|------------|------------|
| JSON-RPC stdio (Node.js) | 752.0 µs | 274.5 µs | 743.5 µs |

For NDA/shmem transport benchmarks, see [Performance Comparison](COMPARISON.md).

## Recommendations

- **Low-latency tools (<50 µs)**: Rust, MicroPython, or Lua
- **Complex computations**: Rust WASI or TinyGo
- **Maximum compatibility**: JavaScript/TypeScript via QuickJS
- **Highest throughput**: NDA binary protocol with shmem transport
- **Edge/serverless**: Rust (30.8 ms cold start) or MicroPython (282.5 ms)

## Benchmark Methodology

- **Environment**: Windows 10 x64, i5-14400F, Wasmer Cranelift backend
- **Cold start**: Fresh WASM module instantiation + init + first tool call (median of 3 runs)
- **Hot call**: 500 repeated `call_tool()` on persistent instance (avg, P50, P95, P99)
- **Correctness**: Verify `{word_count: 29, char_count: 159}` from sample text
- **Tool**: `text_analyze` function counting words, chars, lines
- **Warmup**: 10 calls before measurement

Run: `cargo bench --bench comprehensive_benchmark`

---

*Measured 2026-09-13, updated 2026-09-15 (MicroPython/Lua/QuickJS hot call numbers)*
