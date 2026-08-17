# Built-in Micro-Benchmark Suite

<cite>
**Referenced Files in This Document**
- [src/benchmark.rs](file://src/benchmark.rs)
- [src/protocol/nmcp_binary.rs](file://src/protocol/nmcp_binary.rs)
- [src/ipc/shmem.rs](file://src/ipc/shmem.rs)
</cite>

## Overview

The benchmark module (`src/benchmark.rs`) provides a built-in micro-benchmark suite that measures the latency of three core operations. It is invoked via `--benchmark` CLI flag and runs in-process without external dependencies (no `criterion` or `test` framework).

## Benchmarks

### 1. JSON-RPC Parse Benchmark

Measures the latency of parsing a standard MCP JSON-RPC request using `serde_json`:

```rust
let json_req = r#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"read_nda","arguments":{"ndaPath":"C:/invoices/inv-001.nda"}},"id":101}"#;
// 500,000 iterations
let val: serde_json::Value = serde_json::from_str(black_box(json_req)).unwrap();
let _method = black_box(val["method"].as_str());
```

- **Iterations**: 500,000
- **Reference result**: ~112.59 ns mean latency
- **What it measures**: Full JSON parse + field access

### 2. NMCP Zero-Alloc Binary Parse Benchmark

Measures the latency of parsing a binary NMCP frame with zero allocations:

```rust
let mut binary_buffer = Vec::new();
binary_buffer.extend_from_slice(b"NMCP");           // Magic
binary_buffer.extend_from_slice(&[0u8; 32]);        // Dummy Merkle root
binary_buffer.extend_from_slice(b"read_nda ...");   // Payload
// 1,000,000 iterations
let frame = NmcpBinaryFrame::parse(black_box(&binary_buffer)).unwrap();
let _magic = black_box(frame.magic);
```

- **Iterations**: 1,000,000
- **Reference result**: ~1.54 ns mean latency
- **What it measures**: Slice alignment + magic validation + field reference creation
- **Speedup**: 73.1x faster than JSON-RPC parsing

### 3. Shared Memory Mmapped R/W Benchmark

Measures the latency of a full write + read round-trip through the memory-mapped buffer:

```rust
let mut buffer = SharedMemoryBuffer::create_or_open(temp_shmem_path)?;
// 200,000 iterations
buffer.write_output(black_box(json_req))?;
let _input = black_box(buffer.read_input().unwrap());
```

- **Iterations**: 200,000
- **Reference result**: ~74.05 ns mean latency
- **What it measures**: Buffer write + length set + length read + buffer read + UTF-8 conversion
- **Speedup**: 1.52x faster than JSON-RPC parsing
- **Cleanup**: Temporary buffer file is deleted after benchmark

## Benchmark Methodology

### `std::hint::black_box()`

All benchmark inputs and outputs are wrapped in `black_box()` to prevent the compiler from:
- Optimizing away the loop body (dead code elimination)
- Hoisting invariants out of the loop
- Pre-computing results at compile time

### Timing

Uses `std::time::Instant` for high-resolution timing:
```rust
let start = Instant::now();
for _ in 0..iterations { /* benchmark body */ }
let duration = start.elapsed();
let avg_ns = (duration.as_nanos() as f64) / (iterations as f64);
```

### Reference Hardware

Results are calibrated on Intel Core i5-14400F. Actual numbers vary by CPU, memory speed, and system load.

## Output Format

```
============================================================
         V.E.L.O.C.I.T.Y.-MCP Performance Benchmark Suite
============================================================
Running JSON-RPC Parse Benchmark (500000 iterations)...
  Mean Latency (serde_json): 112.59 ns

Running NMCP Zero-Alloc Binary Frame Parse Benchmark (1000000 iterations)...
  Mean Latency (Zero-Alloc Binary Frame): 1.54 ns

Running Shared Memory Read/Write Operation Benchmark (200000 iterations)...
  Mean Latency (Shared Memory Mmapped R/W): 74.05 ns

============================================================
                       Summary Results
============================================================
  JSON-RPC Parse (Serde):      112.59 ns
  Mmapped Buffer R/W:          74.05 ns
  Zero-Alloc Binary Parse:     1.54 ns
  Binary Ingestion Speedup:    73.1x over JSON-RPC
============================================================
```

## Key Design Decisions

1. **No external dependencies**: Uses `std::time::Instant` instead of `criterion` to keep the binary self-contained and avoid benchmark framework overhead.
2. **`black_box()` discipline**: Essential for meaningful results on modern optimizing compilers. Without it, the compiler would eliminate the entire loop body.
3. **Separate iteration counts**: Each benchmark uses a different iteration count tuned to run for ~1-2 seconds. This balances measurement precision with total benchmark time.
4. **Temporary file for shmem**: The shared memory benchmark creates a temporary file (`temp_bench_shmem.bin`) and cleans it up after. This tests the full file creation → mmap → read/write → cleanup cycle.

**Section sources**
- [src/benchmark.rs](file://src/benchmark.rs)
