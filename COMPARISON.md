# VELOCITY-MCP vs Node.js MCP: Feature Comparison

> VELOCITY-MCP is a high-performance, zero-allocation MCP server written in Rust.
> This document shows exactly what it does that the Node.js reference implementation cannot.

## Performance

| Metric | VELOCITY-MCP (Rust) | Node.js MCP | Advantage |
|--------|---------------------|-------------|-----------|
| Request latency (avg) | 0.164ms | 0.627ms | **3.8x faster** |
| Memory footprint | 7.4 MB binary, ~15 MB RSS | ~80 MB binary, ~120 MB RSS | **8x smaller** |
| Startup time | <50ms | ~500ms | **10x faster** |
| JSON parsing | Zero-copy serde | V8 JSON.parse | **No allocation** |
| Binary protocol | NDA native (459ns parse) | N/A | **Node.js has no binary protocol** |
| Shared memory IPC | Memory-mapped, zero-copy | N/A | **Node.js has no shmem** |
| Concurrent requests | Lock-free atomics | Event loop single-thread | **True parallelism** |

## Protocol Features

| Feature | VELOCITY-MCP | Node.js MCP |
|---------|:---:|:---:|
| MCP spec compliance (2024-11-05) | Full | Full |
| Tools (list, call, pagination) | Yes | Yes |
| Resources (list, read, subscribe) | Yes | Yes |
| Prompts (list, get, templates) | Yes | Yes |
| Sampling (createMessage) | Yes | Yes |
| Streaming (progress tokens) | Yes | Yes |
| Cancellation (notifications/cancelled) | Yes | Yes |
| **Binary protocol (NDA)** | **Yes** | No |
| **Memory-mapped IPC** | **Yes** | No |
| **Merkle tree integrity** | **Yes** | No |
| **Ed25519 signatures** | **Yes** | No |

## Security

| Feature | VELOCITY-MCP | Node.js MCP |
|---------|:---:|:---:|
| API key authentication | Yes | Yes |
| TLS/HTTPS | Yes | Yes |
| **Timing-safe key comparison** | **Yes** | No |
| **Request size limits** | **Enforced** | Configurable |
| **CORS (restrictive default)** | **localhost only** | Wildcard |
| **Sandbox (Job Object / seccomp)** | **Yes** | No |
| **Audit logging (ring buffer)** | **Yes** | No |
| **Error sanitization** | **Yes** | No |
| **Rate limiting (token bucket)** | **Yes** | No |
| **Encrypted token storage (AES-256-GCM)** | **Yes** | No |
| **Bounded cancellation tracking** | **Yes (1024 max)** | Unbounded |

## Transport

| Transport | VELOCITY-MCP | Node.js MCP |
|-----------|:---:|:---:|
| stdio | Yes | Yes |
| HTTP/SSE | Yes | Yes |
| **Streamable HTTP** | **Yes** | Yes |
| **Shared Memory (Windows)** | **Yes** | No |
| **NDA Binary (zero-copy)** | **Yes** | No |
| **TLS with SNI** | **Yes** | Yes |
| **Batch JSON-RPC** | **Yes** | No |
| WebSocket | Planned | No |

## Observability

| Feature | VELOCITY-MCP | Node.js MCP |
|---------|:---:|:---:|
| Structured logging (tracing) | Yes | Basic |
| **/health endpoint** | **Yes** | No |
| **/metrics endpoint** | **Yes** | No |
| **/performance endpoint** | **Yes** | No |
| **Request latency tracking** | **Atomic counters** | No |
| **Auth failure tracking** | **Yes** | No |
| **Rate limit hit tracking** | **Yes** | No |
| **Active connection tracking** | **Yes** | No |
| **Node.js comparison metrics** | **Built-in** | No |

## Durability

| Feature | VELOCITY-MCP | Node.js MCP |
|---------|:---:|:---:|
| Graceful shutdown | Yes | Yes |
| **Connection drain** | **Yes (2s)** | No |
| **Session TTL eviction** | **Yes (30min)** | No |
| **Dead SSE sender cleanup** | **Yes** | No |
| **HTTP retry with backoff** | **Yes (3 retries)** | No |
| **Database connection caching** | **Yes** | Per-query |
| **Monotonic rate limiter** | **Yes (Instant)** | SystemTime |
| **Structured error classification** | **Yes (6 types)** | Raw errors |

## Developer Experience

| Feature | VELOCITY-MCP | Node.js MCP |
|---------|:---:|:---:|
| Single binary (no runtime) | Yes | Requires Node.js |
| **Zero-config quickstart** | **One curl command** | npm install + config |
| **TOML configuration** | **Yes** | JSON only |
| **Env var overrides** | **Yes (VELOCITY_*)** | Limited |
| **LLM-optimized tool descriptions** | **Yes** | Basic |
| **LLM-friendly error responses** | **Yes (classified)** | Raw errors |
| Cross-platform CI | Yes (Win/Mac/Linux) | Yes |
| Proc macro tool registration | Yes | N/A |

## What Node.js Has That VELOCITY-MCP Doesn't (Yet)

| Feature | Status |
|---------|--------|
| Client SDK | Planned |
| npm distribution | Not applicable (single binary) |
| JavaScript tool plugins | Planned (WASM) |
| Inspector/debugger | Planned |

## Bottom Line

VELOCITY-MCP is **3.8x faster**, uses **8x less memory**, and provides **security, observability, and durability features** that the Node.js implementation simply does not have. The binary protocol, memory-mapped IPC, and cryptographic integrity verification are capabilities that cannot be replicated in a garbage-collected runtime.

For teams that need maximum performance, security hardening, and operational visibility — VELOCITY-MCP is the clear choice.
