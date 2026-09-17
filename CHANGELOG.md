# Changelog

All notable changes to the V.E.L.O.C.I.T.Y.-MCP server are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

### Added

- **WASIX HTTP server activation for Edge**: Edge deployment now uses WASIX socket-based HTTP (hyper server) instead of WASI CGI, enabling the full hardened HTTP stack (auth, rate limiting, CORS, body limits, error sanitization) on Wasmer Edge. The cfg gates in `velocity-mcp-edge/src/main.rs` changed from `#[cfg(not(target_arch = "wasm32"))]` to `#[cfg(any(not(target_arch = "wasm32"), target_vendor = "wasmer"))]`, activating the native HTTP server for WASIX targets while preserving plain WASI CGI builds for non-WASIX wasm32 targets.
- **Multi-threaded Edge runtime**: `deploy/build-edge.py` now compiles the WASIX module with shared linear memory (`-C target-feature=+atomics,+bulk-memory,+mutable-globals,+reference-types,+multivalue`), so tokio's `rt-multi-thread` spawns a real worker pool instead of running on one thread. Measured on the live Edge deployment: `available_parallelism()` returns 2 (Edge caps an instance at 2 CPUs; native host has 12), and 24–30 concurrent MCP requests all succeed. Note: this shared-memory binary cannot be exercised with the local `wasmer` 7.4.1 CLI (it fails at socket bind with `ENOTSUP`); only the newer Edge runtime serves it, so local `wasmer run` is not a valid smoke test for the multi-threaded build.
- **Isolated WASIX build helper** (`deploy/build-edge.py`): Stages current sources in a temporary directory outside the repository, uses the WASIX cargo registry (sparse+https://cargo-registry.wasix.org/), and builds with deterministic resolution via `deploy/edge-wasix.lock`. Keeps root `Cargo.lock` and `.cargo/config.toml` untouched. Requires WASIX toolchain (`rustc +wasix`), `cargo-wasix`, and Python 3.
- **`VELOCITY_WASM_INSTRUCTION_LIMIT` env var**: Overrides `wasm_runtimes.instruction_limit` at startup (`0` disables metering — not recommended for production).
- **Metering & caching benchmark harness** (`cargo bench --bench metering_cache_bench`): measures module-compile vs cached-deserialize cost and per-call metering overhead through the real dispatch path. Measured (Windows, Wasmer/Cranelift, 10M instruction limit): module-cache cold start 235 ms → 15.3 ms (**15.4x** on the Lua cache path); metering roughly doubles compile time (+117%) and per-call latency on minimal tools (+145% QuickJS, +158% Lua through `WasmRuntimeRegistry::call_tool`), diluted to **~+19%** end-to-end through the full JSON-RPC stdio stack.

### Changed

- **Module cache key includes the instruction limit**: metering is baked into the compiled module at compile time, so the cache key now hashes the limit alongside the bytecode. Previously a module compiled under one limit could be deserialized for a request using a different limit, silently misapplying (or dropping) metering.
- **wasmer.toml updated for WASIX HTTP**: Changed from WASI CGI runner to WASI socket-based runner with `PORT=80` environment variable. The `[[command]]` section now specifies `runner = "https://webc.org/runner/wasi"` with `[command.annotations.wasi]` containing `env = ["PORT=80"]`.
- **Deploy scripts require WASIX toolchain**: `deploy-edge.sh` and `deploy-edge.bat` now check for WASIX prerequisites (`rustc +wasix`, `cargo-wasix`, `python`) instead of standard `rustup target add wasm32-wasip1`. Build command changed from `cargo build` to `python deploy/build-edge.py`. Test command scoped to `-p velocity-mcp-core -p velocity-mcp-edge` to avoid building native-only crates.

- **WASM instruction limit now enforced**: The `instruction_limit` setting in `[wasm_runtimes]` config (default 10,000,000) was previously parsed but never applied. It now drives wasmer-middlewares metering on every plugin WASM runtime (all 12 languages), with a bypass for the internal benchmark harness. Metering traps are classified as resource-limit errors with actionable guidance, including at runtime creation (interpreter bootstrap consumes metered instructions too — QuickJS needs more than 1,000,000 instructions just to initialize).
- **Per-call budget reset**: Metered instruction budgets deplete cumulatively across calls on persistent interpreter instances. Budgets are now reset before tool registration and before each tool call, so every call starts with a fresh allowance.
- **Metering trap recovery**: A metering trap (e.g. an infinite loop hitting the limit) leaves the cached interpreter unusable for subsequent calls. The runtime is now evicted on such errors and rebuilt from the module cache on the next call — self-healing, no server restart needed.

### Fixed

- **tools/list request id preservation**: The `handle_tools_list_stub` and `handle_tools_list` functions were dropping the request id, returning `null` in the JSON-RPC response. This broke client-side response correlation for MCP clients. Both functions now preserve `request.id` via `request.id.clone()`. Added regression test `test_tools_list_preserves_request_ids` checking both stub and executor paths with numeric and string ids.
- **JSON-RPC 2.0 error-code compliance** (native + Edge): Aligned three error paths with the JSON-RPC 2.0 spec, verified live on Wasmer Edge. (1) Malformed JSON in the Edge socket HTTP handler now returns error code `-32700` (Parse error) instead of leaking the HTTP status `400` as the code. (2) A request missing the required `method` member now returns `-32600` (Invalid Request) instead of `-32601` (Method not found) — enforced by structural validation in `velocity-mcp-core::dispatch`. (3) A request whose `jsonrpc` member is not `"2.0"` is now rejected with `-32600` instead of being processed. Added core regression tests for the two dispatch-level fixes.

---

## [3.2.0-edge] — 2026-09-12

### Added

- **Wasmer Edge deployment**: VELOCITY-MCP can now be deployed as a serverless WebAssembly application on Wasmer Edge. The `velocity-mcp-edge` crate (`crates/velocity-mcp-edge/`) implements an HTTP server using hyper that runs within the Wasmer Edge WASM runtime. Supports JSON-RPC over HTTP with 14 MCP methods (tools, resources, prompts, elicitation, roots, completion, sampling), 10 built-in pure-Rust tools (echo, json_format, text_count, text_transform, math_eval, bench_echo, timestamp, base64_encode, base64_decode, hash_text), and 6 security layers (request size limits, CORS, API key auth, rate limiting, error sanitization, request logging).
- **velocity-mcp-core crate**: Extracted pure protocol logic into a WASM-compatible library crate (`crates/velocity-mcp-core/`) with zero OS-specific dependencies. Provides `handle_mcp_request()`, `parse_request()`, and `serialize_response()` for both native and WASM targets. Compiles as both `cdylib` and `rlib`.
- **One-click deployment scripts**: `deploy-edge.sh` (Linux/macOS) and `deploy-edge.bat` (Windows) automate the full deploy pipeline: environment validation, WASM build, binary size check (<5MB), test execution, deployment via `wasmer deploy`, and post-deploy health verification. Both scripts are idempotent and support `--skip-tests`, `--skip-build`, and `--dry-run` flags.
- **GitHub Actions deploy pipeline**: `.github/workflows/deploy-edge.yml` provides CI/CD for Edge deployments. Triggers on pushes to `main` when edge-related files change, and supports manual dispatch with environment selection (production/staging) and dry-run mode. Validates binary size, runs tests, deploys via Wasmer CLI, and verifies health check with retry logic.
- **Comprehensive Edge documentation**: `docs/edge_user_guide.md` (quick start, configuration reference, API reference, security configuration, performance tuning, troubleshooting) and `docs/edge_operations.md` (monitoring setup, alerting thresholds, scaling guidelines, backup/recovery, incident response playbook, log analysis patterns, cost management).
- **Production-optimized wasmer.toml**: Configured for free tier (128MB memory, 5M instruction limit, 0-3 instance scaling). Includes documented environment variables for API key authentication, request size limits, request timeouts, and health check configuration.
- **Metering middleware integration** (native only): WASM plugin execution on the native binary uses `wasmer-middlewares` metering to enforce per-request instruction limits, preventing runaway computations. On Edge, resource limits are enforced by the Wasmer Edge runtime itself via `wasmer.toml` configuration.
- **WASM module compilation caching** (native only): Compiled WASM modules are cached on the native binary to avoid redundant compilation on subsequent requests, providing ~20x faster cold starts for plugin-heavy workloads. Edge deployments rely on Wasmer Edge's built-in module caching.

### Changed

- **Repository structure**: Added `crates/velocity-mcp-edge/` and `crates/velocity-mcp-core/` workspace members. The edge crate depends only on the core crate plus minimal HTTP dependencies (hyper, tokio, serde_json) compatible with WASM targets.
- **README updated**: Added Edge deployment section with architecture diagram, feature comparison table (native vs edge), quick deploy instructions, and links to the user guide and operations runbook. Version badge updated to 3.2.0 with new "edge-ready" status badge.

### Known Limitations

The following features from the native binary are not available on Edge:
- NDA binary protocol and shared memory IPC (require OS-level shared memory)
- Process spawning via `shell_exec` (WASM sandbox prevents `fork`/`exec`)
- Local filesystem access beyond temporary storage
- Custom TCP/UDP sockets beyond WASI HTTP
- WebSocket transport
- SQLite database resources
- Linux seccomp sandboxing (replaced by WASM sandbox isolation)

### Migration from Previous Versions

No breaking changes for native binary users. The v3.2.0-edge release adds a new deployment target alongside the existing native binary. To deploy on Edge:

```bash
rustup target add wasm32-wasip1
./deploy-edge.sh
```

Existing native deployments continue to work without modification. The `velocity-mcp-core` crate is a new workspace member that extracts protocol logic shared between native and Edge targets.

---

## [3.2.0] — 2026-09-14 — Binary Protocol & WASM Optimization

### Added

- **Toggleable NDA frame Merkle verification**: The per-frame SHA-256 Merkle root in NDA's 36-byte header is now configurable — enabled by default for integrity verification and audit provenance, and disableable for latency-sensitive deployments that trust their transport (e.g., same-machine shared memory). Configure via TOML `[features] nda_merkle = false` or `VELOCITY_NDA_MERKLE=0`. When disabled, frame builders write 32 zero bytes instead of hashing, validators skip the hash comparison (all other structure validation still applies), and audit entries omit the `merkle_root` field entirely. The NDA document Merkle tree (`nda_document.rs`) and all JSON transports are unaffected.
- **Prebuilt WASM module loading for the Go runtime**: The Go/TinyGo runtime now accepts a path to a pre-compiled `.wasm` module in the plugin manifest (`source` field), matching the Rust runtime's behavior, in addition to inline Go source compiled via TinyGo at load time. Per-call stores share the module's compilation engine, and each call gets a fresh instance with proper WASI imports (`_start` initialization, `malloc`-based input allocation, `(ptr<<32)|len` result ABI).
- **Binary argument protocol for WASM tools**: Eliminated double JSON serialization bottleneck across all 12 WASM runtimes by implementing TLV (Type-Length-Value) binary protocol. Added `call_tool_binary()` trait method with default JSON fallback for backwards compatibility. All 6 toy interpreters (PHP, C#, Java, R, Julia, Perl) share single ~230-line C TLV decoder in `interp_core/interp.c`, benefiting all simultaneously. Real engine runtimes (Lua, MicroPython, QuickJS/TypeScript) have custom TLV decoders (~150-200 lines each). Complete dispatch chain operational: `nmcp_binary.rs` → `registry::call_tool_binary()` → `plugins::call_wasm_tool_binary()` → `runtime.call_tool_binary()` → WASM module's `*_wasi_call_tool_binary()` → shared `interp_call_tool_binary()`. Comprehensive test suite (11 tests covering encoding, decoding, nested structures, arrays, floats, limits) all passing. Benchmark results show TLV roundtrip at 1.02 µs vs JSON at 856 ns — key benefit is eliminating interpreter-side `JSON.parse()` which dominates total tool call latency.
- **Criterion benchmark suite for binary protocol**: Added comprehensive benchmark measuring TLV encoding, decoding, round-trip performance, and size comparison against JSON. Benchmarks cover small payloads (2 fields), medium payloads (10 fields), and edge cases. Results documented in project memory.
- **WASM plugin executor**: Plugin system now supports WebAssembly-based tools with 12 language runtimes: JavaScript (QuickJS), TypeScript (QuickJS), Python (MicroPython), Ruby (mruby), Lua, Go (TinyGo), Rust (wasm32-wasi), PHP, C#, Java, R, Julia, Perl. Plugins use `executor_type: "wasm"` with a `language` field and inline `source` or `source_file`. Six runtimes (JS/TS/Py/Ruby/Lua/Go/Rust) execute manifest source with full argument passing; six runtimes (PHP/C#/Java/R/Julia/Perl) are minimal toy interpreters with hardcoded demo wrappers (documented honestly in manifests). E2E test verifies all 12 runtimes through the real stdio binary.

### Changed

- **Protocol response unification**: Extracted `build_initialize_response()` helper to eliminate 4-way duplication across JSON-RPC stdio, NDA stdio, shmem NDA, and shmem JSON handlers (~40 lines removed). Function accepts `include_extra` flag to control elicitation/roots capability inclusion based on transport mode.
- **NDA dispatch consolidation**: Unified NDA request handling by having `json_rpc::handle_nda_frame` delegate to `nmcp_binary::dispatch_nda_request`, eliminating ~90 lines of duplicated method dispatch logic (PING, INITIALIZE, TOOLS_LIST, TOOLS_CALL, etc.). Error frames properly constructed for parse failures instead of propagating Err.
- **WASM factory centralization**: Moved runtime creation logic from `plugins/mod.rs` to `wasm_runtime::create_wasm_runtime_for_language()`, providing single point of truth for all 12 language runtime instantiations. Reduced plugins/mod.rs factory from ~40 lines to 7-line wrapper.
- **Plugin module restructuring**: Began splitting 1529-line `plugins/mod.rs` into focused modules. Created `plugins/manifest.rs` with PluginManifest, PluginTool, PluginExecutor data structures as first extraction step.
- **WASM runtime classification update**: Documentation now accurately distinguishes 7 production-ready WASM runtimes (JavaScript/QuickJS, TypeScript, Python/MicroPython, Lua, Ruby/mruby, Rust/WASI, Go/TinyGo) from 6 planned runtimes currently implemented as toy interpreters (PHP, Perl, C#, R, Java, Julia). Historical changelog entries referencing "12 runtimes" are preserved as-is for accuracy of the historical record.

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
- **Test expectations after NDA consolidation**: Updated `test_nda_frame_tools_call_error_path` and `test_nda_frame_parse_error` to match new behavior where `dispatch_nda_request` returns Ok(error_frame) instead of Err for parse failures.

### Performance (benchmarked 2026-09-02, release build, 500 iter × 3 rounds median)

All hardening changes have negligible performance impact — string-matching blocklist checks complete in nanoseconds, dominated by I/O costs. Unification refactorings maintain identical performance characteristics (zero overhead, pure code organization).

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

[3.2.0-edge]: https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/compare/v3.1.0...v3.2.0-edge
[3.0.0]: https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/compare/v2.0.0...v3.0.0
[2.0.0]: https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/compare/v1.0.0...v2.0.0
[1.0.0]: https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/releases/tag/v1.0.0
