# Wasmer Edge: WASIX Multi-Threading Support

## Problem Statement

Wasmer Edge cannot run WASM modules that use shared memory and atomics (`--shared-memory`, `--import-memory`), which are required by the WASIX toolchain's multi-threaded target (`wasm32-wasmer-wasi`). This prevents deployment of production Rust async servers using tokio + hyper + WASIX — a growing class of workloads that competitors (Cloudflare Workers, Fermyon Spin) already support.

**Concrete repro:** VELOCITY-MCP edge server (984KB WASIX binary, tokio+hyper HTTP server) deploys successfully but returns 500 at runtime because Edge's sandbox doesn't support `wasi_thread_spawn` or shared memory backing.

## Why This Matters for Wasmer

- **Competitive gap:** Cloudflare Workers supports threaded Wasm; Fermyon Spin pushes WASI preview 2 concurrency. Edge is limited to single-threaded request-response functions.
- **Workload class:** Long-lived async servers (MCP servers, game backends, streaming APIs, WebSocket services) require threading. Without it, developers choose other platforms.
- **WASIX ecosystem alignment:** The WASIX toolchain defaults to shared memory + atomics. Every `cargo wasix build` produces a module Edge can't run. This undermines the WASIX-on-Wasmer story.
- **Revenue impact:** Threaded workloads are higher-value (longer compute, more resources). Supporting them unlocks a tier of customers currently going elsewhere.

## Technical Root Cause

Edge's instance execution model assumes single-threaded WASM:
1. No `wasi_thread_spawn` host function exposed to guest modules
2. Memory is allocated per-instance without shared memory backing
3. Instance lifecycle (scale-to-zero, timeout) has no thread cleanup path
4. Resource metering (instructions, CPU time) assumes single execution thread

The open-source Wasmer runtime *already* supports these primitives. The gap is in Edge's orchestration layer.

## Implementation Plan

### Phase 1: Runtime Thread Support (2-3 weeks)

**Goal:** Edge instances can spawn and manage WASI threads.

- Wire `wasi_thread_spawn` through Edge's instance manager
- Allocate shared memory per instance (mmap-backed, not imported)
- Thread lifecycle management: spawn, join, cleanup on request completion
- Resource accounting: aggregate CPU time across threads, track shared memory regions
- Integration with existing instance pool / scale-to-zero logic

**Validation:** Deploy VELOCITY-MCP multi-threaded WASIX binary, confirm it starts and serves requests.

### Phase 2: Safety & Isolation (1-2 weeks)

**Goal:** Threaded instances maintain Edge's security guarantees.

- Thread-local WASI capability scoping (threads inherit parent capabilities, no escalation)
- Graceful shutdown: kill all threads on timeout/scale-to-zero without resource leaks
- Panic propagation: any thread panic → clean instance teardown
- Memory limit enforcement with shared memory regions (account for shared + per-thread stacks)
- Audit: ensure no cross-instance state leakage via shared memory primitives

**Validation:** Chaos testing — thread panics, timeouts during spawn, memory pressure with threads.

### Phase 3: Observability & Metering (1 week)

**Goal:** Threaded instances are billable and debuggable.

- Per-thread instruction counting aggregated to instance-level billing
- Thread count + lifetime metrics in Edge monitoring
- Log correlation across threads within an instance (thread ID tagging)
- Health check behavior: define semantics when background threads are running

**Validation:** Billing accuracy test suite, dashboard shows thread metrics.

### Phase 4: Developer Experience (1 week)

**Goal:** Developers can deploy threaded WASM without guessing what works.

- Pre-deploy compatibility check: `wasmer deploy --check` validates module features against Edge capabilities
- Clear error messages: "This module requires shared memory/threads, which Edge does not yet support" instead of "500 Internal Server Error"
- Documentation: threading support guide, tokio+hyper+WASIX example
- Template/starter: `wasmer init --template rust-wasix-server`

**Validation:** New developer can deploy threaded WASIX server from template in <5 minutes.

## Total Estimate

**5-7 weeks** for production-quality implementation.

**Risk factors:**
- Edge's instance model may need significant refactoring if single-threaded assumption is pervasive (+2-3 weeks)
- Billing/metering integration complexity if tightly coupled to single-thread model (+1 week)
- Security review for shared memory in multi-tenant environment (variable, depends on review cycle)
- WASI threads proposal stability (Phase 2, low risk of breaking changes)

## Validation Workload: VELOCITY-MCP

VELOCITY-MCP is a production MCP (Model Context Protocol) server built with:
- **Stack:** Rust + tokio + hyper + WASIX (`wasm32-wasmer-wasi` target)
- **Binary:** 984KB optimized WASM with shared memory + atomics
- **Features:** Full HTTP server with JSON-RPC, 10 built-in tools, rate limiting, CORS, health checks
- **Native parity:** Same codebase runs natively with full multi-threading; Edge deployment should be identical
- **Build pipeline:** `cargo wasix build` with isolated workspace (documented workaround for WASIX registry conflicts)

Repository: https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP

## What Exists Today (Single-Threaded Fallback)

A single-threaded WASIX build (tokio `current_thread` flavor) deploys to Edge and works, but:
- Cannot handle concurrent requests within one instance
- Relies entirely on horizontal scaling (`max_instances`) for concurrency
- Cold start penalty per new instance (~100-500ms)
- No shared state between instances
- Does not demonstrate WASIX's value proposition over plain WASI

This fallback proves the WASIX→Edge pipeline works end-to-end. Threading support removes the remaining gap.

## Author Context

Built VELOCITY-MCP from scratch: 13 WASM language runtimes, NDA/shmem transport, binary protocol, WASIX integration, security hardening (737 tests, 88% coverage). Hit every WASIX edge case on Windows (scrt1.o packaging, registry conflicts, shared memory linking). Deep familiarity with both the WASM spec layer and practical server workload requirements.
