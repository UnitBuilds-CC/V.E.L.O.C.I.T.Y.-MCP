# Troubleshooting & FAQ

<cite>
**Referenced Files in This Document**
- [src/main.rs](file://src/main.rs)
- [src/ipc/shmem.rs](file://src/ipc/shmem.rs)
- [src/registry.rs](file://src/registry.rs)
- [src/protocol/nmcp_binary.rs](file://src/protocol/nmcp_binary.rs)
</cite>

## Build Issues

### "error: linker 'link.exe' not found"
**Cause**: Missing MSVC build tools on Windows.
**Fix**: Install Visual Studio Build Tools with the C++ workload. The `stable-x86_64-pc-windows-msvc` target requires `link.exe`.

### Slow Release Builds
**Cause**: The release profile uses `lto = true` and `codegen-units = 1`, which are maximally optimized but slow to compile.
**Fix**: For development iteration, use `cargo build` (debug) or `cargo check` (typecheck only). Reserve `--release` for final builds.

### "unused import" or "dead code" Warnings
**Cause**: Some items are marked `#[allow(dead_code)]` intentionally (e.g., `NmcpBinaryFrame`, `set_input_len`, `get_output_len`). These are public API surface for future use.
**Fix**: No action needed — these are explicitly allowed. If you see warnings on other items, verify the code is reachable.

## Runtime Issues

### "C# core engine not found at expected path"
**Cause**: The C# NdaMcpServer.exe is not at the hardcoded path in `src/registry.rs`.
**Fix**: Build the C# NdaMcpServer project first, or update the path in `registry.rs:83` to match your local build output. The expected path is:
```
C:\Users\visse\OneDrive\Documents\Payment and Transaction Flow\Velocity\NdaMcpServer\bin\Debug\net10.0\NdaMcpServer.exe
```

### Shared Memory Deadlock (Server Not Responding)
**Cause**: The shared memory state machine is stuck. Possible causes:
- Host set `STATE_REQ_READY` but server crashed before transitioning to `STATE_PROCESSING`
- Server wrote response but host didn't read it (state stuck at `STATE_RES_READY`)
- A process was killed mid-write, leaving corrupted buffer state
**Fix**: Delete the buffer file (`nmcp_buffer.bin`) and restart both host and server. The buffer is re-initialized to `STATE_IDLE` on fresh creation.

### "Input length exceeds buffer limit"
**Cause**: The request payload exceeds the input buffer capacity (~4KB, from offset 10 to 4096).
**Fix**: Reduce the request payload size. For large tool arguments, consider splitting into multiple requests or increasing `OUTPUT_BUFFER_OFFSET` in `src/ipc/shmem.rs` (requires coordinated host changes).

### "Response length exceeds output buffer limit"
**Cause**: The tool response exceeds the output buffer capacity (~61KB, from offset 4096 to 65536).
**Fix**: The C# NdaMcpServer returned a very large result. Consider increasing `TOTAL_BUFFER_SIZE` in `src/ipc/shmem.rs`, or truncate/paginate the response.

### "Invalid NMCP magic signature"
**Cause**: The binary frame does not start with `NMCP` (bytes `0x4E 0x4D 0x43 0x50`).
**Fix**: Verify the host is writing valid NMCP binary frames. The first 4 bytes must be the ASCII string `NMCP`.

### "Buffer too small for NMCP binary frame header"
**Cause**: The input buffer is smaller than 36 bytes (minimum header: 4-byte magic + 32-byte Merkle root).
**Fix**: Ensure the host writes at least 36 bytes before the payload.

### "Parse error" in JSON-RPC Mode
**Cause**: Invalid JSON received on stdin.
**Fix**: The server returns a JSON-RPC error response with code `-32700` and continues. Verify the client is sending valid newline-delimited JSON.

### "Method not found" Error
**Cause**: The JSON-RPC method is not `initialize`, `tools/list`, or `tools/call`.
**Fix**: The server returns a JSON-RPC error with code `-32601`. Only these three methods are implemented. Notifications (requests without `id`) are silently ignored.

## Target Directory Maintenance

### Large target/ Directory
**Cause**: Accumulated build artifacts across sessions.
**Fix**: Run `cargo clean` periodically, especially before release builds. The target directory can grow to ~20GB with debug artifacts.

```powershell
cargo clean
cargo build --release
```

## FAQ

**Q: Why a single crate instead of a workspace?**
A: The server is compact (~570 LOC) with clear module boundaries. A workspace adds overhead without benefit at this scale. If the project grows significantly (new protocol backends, additional IPC mechanisms), splitting into a workspace should be considered.

**Q: Why delegate tool execution to C# instead of implementing natively?**
A: The NDA file format operations (convert, read, execute) are implemented in the C# NdaMcpServer core engine. The Rust server acts as a high-performance protocol front-end, delegating heavy-lift tool execution to the proven C# implementation. This avoids duplicating complex NDA serialization/deserialization logic.

**Q: Why is the shared memory buffer only 64KB?**
A: The 64KB buffer is sized for typical MCP request/response payloads. The input region (~4KB) handles tool call requests, and the output region (~61KB) handles tool responses. This covers the vast majority of MCP operations. For larger payloads, the buffer size can be increased by modifying `TOTAL_BUFFER_SIZE` in `src/ipc/shmem.rs`.

**Q: Why use memory-mapped files instead of named pipes or sockets?**
A: Memory-mapped files provide zero-copy access — both host and server read/write directly to the same physical pages. This eliminates kernel-to-user copies and syscall overhead. The tradeoff is that both processes must be on the same machine (no network transparency).

**Q: What happens if the server crashes during shared memory operation?**
A: The buffer file persists on disk. The host will detect that the state is stuck (not transitioning) and can reset by deleting the buffer file. On restart, the server re-initializes the buffer to `STATE_IDLE`.

**Q: Is the NMCP binary parser actually used in production?**
A: The `NmcpBinaryFrame` parser is currently marked `#[allow(dead_code)]` — it is a specification-grade reference implementation for future high-speed binary drivers. The current shared memory mode still uses JSON internally for request/response serialization. The binary parser enables future zero-allocation ingestion when the host switches to binary frame format.

**Section sources**
- [src/main.rs](file://src/main.rs)
- [src/ipc/shmem.rs](file://src/ipc/shmem.rs)
- [src/registry.rs](file://src/registry.rs)
- [src/protocol/nmcp_binary.rs](file://src/protocol/nmcp_binary.rs)
