# Shared Memory IPC with Memory-Mapped Ring Buffer

## Classification
- **Category**: IPC / Transport
- **Files**: src/ipc/shmem.rs (117 LOC), src/ipc/mod.rs (2 LOC)
- **Criticality**: High — core IPC mechanism for shared memory mode

## Summary

The `SharedMemoryBuffer` implements a 64KB memory-mapped file buffer for zero-copy IPC. It uses `memmap2::MmapMut` to map a file into memory, with a fixed layout supporting a 5-state machine protocol for request/response coordination between server and host.

## Memory Layout

| Offset | Size | Field |
|--------|------|-------|
| 0 | 1 byte | State byte |
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

## Key API

- `create_or_open(path)` — Open/create and mmap the buffer file
- `read_input()` — Read input buffer as UTF-8 String (with bounds check)
- `write_output(&str)` — Write response to output buffer (with bounds check)
- `flush()` — Sync mmap pages to disk (`FlushViewOfFile` on Windows)

## Critical Constraints

- Input buffer max: 4,086 bytes (offset 10 to 4095)
- Output buffer max: 61,440 bytes (offset 4096 to 65535)
- Total buffer: exactly 65,536 bytes (64KB)
- Length fields are little-endian u32, manually serialized byte-by-byte
- No atomic operations — state machine relies on single-writer discipline
