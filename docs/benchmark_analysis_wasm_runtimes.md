# WASM Runtime Benchmark Analysis (2026-09-13)

## Executive Summary

Comprehensive benchmarks of all 13 WASM runtimes confirm that **7 are production-ready** using real language engines, while **6 are toy interpreters** that cannot execute real language syntax. When tested with actual language-specific functions, all 6 return empty results (`wc=0 cc=0`).

## Production-Ready Runtimes (7/13)

All pass correctness validation with real language syntax:

| Language | Engine | Cold Start | Hot Call (avg) | P50 | P95 | P99 | Throughput |
|----------|--------|------------|-----------------|-----|-----|-----|------------|
| **Rust** | wasm32-wasi | 30.8 ms | 4.4 µs | 3.3 µs | 5.5 µs | 37.7 µs | 228K calls/s |
| **Lua** | Lua 5.4 | 10.8 ms | 17.7 µs | 15.1 µs | 32.1 µs | 83.0 µs | 56K calls/s |
| **JavaScript** | QuickJS | 487.1 ms | 132.9 µs | 118.0 µs | — | — | 7.5K calls/s |
| **TypeScript** | QuickJS+TS | 373.0 ms | 146.0 µs | 118.0 µs | 304.0 µs | 503.4 µs | 6.9K calls/s |
| **Go** | TinyGo | 378.5 ms | 240.3 µs* | — | — | — | 4.2K calls/s |
| **Python** | MicroPython | 181.5 ms | SKIP | — | — | — | — |
| **Ruby** | mruby | ~10 ms | ~5 µs | — | — | — | — |

*Go uses cached module instantiation per call (no persistent instance).

Python (MicroPython) does not support repeated calls due to state pollution; cold start includes first call only.

### Key Findings

- **Rust is fastest**: 3.3 µs P50 hot call, 228K calls/sec throughput
- **Lua has best cold start**: 10.8 ms, excellent for serverless/edge
- **JavaScript/TypeScript have highest cold starts**: 373-487 ms due to QuickJS initialization
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

- **Low-latency tools (<20 µs)**: Rust or Lua
- **Complex computations**: Rust WASI or TinyGo
- **Maximum compatibility**: JavaScript/TypeScript via QuickJS
- **Highest throughput**: NDA binary protocol with shmem transport
- **Edge/serverless**: Lua (10.8 ms cold start) or Rust (30.8 ms)

## Benchmark Methodology

- **Environment**: Windows 10 x64, i5-14400F, Wasmer Cranelift backend
- **Cold start**: Fresh WASM module instantiation + init + first tool call (median of 3 runs)
- **Hot call**: 500 repeated `call_tool()` on persistent instance (avg, P50, P95, P99)
- **Correctness**: Verify `{word_count: 29, char_count: 159}` from sample text
- **Tool**: `text_analyze` function counting words, chars, lines
- **Warmup**: 10 calls before measurement

Run: `cargo bench --bench comprehensive_benchmark`

---

*Measured 2026-09-13*
