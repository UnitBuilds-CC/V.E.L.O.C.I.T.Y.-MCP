# NMCP Binary Protocol Zero-Alloc Frame Parser

## Classification
- **Category**: Protocol / Binary Format
- **Files**: src/protocol/nmcp_binary.rs, src/protocol/nda_native.rs
- **Criticality**: High — binary frame format for high-speed shared memory drivers

## Summary

The `NmcpBinaryFrame` struct provides zero-allocation parsing of NMCP binary frames. It uses `unsafe` pointer casts to create borrowed references directly into the input buffer, achieving 459.1 ns per parse with Merkle verification (1.6x faster than JSON). In v3.0, this is a fully operational wire format — not just a reference implementation.

## Frame Format

```
┌──────────┬──────────────────┬──────────────┬─────────────────┐
│ NMCP     │ Merkle Root      │ Method Type  │ TLV Data        │
│ 4 bytes  │ 32 bytes (SHA256)│ 1 byte       │ Variable        │
└──────────┴──────────────────┴──────────────┴─────────────────┘
Offset 0   Offset 4           Offset 36      Offset 37
```

Method types: `0x01`=initialize, `0x02`=tools/list, `0x03`=tools/call, `0x04`=ping, `0x05`=logging/setLevel, `0x06`=health/check

## Parse Logic

1. Check minimum buffer size (≥ 37 bytes)
2. Validate magic signature (`NMCP` = `0x4E 0x4D 0x43 0x50`)
3. Create borrowed references via unsafe pointer cast (zero-copy)
4. Verify SHA-256 Merkle root against payload
5. Decode TLV (Type-Length-Value) method-specific data
6. Return parsed frame with lifetime tied to input buffer

## Safety Invariants

- Buffer must be ≥ 37 bytes
- First 4 bytes must be `b"NMCP"`
- All references borrow from input slice (no dangling pointers)
- Lifetime `'a` on the struct prevents use-after-free
- Merkle root verified before payload processing

## Performance

| Operation | Latency | Throughput |
|-----------|---------|------------|
| JSON-RPC parse + extract | 722.6 ns | ~1.38M req/s |
| NDA-native parse + Merkle + extract | 459.1 ns | ~2.18M req/s |
| NDA shmem raw R/W | 12.6 ns | 79.2M ops/s |

## Current Status

Fully operational in shared memory mode. The server auto-detects wire format by checking for `NMCP` magic bytes, enabling backwards compatibility with JSON-RPC hosts.
