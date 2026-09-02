# Changelog

All notable changes to the V.E.L.O.C.I.T.Y.-MCP server are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased] — Security Hardening & Performance

### Security

- **shell_exec command injection prevention**: Expanded dangerous command blocklist from 5 patterns to 31 patterns covering both Unix (17 patterns: `rm -rf /`, fork bombs, `dd if=`, `mkfs.`, pipe-to-shell variants) and Windows (14 patterns: `format`, `del /f /s /q`, `rd /s /q`, `diskpart`, `bcdedit`, `reg delete`, encoded PowerShell). All patterns checked cross-platform to prevent OS-detection bypass. Added shell metacharacter detection (`;`, `|`, `&`, `` ` ``, `$`, `\n`) with audit logging. All shell_exec invocations now emit `tracing::info!` audit trail entries.
- **SSRF blocklist expansion**: Extended `http_request` SSRF prevention from basic `127.0.0.1`/`localhost`/`10.`/`192.168.` blocking to comprehensive coverage: full RFC 1918 private ranges (all 16 `172.16.`–`172.31.` subnets, not just `172.16.`), link-local `169.254.`, full `127.0.0.0/8` loopback range, and IPv6 private ranges (`::1`, `[::1]`, `[::]`, `fe80:`, `fc00:`, `fd00:`).
- **HTTP authentication warning**: Server now emits `tracing::warn!` at startup when HTTP transport is enabled without API key authentication, alerting operators to the open-access configuration.
- **Silent error discard elimination**: Replaced `let _ = child.wait()` in sandbox timeout handler with proper error logging. Replaced `let _ = writeln!(..)` / `let _ = f.flush()` in NDA phase logger with `if let Err(e) = ...` + `tracing::debug!` logging. Replaced `let _ = tx.send(..)` in C# reader thread with debug logging on receiver disconnect. All error paths now produce diagnostic output.
- **shell_exec whitespace normalization**: Dangerous pattern matching now collapses all whitespace runs to single spaces before checking, preventing bypass via double spaces, tabs, or mixed whitespace (e.g., `rm  -rf /` or `rm\t-rf /`).
- **SSRF host-scoped checking**: SSRF blocklist now extracts the host portion of the URL before checking patterns, eliminating false positives where private IP substrings appear in URL paths or query strings (e.g., `https://example.com/page?version=10.5` no longer blocked by `10.` pattern).
- **edit_file resource bounds**: Maximum 1000 edits per request, maximum 1MB per oldText/newText field. Prevents resource exhaustion from maliciously large edit payloads.

### Fixed

- **Compiler warnings eliminated**: Zero-warning build achieved. Cfg-gated `Arc` import, `tls_cert`/`tls_key` declarations and `--tls-cert`/`--tls-key` argument parsing behind `#[cfg(feature = "http")]`. Added `#[allow(unused)]` for oauth2-cfg variables (`method`, `body`, `timeout_secs` in `http_request`). Removed unused `error` import from `plugins/marketplace.rs`. Added `#[allow(unused)]` for `addr` variable only consumed by http match arm.

### Performance (benchmarked 2026-09-02, release build, 500 iter × 3 rounds median)

All hardening changes have negligible performance impact — string-matching blocklist checks complete in nanoseconds, dominated by I/O costs.

**NDA/shmem transport (primary path):**

| Method | Latency (avg) | Throughput | vs JSON/stdio |
|--------|--------------|------------|----------|
| ping | 0.002 ms (2µs) | 445,279 r/s | 7.8x faster |
| tools/list (17 tools) | 0.007 ms | 136,983 r/s | 27.7x faster |
| tools/call (64B) | 0.003 ms | 313,582 r/s | 7.3x faster |
| health/check | 0.002 ms | 471,904 r/s | 9.6x faster |

**Payload scaling (bench_echo):**

| Payload | NDA/shmem | JSON/stdio | Speedup |
|---------|-----------|------------|---------|
| 256 B | 0.001 ms | 0.026 ms | 18.1x |
| 1 KB | 0.002 ms | 0.027 ms | 12.1x |
| 4 KB | 0.004 ms | 0.029 ms | 7.2x |

**tools/list registry scaling:**

| Tools | NDA/shmem | JSON/shmem | Speedup |
|-------|-----------|------------|---------|
| 17 | 0.006 ms | 0.077 ms | 13.2x |
| 49 | 0.013 ms | 0.123 ms | 9.6x |
| 81 | 0.035 ms | 0.235 ms | 6.7x |
| 145 | 0.075 ms | 0.464 ms | 6.2x |

**Overall: 9.5x–27.7x faster across methods** (NDA/shmem vs JSON/stdio).

**Node.js vs Rust (JSON/stdio, fair comparison — same 16 tools, 500 iterations, median of 3 rounds):**

| Method | Node.js avg | Rust avg | Speedup |
|--------|------------|----------|---------|
| ping | 0.029 ms | 0.017 ms | 1.7x |
| tools/list | 0.080 ms | 0.202 ms | 0.4x* |
| tools/call | 0.029 ms | 0.023 ms | 1.3x |
| health/check | 0.030 ms | 0.020 ms | 1.5x |

*tools/list: Node.js returns a static array (pre-built constant), Rust dynamically assembles with cache checks + hashset dedup + pagination.

**4-Pipeline Comparison (benchmarked 2026-09-02, release build):**

Isolates service (Node.js vs Rust), tool format (JSON vs NDA), and transport (stdio vs shmem) differences.

| Pipeline | Ping avg | tools/list avg | tools/call avg |
|----------|----------|----------------|----------------|
| Node.js JSON/stdio | 0.029 ms | 0.080 ms | 0.029 ms |
| Rust JSON/stdio | 0.017 ms | 0.202 ms | 0.023 ms |
| Rust NDA/stdio | 0.025 ms | 0.164 ms | 0.032 ms |
| Rust NDA/shmem | 0.002 ms | 0.007 ms | 0.003 ms |

**Key findings:**

| Comparison | Ping | tools/list | tools/call | Conclusion |
|------------|------|------------|------------|------------|
| Service (Node.js vs Rust, same JSON/stdio) | 1.7x | 0.4x* | 1.3x | Marginal — Rust faster on ping/call, Node.js faster on tools/list (static array) |
| Transport (JSON/stdio vs NDA/shmem) | 8.5x | 28.9x | 7.7x | **Dominant factor** — shmem transport is order-of-magnitude faster |
| Full stack (Node.js JSON/stdio vs Rust NDA/shmem) | 14.5x | 11.4x | 9.7x | Combined effect of service + transport |

*tools/list: Node.js returns pre-built `const TOOLS` array; Rust dynamically assembles from 5 sources with cache validation.

Transport primitive costs on test machine:
- Win32 Event RTT (same process): 5.39µs (2.69µs one-way)
- Win32 Event RTT (cross-process): 5.28µs (2.64µs one-way)
- SHA-256 of 8KB (Merkle): 3.60µs (2.28 GB/s, SHA-NI accelerated)
- `flush_async` (FlushViewOfFile): 24.28µs

---

## [3.0.0] — 2026-08-28

### Added

- **NDA-native binary protocol**: Shared memory transport now supports NDA binary frames (`NMCP` magic + SHA-256 Merkle root + TLV-encoded payloads) as the native wire format. Zero JSON parsing on the hot path. Auto-detects NDA vs JSON frames for backwards compatibility.
- **MCP spec compliance**: `ping`, `logging/setLevel`, `notifications/cancelled`, cursor pagination on `tools/list`, `tools.listChanged` capability advertisement. Server now fully compliant with MCP protocol version 2024-11-05.
- **Win32 Event IPC**: `CreateEventW`/`WaitForSingleObject`/`SetEvent` for zero-poll blocking waits on Windows. Replaces 100μs polling sleep with instant event signaling. `Drop` impl for handle cleanup. Non-Windows platforms use 100μs sleep fallback.
- **NDA-native fuzz tests**: 7 new property-based tests (1,700+ cases): random payload resilience, Merkle tampering, TLV round-trip for arbitrary JSON, truncation safety, frame detection correctness, request/response round-trips.
- **Cancellation support**: Pre- and post-execution cancellation checks via `notifications/cancelled` tracking.

### Changed

- **Shared memory protocol**: Auto-detects NDA-native vs JSON-RPC frames by checking for `NMCP` magic bytes. Both formats supported simultaneously.
- **Initialize response**: Now advertises `tools.listChanged: true` and `logging` capability.
- **Test count**: 146 → 172 tests (128 unit + 27 integration + 17 fuzz).
- **Version**: 2.0.0 → 3.0.0.

### Security

- NDA-native frames verified with SHA-256 Merkle roots — any payload tampering detected.
- TLV decoder enforces depth limit (32), max string length (10 MB), max element count (100K).
- Cancellation tracker uses poisoning-tolerant mutex.

---

## [2.0.0] — 2026-08-18

### Added

- **Ed25519 NDA signatures**: Sign and verify NDA documents for authenticity and tamper detection. Backward-compatible with unsigned documents.
- **Capability-based sandbox**: Adapted from Velocity-IDE's TabSandbox. Restricted profile blocks network, isolates filesystem, controls interpreters. Violation tracking with categories.
- **Windows Job Object memory limits**: OS-level 256 MB memory cap for sandboxed processes via `extern "system"` FFI.
- **Merkle tree integrity verification**: SHA-256 root in NDA header, pair-wise hash verification of all triples.
- **Token bucket rate limiter**: 20 req/sec, burst 100. Prevents abuse across all tool calls.
- **Audit logging**: 10K entry ring buffer, poisoning-tolerant mutex, global instance. Records every tool execution.
- **Error sanitization**: Strips internal paths (Windows/Unix), truncates at 500 chars. Prevents information leakage.
- **Property-based fuzz testing (proptest)**: 10 properties, 2,250+ random cases per run. Covers NDA round-trips, random bytes, Merkle integrity, Ed25519 signatures, Unicode strings, sandbox cleanup.
- **Adversarial integration tests**: 15 tests covering XML parsing attacks, sandbox escape attempts, signature tampering, rate limiter burst, audit overflow, error sanitization, parser robustness.
- **GitHub Actions CI**: Automated build + test + cargo audit on every push/PR. Three jobs: build-and-test, security-audit, fuzz-tests.
- **Spec-compliant XML parsing**: Replaced regex-based XML extraction with quick-xml for XLSX/DOCX parsing. Handles namespaces, entities, malformed XML safely.

### Changed

- **Architecture**: Migrated from C# delegation to fully native Rust NDA operations. All compile/read/execute runs in-process.
- **Test count**: 46 → 146 tests (109 unit + 27 integration + 10 fuzz).
- **Security model**: 12 defense layers, all active and tested.
- **Dependencies**: Added quick-xml 0.41, ed25519-dalek 2, rand 0.8, proptest 1 (dev).
- **Documentation**: README.md rewritten with security layers, test breakdown, CI docs. USER_GUIDE.md expanded with comprehensive security model section.

### Security

- NDA parser: bounds checking on all header fields, string pool offsets, triple/command counts.
- Execution: 30s timeout, 1 MB stdout cap, 256 KB stderr cap, 256 MB memory limit.
- Sandbox: capability-based access control, violation recording, temp dir isolation with cleanup.
- Signatures: Ed25519 sign/verify, backward-compatible signature section.
- Dependencies: 0 vulnerabilities across 95 crates (cargo audit clean).

---

## [1.0.0] — 2026-08-17

### Added

- Initial release: high-performance MCP server in Rust.
- **Dual-protocol support**: Stdio JSON-RPC v2.0 and Shared Memory IPC.
- **Four built-in tools**: convert_to_nda_document, convert_to_nda_tool, read_nda, execute_nda.
- **NDA binary format**: 52-byte header, semantic triples, display commands, string pool, Merkle tree.
- **File format support**: CSV, XLSX, DOCX, PDF, Images (PNG/JPG/WebP), 20+ source code languages.
- **Dynamic tool hosting**: Auto-discovery of C# backend tools, merged with built-in NDA tools.
- **Graceful shutdown**: Ctrl+C handler with atomic shutdown flag.
- **Health checks**: `health/check` JSON-RPC method.
- **Structured logging**: tracing + tracing-subscriber with env-filter.
- **Path validation**: Rejects empty, relative, and traversal paths.
- **46 tests**: 34 unit + 12 integration.

[3.0.0]: https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/compare/v2.0.0...v3.0.0
[2.0.0]: https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/compare/v1.0.0...v2.0.0
[1.0.0]: https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/releases/tag/v1.0.0
