# Changelog

All notable changes to the V.E.L.O.C.I.T.Y.-MCP server are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

[2.0.0]: https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/compare/v1.0.0...v2.0.0
[1.0.0]: https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/releases/tag/v1.0.0
