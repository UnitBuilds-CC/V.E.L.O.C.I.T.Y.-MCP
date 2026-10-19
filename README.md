# V.E.L.O.C.I.T.Y. MCP Server

[![CI](https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/actions/workflows/ci.yml/badge.svg)](https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/actions/workflows/ci.yml)
[![Version](https://img.shields.io/badge/version-3.2.0-blue.svg)](https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/releases)
[![crates.io](https://img.shields.io/crates/v/velocity_mcp.svg)](https://crates.io/crates/velocity_mcp)
[![Edge Deploy](https://img.shields.io/badge/edge-ready-brightgreen.svg)](docs/edge_user_guide.md)
[![License](https://img.shields.io/badge/license-MIT%20|%20Apache%202.0-green.svg)](LICENSE)

**A high-performance Model Context Protocol server written in Rust.**

Replaces slow Node.js/Python MCP servers with a self-contained, optimized executable. Measured **3.4x-46.5x faster** than the Node.js reference implementation over NDA/shmem transport, with enterprise-grade security and production-ready features out of the box.

## Quick Start

```bash
# Install from crates.io and run (stdio server; Ctrl-D stops it)
cargo install velocity_mcp
velocity_mcp
```

Pre-built binaries are Windows-only today: the release asset `velocity_mcp.exe` resolves,
while the Linux/macOS asset names answer HTTP 404, as measured on 2026-10-10.

Configure your MCP client to point at it. Done.

**Documentation:**
- [Getting Started Guide](docs/USER_GUIDE.md)
- [Migration from Node.js MCP](docs/MIGRATION.md)
- [Client Integration](docs/CLIENT_INTEGRATION.md) (Claude Desktop, Cursor, Windsurf)
- [Performance Comparison](docs/COMPARISON.md)
- [NDA Binary Protocol](docs/NDA_PROTOCOL.md) (frame layout, TLV encoding, dispatch)
- [WASM Runtime Benchmarks](docs/COMPREHENSIVE_BENCHMARK.md)
- [Plugin Marketplace](docs/MARKETPLACE.md)
- [Edge Deployment](docs/edge_user_guide.md) (Wasmer Edge, free tier)

---

## Performance

All numbers below are the 2026-09-13 run on Core 5 210H, release build (raw capture `bench_results_core5.txt`, whose own banner stamps `VELOCITY-MCP v3.0.0`). The 2026-09-17 re-run on the same hardware reproduces the ordering and hierarchy at different single-pass figures — ping 1,761,585 r/s and 82.0x avg vs JSON/stdio, against the 800K / 57.1x in the table below — and those are carried in [docs/COMPARISON.md](docs/COMPARISON.md) with `bench_nda_2026-09-17.txt` as their capture. Both captures are local artifacts (raw benchmark output is not committed to this repo); regenerate them with `cargo run --release --manifest-path bench_nda/Cargo.toml` from the repository root.

### NDA/shmem Transport

| Method | Latency | Throughput | vs JSON/stdio |
|--------|---------|------------|---------------|
| ping | 1 µs | 800K req/s | 57.1x faster |
| tools/list (40 tools) | 7 µs | 138K req/s | 87.6x faster |
| tools/call (64B) | 3 µs | 335K req/s | 15.2x faster |

### 8-Pipeline Comparison (warm avg / P50 / P95 / P99 / throughput)

#### Ping

| Pipeline | Avg | P50 | P95 | P99 | Throughput |
|----------|-----|-----|-----|-----|------------|
| **NDA/shmem** | **1 µs** | **1 µs** | **1 µs** | **2 µs** | **800K r/s** |
| JSON/shmem | 12 µs | 12 µs | 22 µs | 51 µs | 80K r/s |
| NDA/stdio | 29 µs | 22 µs | 59 µs | 83 µs | 34K r/s |
| JSON/stdio | 71 µs | 65 µs | 117 µs | 149 µs | 14K r/s |
| Node/stdio | 55 µs | 37 µs | 119 µs | 220 µs | 18K r/s |
| JSON/HTTP* | 134 µs | 103 µs | 283 µs | 403 µs | 7.4K r/s |
| Node/HTTP | 92 µs | 79 µs | 169 µs | 261 µs | 11K r/s |
| NDA/HTTP* | 119 µs | 96 µs | 245 µs | 377 µs | 8.4K r/s |

#### tools/list (40 tools)

| Pipeline | Avg | P50 | P95 | P99 | Throughput |
|----------|-----|-----|-----|-----|------------|
| **NDA/shmem** | **7 µs** | **7 µs** | **8 µs** | **10 µs** | **138K r/s** |
| JSON/shmem | 304 µs | 274 µs | 568 µs | 709 µs | 3.3K r/s |
| NDA/stdio | 167 µs | 139 µs | 302 µs | 364 µs | 6.0K r/s |
| JSON/stdio | 635 µs | 593 µs | 973 µs | 1203 µs | 1.6K r/s |
| Node/stdio | 117 µs | 100 µs | 196 µs | 296 µs | 8.6K r/s |
| JSON/HTTP* | 606 µs | 548 µs | 1007 µs | 1378 µs | 1.7K r/s |
| Node/HTTP | 106 µs | 91 µs | 177 µs | 268 µs | 9.4K r/s |
| NDA/HTTP* | 113 µs | 94 µs | 234 µs | 357 µs | 8.8K r/s |

#### tools/call (64B payload)

| Pipeline | Avg | P50 | P95 | P99 | Throughput |
|----------|-----|-----|-----|-----|------------|
| **NDA/shmem** | **3 µs** | **3 µs** | **4 µs** | **7 µs** | **335K r/s** |
| JSON/shmem | 18 µs | 15 µs | 34 µs | 86 µs | 55K r/s |
| NDA/stdio | 30 µs | 24 µs | 58 µs | 76 µs | 33K r/s |
| JSON/stdio | 45 µs | 38 µs | 83 µs | 125 µs | 22K r/s |
| Node/stdio | 43 µs | 40 µs | 63 µs | 142 µs | 23K r/s |
| JSON/HTTP* | 151 µs | 123 µs | 321 µs | 542 µs | 6.6K r/s |
| Node/HTTP | 77 µs | 64 µs | 139 µs | 244 µs | 13K r/s |
| NDA/HTTP* | 116 µs | 87 µs | 254 µs | 337 µs | 8.6K r/s |

*Rust HTTP pipelines include 4 Axum middleware layers (auth, rate-limit, request-logger, request-validator) + CORS + body-limit. Node/HTTP is a bare `http.createServer` with no middleware. The gap on ping between Node/HTTP and JSON/HTTP is 42 µs on warm avg (134 vs 92) and 24 µs on warm p50 (103 vs 79) — attributed to the middleware stack, since server-side dispatch and TCP loopback are the same for both pipelines. On tools/list the gap nearly vanishes because response serialization dominates.

Transport is the dominant factor. Shared memory is an order of magnitude faster than stdio. Binary encoding (NDA) saves 1.5x-41.9x over JSON on the same transport.

### WASM Runtime Latency

Measured 2026-09-17 on Core 5 210H, wasmer 5.0.6, release build, `cargo bench --bench wasm_vs_native` — same `text_analyze` tool and same input compiled for each language, WASM on a persistent instance vs the native CLI:

| Language | WASM hot (µs/call) | Native hot (µs/call) | WASM vs native | WASM cold (ms) | Native cold (ms) |
|----------|--------------------|----------------------|----------------|----------------|------------------|
| **JavaScript** (QuickJS) | **15.7** (63.8K calls/s) | 68.3 (Node.js) | **4.36x** | 149.9 | 70.6 |
| **Python** (MicroPython) | **15.1** (66.0K calls/s) | 47.8 (CPython) | **3.16x** | 80.5 | 249.4 |
| **Lua** | **11.1** (90.3K calls/s) | 40.8 (Lua 5.4) | **3.69x** | 82.5 | 45.7 |
| **Go** (TinyGo) | 257.3 cached (3.9K calls/s) | not measured | — | 184.7 | not measured |

Go has no persistent instance — the module is instantiated per call. Reusing the compiled module drops the per-call cost from 191,575.0 µs to 257.3 µs, a **744.5x** cache speedup. Native Go was not installed on this machine, so no Go WASM-vs-native ratio exists.

Cold start is WASM *slower* for JavaScript (149.9 vs 70.6 ms) and Lua (82.5 vs 45.7 ms), and WASM *faster* for Python (80.5 vs 249.4 ms).

**Not re-measured this run** (no fresh numbers, so the previous table rows were dropped rather than carried over): Rust, TypeScript, Ruby, PHP, C#, Java, R, Julia, Perl.

*The in-process runtimes (QuickJS, MicroPython, Lua) use pre-compiled wrappers — tool source is compiled once at registration, then called via `call_tool` with JSON arg parsing. Ruby/mruby runs on Wasmer with stubbed setjmp/longjmp (no WASM EH).

6 additional languages use tree-walk interpreters sharing a common C core with the same control-flow, function, data-structure and JSON-I/O surface — under documented execution caps (100 000 loop iterations, 100 000 TLV arguments, and a per-call metering budget that is a compute cap, not a timeout). See [WASM Runtime Status](docs/wasm_runtime_status.md) for details.

---

## Key Features

### Dual-Protocol Execution

| Mode | Transport | Use Case |
|------|-----------|----------|
| Stdio | Standard I/O | Compatible with all MCP clients |
| HTTP/SSE | HTTP + Server-Sent Events | Web clients, REST APIs |
| WebSocket | `GET /v1/ws`, served by HTTP mode | Real-time applications |
| Shared Memory | Memory-mapped IPC | Ultra-low latency (2 µs round-trip) |

Only Stdio, HTTP/SSE and Shared Memory are `--mode` values; WebSocket is a route on the
HTTP server, not a mode of its own.

### NDA Binary Protocol

Zero-copy TLV parsing with SHA-256 Merkle integrity verification on every frame:

```
[4 bytes: magic "NMCP"]
[32 bytes: Merkle root (SHA-256)]
[1 byte: method type]
[TLV: request id]
[TLV: method-specific data]
```

- Zero-copy parsing via pointer arithmetic
- SHA-NI accelerated Merkle integrity checks
- Hybrid spin-wait for sub-microsecond event signaling
- Generation-keyed tools/list cache

### Enterprise Security

15 security layers (14 enforced, 1 defined but not yet installed):

1. Capability-based sandbox with resource limits
2. Linux seccomp filters (kernel-level syscall filtering) — defined in `src/sandbox/linux_seccomp.rs`; no production path installs them yet
3. Windows Job Object limits (memory enforcement)
4. Rate limiting with per-client tracking
5. Audit logging with JSON/CSV export
6. Input validation and path traversal protection
7. CORS restrictions and API key authentication
8. Timing-safe comparison for secrets
9. Error sanitization (prevents information leakage)
10. shell_exec injection prevention (62 Unix + 51 Windows command patterns, all distinct after the 2026-10-13 audit; matching is substring-based, so the option-grammar forms it provably cannot cover are recorded in the CHANGELOG rather than claimed closed here) — scope caveat: the guard sits in the JSON dispatcher's arm, while the binary/NDA dispatcher resolves a plugin-registered name *before* built-in matching, so a plugin that claims a built-in's name answers that call in-guest with the blocklist never consulted. No shipped manifest does this today, and the behaviour is pinned by a test rather than repaired: whether a plugin may shadow a built-in name is an open owner call)
11. SSRF protection (full RFC 1918 + IPv6 blocklist)
12. edit_file request bounds (max 1000 edits, 1MB per field, 10MB input file) — these cap the incoming call, not the file it produces: each replacement is applied to *every* occurrence and the result is written without any size check, so one request can demand a write far larger than any bound listed here (measured at 10,000x in a unit test; the arithmetic at the caps reaches terabytes). Pinned as owner call #556 rather than silently repaired, because an output-size bound would change what existing calls return
13. Merkle integrity verification (SHA-256, SHA-NI accelerated)
14. Dependency audit (zero vulnerabilities)
15. Panic catching with graceful error handling

### Production Monitoring

- Prometheus metrics (7 series: `velocity_mcp_*`) with alerting rules
- Grafana dashboard for visualization
- Health and performance endpoints

### Extensibility

- Plugin marketplace with install/update/review system
- Dynamic plugin loading without restart
- 7 WASM language runtimes (JavaScript, TypeScript, Python, Lua, Ruby, Rust, Go)
- Client SDKs in 4 languages (Rust, Python, TypeScript, Go)
- Type-safe tool registration via proc macros
- Wasmer-powered execution: metering middleware (configurable instruction limit), module caching for every runtime (measured 2026-09-17: first create vs cache hit ranges from 1.2x (TypeScript) to 29.8x (Go), 13.8x for Lua, 12.3x for QuickJS)

### Built-in Tools

| Tool | Description |
|------|-------------|
| `file_read` | Read file contents with validation |
| `file_write` | Write files with path validation |
| `shell_exec` | Execute shell commands in sandbox |
| `http_request` | HTTP requests with retry logic (requires the `oauth2` feature) |
| `list_directory` | List directory contents |
| `directory_tree` | Display directory tree structure |
| `search_files` | Search files by pattern/regex |
| `move_file` | Move/rename files |
| `create_directory` | Create directories recursively |
| `edit_file` | Apply targeted edits to files |
| `get_file_info` | File metadata (size, permissions, timestamps) |
| `bench_echo` | Echo tool for benchmarking |
| `convert_to_nda_document` | Convert files to NDA binary format |
| `convert_to_nda_tool` | Convert JSON tools to NDA binary |
| `read_nda` | Read and parse NDA documents |
| `execute_nda` | Execute NDA payloads in sandbox |

---

## Installation

### From crates.io

```bash
cargo install velocity_mcp
```

Published crates: [velocity_mcp](https://crates.io/crates/velocity_mcp) (server) · [velocity-mcp-core](https://crates.io/crates/velocity-mcp-core) (protocol core, WASM-compatible) · [velocity_mcp_macros](https://crates.io/crates/velocity_mcp_macros) (`#[mcp_tool]` proc macros) · [velocity-mcp-client](https://crates.io/crates/velocity-mcp-client) (Rust client SDK) · [velocity-mcp-edge](https://crates.io/crates/velocity-mcp-edge) (Wasmer Edge target)

### From Binary

The published release carries one asset, `velocity_mcp.exe` (from v3.1.0). The Linux and
macOS names this section used to list answer HTTP 404 — there is no v3.2.0 GitHub release,
and the release workflow's own naming is versioned, so pushing a tag is what would publish
them. See *Installation* in `docs/DEPLOYMENT.md` for the measured detail; use
`cargo install` or `docker pull unitbuilds/velocity-mcp:latest` on Linux and macOS meanwhile.

```bash
# Windows
curl -L https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/releases/latest/download/velocity_mcp.exe -o velocity_mcp.exe
```

### From Source

```bash
git clone https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP.git
cd V.E.L.O.C.I.T.Y.-MCP

# The default feature set is empty: a bare `cargo build --release` produces a
# stdio/shmem-only binary. This is the same feature set the release workflow uses.
cargo build --release --features http,database,oauth2
```

### Docker

```bash
docker pull unitbuilds/velocity-mcp:latest
docker run -p 3000:3000 unitbuilds/velocity-mcp:latest
```

---

## Usage

```bash
# Stdio mode (default, compatible with all MCP clients)
velocity_mcp --mode stdio

# HTTP mode
velocity_mcp --mode http --addr 0.0.0.0:3000

# With configuration file
velocity_mcp --config config.toml
```

| Flag | Description | Default |
|------|-------------|---------|
| `--mode <stdio\|shmem\|http>` | Protocol mode | `stdio` |
| `--config <path>` | TOML config file; CLI args override file values | — |
| `--addr <address>` | HTTP listen address (http mode only) | `0.0.0.0:3000` |
| `--buffer-path <path>` | Mapped buffer file (shmem mode only) | `nmcp_buffer.bin` |
| `--tls-cert <path>` | TLS certificate (PEM); enables HTTPS when paired with `--tls-key` | — |
| `--tls-key <path>` | TLS private key (PEM); enables HTTPS when paired with `--tls-cert` | — |
| `--benchmark` | Run the performance benchmark suite | — |
| `-h`, `--help` | Print the help screen | — |

WebSocket is not a separate mode — it is the `GET /v1/ws` route served by `--mode http`.
`--mode http` requires building with the `http` feature (see [From Source](#from-source));
the default feature set is empty, and a default build exits with
`Error: HTTP transport not enabled. Build with --features http`.

---

## Monitoring

```bash
# Prometheus metrics
curl http://localhost:3000/v1/metrics/prometheus

# Health check
curl http://localhost:3000/health

# Performance metrics (JSON)
curl http://localhost:3000/v1/performance
```

### Live Dashboard

A zero-dependency browser console to visually demo the server — live request-rate
chart, metrics polling, all 40 tools with schema-prefilled `tools/call`, measured
round-trip latency, a ping burst benchmark, and an SSE event feed:

```bash
cargo build --release --features http
target/release/velocity_mcp --mode http --config dashboard/demo.toml   # listens on :3900
# then open dashboard/index.html in a browser and click Connect
```

The demo config sets `cors_origins = ["*"]` so the page works when opened as a
local file; production default (no `cors_origins`) restricts CORS to localhost origins.
Full guide, URL parameters (`?demo=1` zero-click mode), and the headless test harness:
[docs/DASHBOARD.md](docs/DASHBOARD.md).

---

## Testing

```bash
cargo test --all-features    # All tests
cargo test --lib             # Unit tests only
cargo test --test integration # Integration tests
cargo bench                  # Benchmarks
```

---

## Client SDKs

All four clients use the same shape: build a transport, hand it to the client, initialize, then call tools. The examples target `POST /v1/mcp`; the API key is optional and only required when `[http].api_key` is configured server-side.

### Rust (`velocity-mcp-client`)
```rust
use velocity_mcp_client::{HttpTransport, McpClient};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let transport = HttpTransport::new("http://localhost:3000/v1/mcp", None)?;
    let mut client = McpClient::new(transport);
    client.initialize().await?;
    let tools = client.list_tools().await?;
    println!("{} tools", tools.len());
    Ok(())
}
```

### Python (`velocity-mcp-client`, async)
```python
import asyncio
from velocity_mcp_client import HttpTransport, McpClient

async def main():
    client = McpClient(HttpTransport("http://localhost:3000/v1/mcp"))
    await client.initialize()
    tools = await client.list_tools()
    print(f"{len(tools)} tools")

asyncio.run(main())
```

### TypeScript (`@velocity-mcp/client`)
```typescript
import { HttpTransport, McpClient } from '@velocity-mcp/client';

const client = new McpClient(new HttpTransport('http://localhost:3000/v1/mcp'));
await client.initialize();
const tools = await client.listTools();
console.log(`${tools.length} tools`);
```

### Go (`github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/sdk/go`)
```go
package main

import (
	"context"
	"fmt"
	"time"

	velocity_mcp "github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/sdk/go"
)

func main() {
	transport := velocity_mcp.NewHttpTransport("http://localhost:3000/v1/mcp", "", 30*time.Second)
	client := velocity_mcp.NewMcpClient(transport)
	defer client.Close()

	ctx := context.Background()
	if _, err := client.Initialize(ctx); err != nil {
		panic(err)
	}
	tools, err := client.ListTools(ctx)
	if err != nil {
		panic(err)
	}
	fmt.Printf("%d tools\n", len(tools))
}
```

Full API reference for each language lives in `client/README.md` (Rust) and `sdk/{python,typescript,go}/README.md`.

---

## Wasmer Edge Deployment

The existing Hyper/Tokio MCP HTTP server runs on Wasmer Edge as WASIX (`wasm32-wasmer-wasi`), including its auth, request/rate limits and CORS handling. It is built with a shared linear memory (`+atomics`), so tokio runs a genuine multi-threaded worker pool — `available_parallelism()` returns 2 on the live Edge instance (Edge caps an instance at 2 CPUs). Verified live 2026-09-17 on the public default (`https://velocity-mcp-edge.wasmer.app`): the approximately 1 MB binary serves `/health` (200), `initialize`, `tools/list` (request `id` preserved, 10 tools), repeated `echo` calls, 24–30 concurrent requests, and rejects malformed JSON. Activation is complete.

Prerequisites: Python 3, `cargo-wasix` 0.1.34, an installed rustup `+wasix` toolchain, and Wasmer CLI. From the repository root:

```bash
python deploy/build-edge.py
# Output: target/wasm32-wasmer-wasi/release/velocity-edge.wasm
```

The reproducible build uses current core/edge sources in an isolated temporary workspace, `deploy/edge-wasix.lock`, the official registry `https://cargo-registry.wasix.org/`, and `CARGO_HTTP_MULTIPLEXING=false`. It leaves the native root `Cargo.lock` and `.cargo/config.toml` untouched. Package with runner `https://webc.org/runner/wasi` and `[command.annotations.wasi]` setting `env = ["PORT=80"]`; see the guide for the full manifest and preview steps.

Plain WASI CGI remains a separate path, not the Edge HTTP build. Its earlier `wasm32-wasip1` entrypoint failed in the deployed configuration; that is not evidence that WCGI is globally broken.

| Feature | Native Binary | Wasmer Edge |
|---------|--------------|-------------|
| Transport | stdio, HTTP, shmem, NDA | HTTP only |
| Scaling | Manual | Automatic (0 to N) |
| Built-in tools (10 pure-Rust) | Yes | Yes |
| WASM plugin runtimes (7 langs) | Yes | No |
| NDA binary protocol | Yes | No |
| Shared memory IPC | Yes | No |
| Filesystem access | Full | Temp only |
| Cost model | Server cost | Pay-per-invocation |
| Free tier | N/A | ~1M invocations/month |

See [Edge User Guide](docs/edge_user_guide.md) and [Edge Operations Runbook](docs/edge_operations.md).

---

## Repository Structure

```
src/
  lib.rs              Library root
  main.rs             CLI entry point
  registry.rs         Tool registration
  sandbox.rs          Capability sandbox
  resources.rs        MCP Resources
  sampling.rs         MCP Sampling
  streaming.rs        Streaming responses
  oauth2.rs           OAuth2 framework
  audit.rs            Audit logging
  rate_limit.rs       Rate limiting
  middleware.rs        HTTP middleware
  plugins/            Plugin system + marketplace
  protocol/           JSON-RPC, NMCP binary, NDA native
  ipc/                Shared memory IPC
  transport/          HTTP/SSE/WebSocket transport
  wasm_runtime/       7 production + 6 planned WASM runtimes
crates/
  velocity-mcp-core/  Pure MCP protocol logic (WASM-compatible, no OS deps)
  velocity-mcp-edge/  Wasmer Edge HTTP server with 10 built-in tools
client/               Rust client SDK
sdk/                  Python, TypeScript, Go SDKs
macros/               Proc-macro crate (#[mcp_tool])
benches/              Criterion + comprehensive benchmarks
docs/                 Documentation
```

---

## Documentation

- [User Guide](docs/USER_GUIDE.md)
- [API Reference](docs/API.md)
- [Deployment Guide](docs/DEPLOYMENT.md)
- [Plugin Marketplace](docs/MARKETPLACE.md)
- [Migration Guide](docs/MIGRATION.md)
- [Client Integration](docs/CLIENT_INTEGRATION.md)
- [Performance Comparison](docs/COMPARISON.md)
- [NDA Binary Protocol](docs/NDA_PROTOCOL.md)
- [WASM Runtime Status](docs/wasm_runtime_status.md)
- [Comprehensive Benchmark Guide](docs/COMPREHENSIVE_BENCHMARK.md)

---

## Contributing

Contributions welcome. Open an issue or pull request on [GitHub](https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP).

## License

Licensed under either of:
- [Apache License, Version 2.0](LICENSE-APACHE)
- [MIT License](LICENSE-MIT)

at your option.

## Acknowledgments

- [Model Context Protocol](https://modelcontextprotocol.io/) - Protocol specification
- [Rust](https://www.rust-lang.org/) - Programming language
- [Tokio](https://tokio.rs/) - Async runtime
- [Axum](https://github.com/tokio-rs/axum) - Web framework
- [Wasmer](https://wasmer.io/) - WASM runtime engine
