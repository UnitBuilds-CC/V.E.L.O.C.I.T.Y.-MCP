# Shared Memory IPC and Zero-Copy Design

<cite>
**Referenced Files in This Document**
- [src/ipc/shmem.rs](file://src/ipc/shmem.rs)
- [src/protocol/nmcp_binary.rs](file://src/protocol/nmcp_binary.rs)
- [src/benchmark.rs](file://src/benchmark.rs)
</cite>

## Overview

The IPC subsystem provides a memory-mapped file buffer for zero-copy inter-process communication between the NMCP server and its host process. Built on the `memmap2` crate, it maps a 64KB file into the address space of both processes, allowing direct read/write access to shared memory without kernel-mediated copies.

## Memory Layout

The 64KB buffer has a fixed layout:

```
Offset      Size       Field
────────    ────       ─────
0           1 byte     State byte (0-4)
1           4 bytes    Input buffer length (u32, little-endian)
5           4 bytes    Output buffer length (u32, little-endian)
9           1 byte     (reserved/padding)
10          ~4KB       Input request buffer (offset 10 to 4095)
4096        ~61KB      Output response buffer (offset 4096 to 65535)
```

### Buffer Capacities

| Region | Offset Range | Capacity |
|--------|-------------|----------|
| Header | 0–9 | 10 bytes (state + lengths) |
| Input | 10–4095 | 4,086 bytes |
| Output | 4096–65535 | 61,440 bytes |
| **Total** | 0–65535 | **65,536 bytes (64KB)** |

## State Machine

The state byte at offset 0 implements a 5-state protocol:

| Value | Constant | Meaning |
|-------|----------|---------|
| 0 | `STATE_IDLE` | Buffer initialized, no pending request |
| 1 | `STATE_REQ_READY` | Host has written a request, server should process |
| 2 | `STATE_PROCESSING` | Server is processing (lock to prevent host writes) |
| 3 | `STATE_RES_READY` | Server has written a response, host should read |
| 4 | `STATE_ERROR` | An error occurred during processing |

### Protocol Flow

```
Host                          Server
────                          ──────
Write request to input buffer
Set input length
Set state = REQ_READY (1)
                              Poll: detect state == REQ_READY
                              Set state = PROCESSING (2)
                              Flush mmap
                              Read input buffer
                              Process request
                              Write response to output buffer
                              Set output length
                              Set state = RES_READY (3)
                              Flush mmap
Read response from output buffer
Set state = IDLE (0) [implicit on next write]
```

## SharedMemoryBuffer API

### Creation

```rust
let mut buffer = SharedMemoryBuffer::create_or_open("nmcp_buffer.bin")?;
```

Opens the file with read+write+create, sets the file length to 64KB, and maps it into memory. If the buffer is freshly created (state == 0 and input_len == 0), it initializes state to `STATE_IDLE`.

### Key Operations

| Method | Description |
|--------|-------------|
| `get_state()` | Read the state byte |
| `set_state(u8)` | Write the state byte |
| `get_input_len()` | Read input buffer length (u32 LE) |
| `set_input_len(u32)` | Write input buffer length (u32 LE) |
| `get_output_len()` | Read output buffer length (u32 LE) |
| `set_output_len(u32)` | Write output buffer length (u32 LE) |
| `read_input()` | Read input buffer as UTF-8 String |
| `write_output(&str)` | Write response string to output buffer |
| `flush()` | Force-sync memory-mapped pages to disk |

## Safety Considerations

1. **No atomic operations**: The state machine uses plain byte reads/writes, not atomic operations. This is safe because only one process writes the state byte at a time (host writes REQ_READY, server writes PROCESSING/RES_READY). There is no concurrent write contention by protocol design.

2. **Flush discipline**: `flush()` must be called after writing data and before changing state. This ensures the data is visible to the other process through the file system.

3. **Buffer overflow protection**: Both `read_input()` and `write_output()` check length bounds before accessing the buffer. Exceeding limits returns an error rather than panicking.

4. **UTF-8 assumption**: `read_input()` converts bytes to UTF-8 via `String::from_utf8()`. If the host writes invalid UTF-8, this will return an error.

## Performance

The shared memory benchmark measures 200,000 iterations of write + read through the memory-mapped buffer:
- **Mean latency**: ~74 ns per round-trip
- **Speedup**: 1.52x faster than JSON-RPC parsing alone
- The benchmark uses `black_box()` to prevent compiler optimization

## Key Design Decisions

1. **File-backed mmap vs anonymous mmap**: File-backed allows persistence across process restarts and provides a named rendezvous point for host-server discovery.
2. **Fixed layout vs dynamic**: The fixed layout avoids serialization overhead for the header. State and lengths are at known offsets — no need for a header parser.
3. **Separate input/output regions**: Prevents read-write conflicts. The host writes to input, the server reads from input. The server writes to output, the host reads from output. No overlapping access.
4. **64KB total size**: Sized for typical MCP payloads. Input region (~4KB) handles tool call requests; output region (~61KB) handles tool responses which can be larger.

**Section sources**
- [src/ipc/shmem.rs](file://src/ipc/shmem.rs)
- [src/benchmark.rs](file://src/benchmark.rs)
