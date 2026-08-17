# NMCP Binary Zero-Alloc Parser

<cite>
**Referenced Files in This Document**
- [src/protocol/nmcp_binary.rs](file://src/protocol/nmcp_binary.rs)
- [src/ipc/shmem.rs](file://src/ipc/shmem.rs)
</cite>

## Overview

The NMCP binary module (`src/protocol/nmcp_binary.rs`) contains two components:
1. **Shared memory polling loop** — The `run_shmem_loop()` function that drives the server in `--mode shmem`
2. **Zero-allocation binary frame parser** — The `NmcpBinaryFrame` struct for future high-speed binary drivers

## Shared Memory Loop

### Flow

```rust
run_shmem_loop(buffer_path)
├── SharedMemoryBuffer::create_or_open(path)
└── loop:
    ├── get_state() → check for STATE_REQ_READY
    ├── set_state(STATE_PROCESSING) → lock buffer
    ├── flush() → sync to disk
    ├── read_input() → parse JSON-RPC from input region
    ├── match method:
    │   ├── "tools/list" → registry::get_tools()
    │   ├── "tools/call" → registry::call_tool()
    │   └── _ → error -32601
    ├── write_output(response) → write to output region
    ├── set_state(STATE_RES_READY) → signal host
    └── flush() → sync to disk
```

### Error Paths

- **JSON parse failure**: Writes error response, sets `STATE_ERROR`, continues polling
- **Memory/IO error**: Writes error response, sets `STATE_ERROR`, continues polling
- **Unknown method**: Returns JSON-RPC error `-32601` in normal response flow

### Adaptive Backoff

When the state is not `STATE_REQ_READY`, the loop sleeps for 100 microseconds:

```rust
thread::sleep(Duration::from_micros(100));
```

This prevents CPU pegging while maintaining sub-millisecond response latency. At 100μs intervals, the server can detect a new request within ~50μs on average.

## NmcpBinaryFrame Parser

### Frame Structure

```
┌──────────┬──────────────────┬─────────────────┐
│ NMCP     │ Merkle Root      │ Payload         │
│ 4 bytes  │ 32 bytes         │ Variable        │
└──────────┴──────────────────┴─────────────────┘
Offset 0   Offset 4           Offset 36
```

### Zero-Allocation Design

The parser uses `unsafe` pointer casts to create references directly into the input buffer without copying:

```rust
let magic = unsafe { &*(bytes[0..4].as_ptr() as *const [u8; 4]) };
let merkle_root = unsafe { &*(bytes[4..36].as_ptr() as *const [u8; 32]) };
let payload = &bytes[36..];
```

This achieves:
- **Zero heap allocations**: All references point into the original buffer
- **~1.54 ns per parse**: 73x faster than JSON-RPC parsing
- **600+ million frames/second**: Single-thread throughput

### Safety Invariants

1. **Minimum size check**: Buffer must be ≥ 36 bytes (4 magic + 32 merkle root)
2. **Magic validation**: First 4 bytes must equal `b"NMCP"`
3. **Lifetime binding**: The returned `NmcpBinaryFrame<'a>` borrows from the input slice, preventing use-after-free

### Current Status

The `NmcpBinaryFrame` is marked `#[allow(dead_code)]` — it is a specification-grade reference implementation. The current shared memory mode uses JSON internally. The binary parser enables future zero-allocation ingestion when the host process switches to binary frame format.

## Key Design Decisions

1. **JSON-in-shmem for now**: The shared memory mode serializes JSON into the memory-mapped buffer. This provides the zero-copy transport benefit while maintaining compatibility with the existing JSON-RPC tool dispatch.
2. **Separation of concerns**: The binary parser is independent of the IPC mechanism. It can be used with any byte buffer (files, network, pipes).
3. **Merkle root in frame header**: The 32-byte SHA-256 Merkle root provides cryptographic integrity verification of the payload. This is essential for the NDA security model.
4. **100μs polling interval**: Chosen as the sweet spot between CPU usage and latency. Shorter intervals waste CPU on no-ops; longer intervals add unnecessary latency.

**Section sources**
- [src/protocol/nmcp_binary.rs](file://src/protocol/nmcp_binary.rs)
