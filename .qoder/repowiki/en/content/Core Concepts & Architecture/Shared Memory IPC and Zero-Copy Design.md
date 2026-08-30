# Shared Memory IPC and Zero-Copy Design

<cite>
**Referenced Files in This Document**
- [src/ipc/shmem.rs](file://src/ipc/shmem.rs)
- [src/protocol/nmcp_binary.rs](file://src/protocol/nmcp_binary.rs)
- [src/protocol/nda_native.rs](file://src/protocol/nda_native.rs)
- [src/benchmark.rs](file://src/benchmark.rs)
</cite>

## Overview

The IPC subsystem provides a memory-mapped file buffer for zero-copy inter-process communication between the MCP server and its host process. Built on the `memmap2` crate, it maps a 64KB file into the address space of both processes, allowing direct read/write access to shared memory without kernel-mediated copies.

In v3.0, the shared memory mode supports **two wire formats** with auto-detection:
- **NDA-native** (binary): Zero JSON parsing on the hot path
- **JSON-RPC** (backwards-compatible): Standard JSON-RPC strings

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
SeqCst fence
Set state = REQ_READY (1) [Release]
Signal request event (Windows)
                              Wait for request event (Windows)
                              / Poll: detect state == REQ_READY [Acquire]
                              SeqCst fence
                              Set state = PROCESSING (2)
                              Flush mmap
                              Read input buffer
                              Auto-detect wire format (NMCP magic vs JSON)
                              Process request
                              Write response to output buffer
                              Set output length
                              SeqCst fence
                              Set state = RES_READY (3) [Release]
                              Flush mmap
                              Signal response event (Windows)
Wait for response event
Read response from output buffer
Set state = IDLE (0) [implicit on next write]
```

## Win32 Event Signaling

On Windows, the server uses `CreateEventW`/`WaitForSingleObject`/`SetEvent` for zero-poll blocking waits. Events are named `Global\VELOCITY_NMCP_REQ_{buffer}` and `Global\VELOCITY_NMCP_RES_{buffer}`. This replaces polling with instant wake-up.

On other platforms, a 100μs sleep fallback is used.

## Synchronization Requirements

Cross-process correctness requires proper memory ordering:

- **State byte**: Atomic load/store with Acquire/Release ordering
- **Length fields**: `SeqCst` memory fence between reading/writing length fields and checking/setting the state byte
- **Writer sequence**: Write data → write length → `SeqCst` fence → set state (Release)
- **Reader sequence**: Read state (Acquire) → `SeqCst` fence → read length → read data

## Wire Format Auto-Detection

The server detects the wire format by checking the first 4 bytes of the input buffer:

- **Starts with `NMCP`**: NDA-native binary frame — parsed with zero-copy pointer casts, TLV decoding, and Merkle verification
- **Anything else**: JSON-RPC string — parsed with serde_json

This enables backwards compatibility while allowing hosts to opt into the faster binary protocol.

## Performance

| Operation | Mean Latency | Throughput |
|-----------|:---:|:---:|
| JSON-in-shmem R/W | 38.8 ns | 25.8M ops/s |
| NDA-native shmem R/W | 12.6 ns | 79.2M ops/s |
| **NDA speedup** | | **3.1x faster** |

Concurrency (NDA-native dispatch):

| Threads | Throughput |
|:-------:|:----------:|
| 1 | 2.86M req/s |
| 4 | 7.01M req/s |
| 8 | 12.7M req/s |

## Key Design Decisions

1. **File-backed mmap vs anonymous mmap**: File-backed allows persistence across process restarts and provides a named rendezvous point for host-server discovery.
2. **Fixed layout vs dynamic**: The fixed layout avoids serialization overhead for the header. State and lengths are at known offsets — no need for a header parser.
3. **Separate input/output regions**: Prevents read-write conflicts. The host writes to input, the server reads from input. The server writes to output, the host reads from output.
4. **Dual wire format**: NDA-native for maximum performance, JSON-RPC for backwards compatibility. Auto-detection means hosts can migrate at their own pace.
5. **Win32 Events on Windows**: Zero-poll blocking waits eliminate CPU waste while maintaining instant wake-up. Cross-platform fallback uses short sleep intervals.

**Section sources**
- [src/ipc/shmem.rs](file://src/ipc/shmem.rs)
- [src/protocol/nmcp_binary.rs](file://src/protocol/nmcp_binary.rs)
- [src/benchmark.rs](file://src/benchmark.rs)
