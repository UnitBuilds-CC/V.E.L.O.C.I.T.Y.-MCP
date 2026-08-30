# Troubleshooting & FAQ

<cite>
**Referenced Files in This Document**
- [src/main.rs](file://src/main.rs)
- [src/ipc/shmem.rs](file://src/ipc/shmem.rs)
- [src/registry.rs](file://src/registry.rs)
- [src/transport/http.rs](file://src/transport/http.rs)
- [docs/USER_GUIDE.md](file://docs/USER_GUIDE.md)
</cite>

## Build Issues

### "error: linker 'link.exe' not found"
**Cause**: Missing MSVC build tools on Windows.
**Fix**: Install Visual Studio Build Tools with the C++ workload. The `stable-x86_64-pc-windows-msvc` target requires `link.exe`.

### Slow Release Builds
**Cause**: The release profile uses `lto = true` and `codegen-units = 1`, which are maximally optimized but slow to compile.
**Fix**: For development iteration, use `cargo build` (debug) or `cargo check` (typecheck only). Reserve `--release` for final builds.

### Feature-gated Code Not Compiling
**Cause**: Some modules require feature flags to be enabled.
**Fix**: Use `cargo build --all-features` or specify the required features: `cargo build --features http,oauth2`.

### "unused import" Warnings on Optional Code
**Cause**: Some imports are only used when certain feature flags are enabled.
**Fix**: These are gated with `#[cfg(feature = "...")]` attributes. No action needed when building with the appropriate features.

## Runtime Issues

### "File path must be absolute"
**Cause**: A relative path was provided instead of an absolute path.
**Fix**: Provide the full absolute path (e.g., `/home/user/file.nda` or `C:\Users\me\file.nda`). Path validation is cross-platform.

### "File path contains traversal sequence '..'"
**Cause**: The provided file path contains `..` sequences, which are rejected for security.
**Fix**: Use absolute paths without directory traversal.

### Shared Memory Deadlock (Server Not Responding)
**Cause**: The shared memory state machine is stuck. Possible causes:
- Host set `STATE_REQ_READY` but server crashed before transitioning to `STATE_PROCESSING`
- Server wrote response but host didn't read it (state stuck at `STATE_RES_READY`)
- A process was killed mid-write, leaving corrupted buffer state
**Fix**: Delete the buffer file (`nmcp_buffer.bin`) and restart both host and server. The buffer is re-initialized to `STATE_IDLE` on fresh creation.

### "Input length exceeds buffer limit"
**Cause**: The request payload exceeds the input buffer capacity (~4KB, from offset 10 to 4096).
**Fix**: Reduce the request payload size. For large tool arguments, consider using HTTP mode which supports up to 1 MB requests.

### "Response length exceeds output buffer limit"
**Cause**: The tool response exceeds the output buffer capacity (~61KB, from offset 4096 to 65536).
**Fix**: Use HTTP mode for large responses, or reduce the output size.

### "Invalid NMCP magic signature"
**Cause**: The binary frame does not start with `NMCP` (bytes `0x4E 0x4D 0x43 0x50`).
**Fix**: Verify the host is writing valid NMCP binary frames. The first 4 bytes must be the ASCII string `NMCP`.

### "Parse error" in JSON-RPC Mode
**Cause**: Invalid JSON received on stdin.
**Fix**: The server returns a JSON-RPC error response with code `-32700` and continues. Verify the client is sending valid newline-delimited JSON.

### "Method not found" Error
**Cause**: The JSON-RPC method is not supported.
**Fix**: Supported methods: `initialize`, `tools/list`, `tools/call`, `ping`, `logging/setLevel`, `notifications/cancelled`, `health/check`. Notifications (requests without `id`) for unknown methods are silently ignored.

### HTTP: "Connection refused"
**Cause**: The HTTP server is not running or listening on a different port.
**Fix**: Start with `--mode http --addr 0.0.0.0:3000` and verify the port is not in use.

### HTTP: "401 Unauthorized"
**Cause**: API key authentication is enabled but no valid key was provided.
**Fix**: Include the `Authorization: Bearer <key>` header in requests.

### HTTP: "413 Payload Too Large"
**Cause**: Request body exceeds `max_request_size` (default: 1 MB).
**Fix**: Reduce request size or increase the limit in `config.toml`.

### "C# core engine not found at expected path"
**Cause**: The optional C# NdaMcpServer executable is not at the configured path (for dynamic tool hosting).
**Fix**: Set the `VELOCITY_CSHARP_PATH` environment variable to the correct path, or ignore if you only use built-in tools.

### "Rate limit exceeded"
**Cause**: Too many requests in a short period (default: 20 req/sec, burst 100).
**Fix**: Implement request throttling on the client side, or increase the rate limit in `config.toml`.

### "Sandbox violation: ..."
**Cause**: A tool attempted an operation not allowed by the sandbox (network access, filesystem outside work dir, unauthorized interpreter).
**Fix**: This is expected security behavior. Check the audit log for details. If the operation is legitimate, adjust sandbox capabilities in the configuration.

## Target Directory Maintenance

### Large target/ Directory
**Cause**: Accumulated build artifacts across sessions.
**Fix**: Run `cargo clean` periodically, especially before release builds. The target directory can grow to ~20GB with debug artifacts.

```bash
cargo clean
cargo build --release
```

## FAQ

**Q: What MCP protocol version does this server support?**
A: Protocol version `2024-11-05`. The server supports `ping`, `logging/setLevel`, `notifications/cancelled`, cursor pagination on `tools/list`, elicitation, and roots.

**Q: Can I use this server with multiple MCP clients simultaneously?**
A: In stdio mode, each client needs its own server process. In HTTP mode, multiple clients can connect simultaneously with automatic session management (up to 1000 concurrent sessions). In shared memory mode, only one host can use the buffer at a time.

**Q: Is the server compatible with Linux or macOS?**
A: Yes. The server is fully cross-platform. Linux gets additional security from seccomp syscall filters. Shared memory mode uses file-based memory mapping on all platforms. Path validation works with both Windows and Unix paths.

**Q: Why use memory-mapped files instead of named pipes or sockets?**
A: Memory-mapped files provide zero-copy access — both host and server read/write directly to the same physical pages. This eliminates kernel-to-user copies and syscall overhead. The tradeoff is that both processes must be on the same machine (no network transparency). For network access, use HTTP or WebSocket mode.

**Q: What happens if the server crashes during shared memory operation?**
A: The buffer file persists on disk. The host will detect that the state is stuck (not transitioning) and can reset by deleting the buffer file. On restart, the server re-initializes the buffer to `STATE_IDLE`.

**Q: Are all NDA operations implemented natively in Rust?**
A: Yes. Since v3.0, all NDA compile/read/execute operations run natively in Rust. The C# engine is optional and only used for dynamic tool hosting of additional C# tools.

**Q: How do I monitor the server in production?**
A: In HTTP mode, Prometheus metrics are available at `/metrics`. Pre-built Grafana dashboards and alerting rules are in `monitoring/`. OpenTelemetry tracing can be enabled with the `observability` feature flag. Health checks are available at `/health` (HTTP) or via the `health/check` JSON-RPC method (all modes).

**Q: How do I add custom tools?**
A: Three ways: (1) Use `#[mcp_tool]` proc macro for compile-time registration, (2) install a plugin via the marketplace for dynamic loading, (3) add a `Tool` entry in `src/registry.rs` for built-in tools.

**Q: What's the maximum file size that can be converted?**
A: For built-in NDA tools, file size is limited by available memory. For external engine tools, the server passes the file path without reading the file itself.

**Section sources**
- [src/main.rs](file://src/main.rs)
- [src/ipc/shmem.rs](file://src/ipc/shmem.rs)
- [src/registry.rs](file://src/registry.rs)
- [src/transport/http.rs](file://src/transport/http.rs)
- [docs/USER_GUIDE.md](file://docs/USER_GUIDE.md)
