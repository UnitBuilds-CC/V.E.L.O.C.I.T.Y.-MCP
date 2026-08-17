# NMCP Binary Protocol Zero-Alloc Frame Parser

## Classification
- **Category**: Protocol / Binary Format
- **Files**: src/protocol/nmcp_binary.rs (141 LOC)
- **Criticality**: High — defines the binary frame format for future high-speed drivers

## Summary

The `NmcpBinaryFrame` struct provides zero-allocation parsing of NMCP binary frames. It uses `unsafe` pointer casts to create borrowed references directly into the input buffer, achieving ~1.54 ns per parse (73x faster than JSON-RPC).

## Frame Format

```
┌──────────┬──────────────────┬─────────────────┐
│ NMCP     │ Merkle Root      │ Payload         │
│ 4 bytes  │ 32 bytes (SHA256)│ Variable        │
└──────────┴──────────────────┴─────────────────┘
Offset 0   Offset 4           Offset 36
```

## Parse Logic

1. Check minimum buffer size (≥ 36 bytes)
2. Validate magic signature (`NMCP` = `0x4E 0x4D 0x43 0x50`)
3. Create borrowed references via unsafe pointer cast (zero-copy)
4. Return `NmcpBinaryFrame<'a>` with lifetime tied to input buffer

## Safety Invariants

- Buffer must be ≥ 36 bytes
- First 4 bytes must be `b"NMCP"`
- All references borrow from input slice (no dangling pointers)
- Lifetime `'a` on the struct prevents use-after-free

## Current Status

Marked `#[allow(dead_code)]` — specification-grade reference implementation for future binary drivers. Current shared memory mode uses JSON internally.
