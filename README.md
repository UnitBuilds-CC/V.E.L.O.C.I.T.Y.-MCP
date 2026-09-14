# V.E.L.O.C.I.T.Y. MCP Server

[![CI](https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/actions/workflows/ci.yml/badge.svg)](https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/actions/workflows/ci.yml)
[![Version](https://img.shields.io/badge/version-3.2.0-blue.svg)](https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/releases)
[![Edge Deploy](https://img.shields.io/badge/edge-ready-brightgreen.svg)](docs/edge_user_guide.md)
[![License](https://img.shields.io/badge/license-MIT%20|%20Apache%202.0-green.svg)](LICENSE)

**A high-performance Model Context Protocol server written in Rust.**

Replaces slow Node.js/Python MCP servers with a self-contained, optimized executable. Measured **3.4x-46.5x faster** than the Node.js reference implementation over NDA/shmem transport, with enterprise-grade security and production-ready features out of the box.

## Quick Start

```bash
# Download and run
curl -L https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/releases/latest/download/velocity_mcp -o velocity_mcp
chmod +x velocity_mcp
./velocity_mcp
```

Configure your MCP client to point at it. Done.

**Documentation:**
- [Getting Started Guide](docs/USER_GUIDE.md)
- [Migration from Node.js MCP](docs/MIGRATION.md)
- [Client Integration](docs/CLIENT_INTEGRATION.md) (Claude Desktop, Cursor, Windsurf)
- [Performance Comparison](docs/COMPARISON.md)
- [WASM Runtime Benchmarks](docs/COMPREHENSIVE_BENCHMARK.md)
- [Plugin Marketplace](docs/MARKETPLACE.md)
- [Edge Deployment](docs/edge_user_guide.md) (Wasmer Edge, free tier)

---

## Performance

All numbers measured 2026-09-13 on Core 5 210H, release build.

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

*Rust HTTP pipelines include 4 Axum middleware layers (auth, rate-limit, request-logger, request-validator) + CORS + body-limit. Node/HTTP is a bare `http.createServer` with no middleware. The ~25-40µs gap on ping between Node/HTTP and JSON/HTTP is entirely middleware overhead — server-side dispatch and TCP loopback are identical. On tools/list the gap nearly vanishes because response serialization dominates.

Transport is the dominant factor. Shared memory is an order of magnitude faster than stdio. Binary encoding (NDA) saves 1.5x-41.9x over JSON on the same transport.

### WASM Runtime Latency

All numbers measured 2026-09-13 on Core 5 210H, release build, `text_analyze` tool:

| Language | Engine | Cold Start | Hot Call (P50) | P95 | P99 |
|----------|--------|------------|-----------------|-----|-----|
| **Rust** | wasm32-wasi | 42.1 ms | **4.4 µs** | 4.6 µs | 5.0 µs |
| **JavaScript** | QuickJS | 197.7 ms | **59.5 µs** | 70.0 µs | 125.9 µs |
| **TypeScript** | QuickJS+TS | 142.7 ms | **60.1 µs** | 138.7 µs | 150.3 µs |
| **PHP** | php-wasm | 117.1 ms | **16.5 µs** | 26.7 µs | 41.5 µs |
| **Java** | Wasmtime | 98.8 ms | **16.8 µs** | 25.0 µs | 36.2 µs |
| **Julia** | Wasmtime | 102.1 ms | **18.0 µs** | 32.5 µs | 145.0 µs |
| **Perl** | zeroperl | 121.1 ms | **18.4 µs** | 28.6 µs | 84.7 µs |
| **C#** | Wasmtime | 96.5 ms | **18.6 µs** | 25.7 µs | 31.6 µs |
| **R** | Wasmtime | 106.4 ms | **18.7 µs** | 29.2 µs | 60.3 µs |
| **Lua** | Lua 5.4 | 10.9 ms | **22.0 µs** | 36.0 µs | 52.4 µs |
| **Go** | TinyGo | 695.8 ms | 328.4 µs* | 504.8 µs | 1996.0 µs |
| **Python** | MicroPython | 113.3 ms | — | — | — |

*Go uses cached module instantiation per call. Python fails on sustained hot calls (state pollution after ~10 calls).
Ruby/mruby skipped (WASM EH incompatibility with Wasmer).

6 additional languages use tree-walk interpreters sharing a common C core with full control flow, functions, data structures, and JSON I/O. See [WASM Runtime Status](docs/wasm_runtime_status.md) for details.

---

## Key Features

### Dual-Protocol Execution

| Mode | Transport | Use Case |
|------|-----------|----------|
| Stdio | Standard I/O | Compatible with all MCP clients |
| HTTP/SSE | HTTP + Server-Sent Events | Web clients, REST APIs |
| WebSocket | Bidirectional WS | Real-time applications |
| Shared Memory | Memory-mapped IPC | Ultra-low latency (2 µs round-trip) |

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

15+ security layers:

1. Capability-based sandbox with resource limits
2. Linux seccomp filters (kernel-level syscall filtering)
3. Windows Job Object limits (memory enforcement)
4. Rate limiting with per-client tracking
5. Audit logging with JSON/CSV export
6. Input validation and path traversal protection
7. CORS restrictions and API key authentication
8. Timing-safe comparison for secrets
9. Error sanitization (prevents information leakage)
10. shell_exec injection prevention (31 dangerous patterns blocked)
11. SSRF protection (full RFC 1918 + IPv6 blocklist)
12. edit_file resource bounds (max 1000 edits, 1MB per field)
13. Merkle integrity verification (SHA-256, SHA-NI accelerated)
14. Dependency audit (zero vulnerabilities)
15. Panic catching with graceful error handling

### Production Monitoring

- Prometheus metrics (20+ metrics) with alerting rules
- Grafana dashboard for visualization
- OpenTelemetry distributed tracing
- Structured JSON logging with correlation IDs
- Health and performance endpoints

### Extensibility

- Plugin marketplace with install/update/review system
- Dynamic plugin loading without restart
- 7 WASM language runtimes (JavaScript, TypeScript, Python, Lua, Ruby, Rust, Go)
- Client SDKs in 4 languages (Rust, Python, TypeScript, Go)
- Type-safe tool registration via proc macros
- Wasmer-powered execution: metering middleware, module caching (20x faster cold starts)

### Built-in Tools

| Tool | Description |
|------|-------------|
| `file_read` | Read file contents with validation |
| `file_write` | Write files with path validation |
| `shell_exec` | Execute shell commands in sandbox |
| `http_request` | HTTP requests with retry logic |
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

### From Binary

```bash
# Linux/macOS
curl -L https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/releases/latest/download/velocity_mcp -o velocity_mcp
chmod +x velocity_mcp
sudo mv velocity_mcp /usr/local/bin/

# Windows
curl -L https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/releases/latest/download/velocity_mcp.exe -o velocity_mcp.exe
```

### From Source

```bash
git clone https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP.git
cd V.E.L.O.C.I.T.Y.-MCP
cargo build --release
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
| `--mode <stdio\|http\|ws\|shmem>` | Transport mode | `stdio` |
| `--addr <address>` | HTTP listen address | `0.0.0.0:3000` |
| `--config <path>` | Configuration file | — |
| `--benchmark` | Run performance benchmarks | — |

---

## Monitoring

```bash
# Prometheus metrics
curl http://localhost:3000/metrics

# Health check
curl http://localhost:3000/health

# Performance metrics
curl http://localhost:3000/performance
```

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

### Rust
```rust
use velocity_mcp_client::Client;

let client = Client::new("http://localhost:3000");
let tools = client.list_tools().await?;
```

### Python
```python
from velocity_mcp import Client

client = Client("http://localhost:3000")
tools = client.list_tools()
```

### TypeScript
```typescript
import { Client } from 'velocity-mcp-client';

const client = new Client('http://localhost:3000');
const tools = await client.listTools();
```

### Go
```go
import "github.com/UnitBuilds-CC/velocity-mcp/client/go"

client := velocity_mcp.NewClient("http://localhost:3000")
tools, err := client.ListTools()
```

---

## Wasmer Edge Deployment

Deploy as a serverless WebAssembly application on Wasmer Edge. Zero infrastructure, automatic scaling, global CDN.

```bash
./deploy-edge.sh       # Linux/macOS
deploy-edge.bat        # Windows
```

| Feature | Native Binary | Wasmer Edge |
|---------|--------------|-------------|
| Transport | stdio, HTTP, shmem, NDA | HTTP only |
| Cold start | Instant | 100-500ms (min_instances=1 eliminates) |
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
