# NMCP Binary Zero-Alloc Parser

<cite>
**Referenced Files in This Document**
- [src/protocol/nmcp_binary.rs](file://src/protocol/nmcp_binary.rs)
- [src/protocol/nda_native.rs](file://src/protocol/nda_native.rs)
- [src/ipc/shmem.rs](file://src/ipc/shmem.rs)
</cite>

## Overview

The NMCP binary module contains components for the shared memory polling loop and zero-allocation binary frame parsing. In v3.0, the binary protocol is fully operational — not just a reference implementation.

## Shared Memory Loop

### Flow

```rust
run_shmem_loop(buffer_path)
├── SharedMemoryBuffer::create_or_open(path)
├── Create Win32 events (Windows) or use polling fallback
└── loop:
    ├── Wait for request event / poll state == REQ_READY
    ├── SeqCst fence
    ├── set_state(STATE_PROCESSING) [Release]
    ├── flush()
    ├── read_input() → detect wire format
    ├── if starts with "NMCP":
    │   ├── NmcpBinaryFrame::parse() → zero-copy frame
    │   ├── Verify Merkle root
    │   ├── Decode TLV method type + request id + data
    │   └── Dispatch to native handler
    ├── else:
    │   ├── Parse as JSON-RPC string
    │   └── Dispatch to standard handler
    ├── write_output(response)
    ├── SeqCst fence
    ├── set_state(STATE_RES_READY) [Release]
    ├── flush()
    └── Signal response event (Windows)
```

### Error Paths

- **Binary frame parse failure**: Writes error response, sets `STATE_ERROR`, continues polling
- **JSON parse failure**: Writes error response, sets `STATE_ERROR`, continues polling
- **Merkle verification failure**: Writes error response, sets `STATE_ERROR`, continues polling
- **Unknown method**: Returns JSON-RPC error `-32601` in normal response flow

### Win32 Event Signaling

On Windows, `CreateEventW`/`WaitForSingleObject`/`SetEvent` provide zero-poll blocking waits:
- Request event: `Global\VELOCITY_NMCP_REQ_{buffer}`
- Response event: `Global\VELOCITY_NMCP_RES_{buffer}`

On other platforms, a 100μs sleep fallback is used.

## NmcpBinaryFrame Parser

### Frame Structure

```
┌──────────┬──────────────────┬──────────────┬─────────────────┐
│ NMCP     │ Merkle Root      │ Method Type  │ TLV Data        │
│ 4 bytes  │ 32 bytes         │ 1 byte       │ Variable        │
└──────────┴──────────────────┴──────────────┴─────────────────┘
Offset 0   Offset 4           Offset 36      Offset 37
```

Method types:
- `0x01` = initialize
- `0x02` = tools/list
- `0x03` = tools/call
- `0x04` = ping
- `0x05` = logging/setLevel
- `0x06` = health/check

### Zero-Allocation Design

The parser uses `unsafe` pointer casts to create references directly into the input buffer without copying:

```rust
let magic = unsafe { &*(bytes[0..4].as_ptr() as *const [u8; 4]) };
let merkle_root = unsafe { &*(bytes[4..36].as_ptr() as *const [u8; 32]) };
let method_type = bytes[36];
let tlv_data = &bytes[37..];
```

This achieves:
- **Zero heap allocations**: All references point into the original buffer
- **459.1 ns per parse** with Merkle verification (1.6x faster than JSON)
- **12.6 ns per raw shmem R/W** (3.1x faster than JSON shmem)

### TLV Encoding

Arguments use Type-Length-Value binary encoding instead of JSON string parsing:
- Request ID: TLV tag 0x01
- Tool name: TLV tag 0x02
- Arguments: TLV tag 0x03 (binary key-value pairs)

### Safety Invariants

1. **Minimum size check**: Buffer must be ≥ 37 bytes (4 magic + 32 merkle root + 1 method type)
2. **Magic validation**: First 4 bytes must equal `b"NMCP"`
3. **Merkle verification**: SHA-256 root of payload is verified against the header
4. **Lifetime binding**: The returned frame borrows from the input slice, preventing use-after-free

## Performance

**Protocol Parsing (apples-to-apples, same tool call, full field extraction):**

| Operation | Mean Latency | Throughput |
|-----------|:---:|:---:|
| JSON-RPC parse + extract | 722.6 ns | ~1.38M req/s |
| NDA-native parse + Merkle + extract | 459.1 ns | ~2.18M req/s |
| **NDA speedup** | | **1.6x faster** |

**Shared Memory Throughput:**

| Operation | Mean Latency | Throughput |
|-----------|:---:|:---:|
| JSON-in-shmem R/W | 38.8 ns | 25.8M ops/s |
| NDA-native shmem R/W | 12.6 ns | 79.2M ops/s |
| **NDA speedup** | | **3.1x faster** |

## Key Design Decisions

1. **NDA-native is production-ready**: Unlike the early v1.0 design where the binary parser was a future optimization, v3.0 uses it as a fully operational wire format in shared memory mode.
2. **Auto-detection**: The server checks for `NMCP` magic bytes to auto-detect the wire format, enabling backwards compatibility with JSON-RPC hosts.
3. **Merkle root in frame header**: The 32-byte SHA-256 Merkle root provides cryptographic integrity verification of the payload.
4. **TLV binary encoding**: Arguments use TLV encoding instead of JSON, eliminating string parsing overhead on the hot path.
5. **Win32 Events on Windows**: Zero-poll blocking waits eliminate CPU waste while maintaining instant wake-up.

**Section sources**
- [src/protocol/nmcp_binary.rs](file://src/protocol/nmcp_binary.rs)
- [src/protocol/nda_native.rs](file://src/protocol/nda_native.rs)
