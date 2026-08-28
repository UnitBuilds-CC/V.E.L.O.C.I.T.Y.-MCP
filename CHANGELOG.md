# Changelog

All notable changes to the V.E.L.O.C.I.T.Y.-MCP server are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
