# Shared Memory IPC with Memory-Mapped Ring Buffer

## Classification
- **Category**: IPC / Transport
- **Files**: src/ipc/shmem.rs, src/ipc/mod.rs
- **Criticality**: High — core IPC mechanism for shared memory mode

## Summary

The `SharedMemoryBuffer` implements a 64KB memory-mapped file buffer for zero-copy IPC. It uses `memmap2::MmapMut` to map a file into memory, with a fixed layout supporting a 5-state machine protocol. In v3.0, the state machine uses `AtomicU8` with Acquire/Release ordering, and Win32 Events provide zero-poll blocking waits on Windows.

## Memory Layout

| Offset | Size | Field |
|--------|------|-------|
| 0 | 1 byte | State byte (AtomicU8) |
| 1 | 4 bytes | Input length (u32 LE) |
| 5 | 4 bytes | Output length (u32 LE) |
| 9 | 1 byte | Reserved |
| 10–4095 | ~4KB | Input request buffer |
| 4096–65535 | ~61KB | Output response buffer |

## State Machine

| Value | Name | Owner |
|-------|------|-------|
| 0 | `STATE_IDLE` | System |
| 1 | `STATE_REQ_READY` | Host |
| 2 | `STATE_PROCESSING` | Server |
| 3 | `STATE_RES_READY` | Server |
| 4 | `STATE_ERROR` | Server |

## Synchronization

- **State byte**: `AtomicU8` with Acquire/Release ordering
- **Length fields**: `SeqCst` memory fence between length and state operations
- **Windows**: `CreateEventW`/`WaitForSingleObject`/`SetEvent` for zero-poll blocking
- **Other platforms**: 100μs sleep fallback

## Wire Format Auto-Detection

The server auto-detects the wire format by checking the first 4 bytes:
- Starts with `NMCP`: NDA-native binary frame (zero-copy parse + Merkle verify)
- Anything else: JSON-RPC string (serde_json parse)

## Key API

- `create_or_open(path)` — Open/create and mmap the buffer file
- `read_input()` — Read input buffer as UTF-8 String (with bounds check)
- `write_output(&str)` — Write response to output buffer (with bounds check)
- `flush()` — Sync mmap pages to disk

## Performance

| Operation | Latency | Throughput |
|-----------|---------|------------|
| JSON-in-shmem R/W | 38.8 ns | 25.8M ops/s |
| NDA-native shmem R/W | 12.6 ns | 79.2M ops/s |

## Critical Constraints

- Input buffer max: 4,086 bytes
- Output buffer max: 61,440 bytes
- Total buffer: exactly 65,536 bytes (64KB)
- Length fields are little-endian u32, manually serialized byte-by-byte
- Cross-platform: Windows (Win32 Events), Linux, macOS (polling fallback)
