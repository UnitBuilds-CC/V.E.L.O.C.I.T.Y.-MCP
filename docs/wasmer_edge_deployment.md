# Wasmer Edge Deployment Strategy for VELOCITY-MCP

## Overview

Wasmer Edge is Wasmer's serverless platform designed for WebAssembly workloads. It provides automatic scaling, persistent volumes, and cronjob support with sub-millisecond cold starts.

## Key Features

### Performance
- **Cold Start Time**: <1ms (WASM modules load instantly without container overhead)
- **Auto-scaling**: Scales to zero when idle, scales up automatically under load
- **Global CDN**: Deployments distributed across edge locations worldwide

### Deployment Modes
1. **Proxy Mode**: Direct HTTP forwarding to WASM module
2. **WCGI Mode** (WebAssembly CGI): WASM handles HTTP requests/responses natively

### Infrastructure
- Automatic SSL/TLS certificate management
- Persistent volumes for stateful applications
- Cronjob scheduling for periodic tasks
- Built-in monitoring and metrics

## Technical Requirements

### WASM Module Format
VELOCITY-MCP's 12 language runtimes must be compiled as:
- WASI-compatible WASM modules
- Export `_start` or HTTP handler function
- Maximum module size: ~50MB per runtime

### Current Compatibility Assessment

**Directly Compatible** (already WASI-based):
- Rust/WASM (wasm32-wasi target)
- Lua/WASI reactor
- MicroPython/WASI reactor
- QuickJS/WASI reactor
- Ruby/mruby (with WASI imports)
- PHP/WASI
- Perl/WASI
- Java/TeaVM (with WASI polyfill)
- R/WebR (with WASI networking)
- Julia/WASI
- C#/.NET (with WASI runtime)
- Go/TinyGo (wasm32-wasi)

All 12 runtimes already use WASI imports via `build_wasi_imports()`, making them compatible with Wasmer Edge.

## Deployment Architecture Options

### Option 1: Full Server Migration (Recommended for Cloud Mode)
Deploy entire VELOCITY-MCP server as WASM module on Wasmer Edge:
- Compile Rust binary to `wasm32-wasip1` target
- Replace Axum HTTP server with WASI-HTTP handler
- Use Wasmer Edge volumes for plugin storage and audit logs
- Leverage built-in metering for resource limits

**Pros**:
- Zero infrastructure management
- Automatic scaling from 0 to thousands of instances
- Pay-per-execution pricing model
- Global distribution out of the box

**Cons**:
- Requires significant refactoring of HTTP transport layer
- Loss of NDA/shmem optimizations (local-only features)
- Limited access to OS-level features (process spawning, raw sockets)

### Option 2: Hybrid Two-Tier Architecture (Recommended)
Keep current Rust native server for local/NDA deployments, add WASM cloud tier:
- Maintain existing stdio/shmem/NDA transports for local use
- Build separate WASM module for HTTP/SSE transport only
- Deploy WASM HTTP gateway on Wasmer Edge
- Route tool execution to either local plugins or cloud WASM runtimes

**Pros**:
- Preserves all local performance optimizations
- Adds cloud deployment capability without sacrificing local features
- Can share WASM runtime code between local and cloud deployments
- Metering integration (#343) provides natural resource control for multi-tenant cloud

**Cons**:
- More complex deployment pipeline
- Need to maintain two build targets

### Option 3: Plugin-Only Cloud Execution
Deploy individual language runtimes as separate WASM modules on Wasmer Edge:
- Each of 12 runtimes becomes independent Edge deployment
- Main server orchestrates calls to appropriate runtime endpoint
- Enables per-runtime scaling and isolation

**Pros**:
- Fine-grained resource allocation per language
- Can update individual runtimes independently
- Natural fault isolation

**Cons**:
- Network latency between orchestrator and runtimes
- Complex service mesh management
- Higher operational overhead

## Pricing Model

Based on Wasmer Edge public pricing (verify current rates at [wasmer.io/products/edge](https://wasmer.io/products/edge)):

- **Compute**: Pay per WASM instruction executed (metering integration #343 enables precise billing)
- **Storage**: Persistent volumes charged per GB/month
- **Network**: Data transfer out charged per GB
- **Free Tier**: Typically includes ~1M invocations/month for testing

With our metering middleware (`instruction_limit` config), we can:
1. Enforce per-tool execution budgets
2. Implement fair usage policies
3. Provide cost estimates to users before execution
4. Prevent runaway computations

## Implementation Roadmap

### Phase 1: Prerequisites (Current Work)
- ✅ Metering middleware integration (#343)
- ✅ WASM module compilation caching (#345)
- ⏳ Instruction limit configuration in ServerConfig (#344)

### Phase 2: WASM HTTP Adapter
Create WASI-HTTP handler for VELOCITY-MCP:
```rust
// New module: src/wasm_runtime/edge_http.rs
#[cfg(target_arch = "wasm32")]
pub fn handle_http_request(request: wasi_http::Request) -> wasi_http::Response {
    // Parse MCP JSON-RPC request
    // Dispatch to appropriate WASM runtime
    // Return JSON-RPC response
}
```

### Phase 3: Build Pipeline
Add WASM build target:
```toml
# Cargo.toml
[target.wasm32-wasip1.dependencies]
wasi-http = "0.2"  # WASI Preview 2 HTTP bindings

[[bin]]
name = "velocity-edge"
path = "src/bin/velocity_edge.rs"
```

### Phase 4: Deployment Configuration
Create `wasmer.toml`:
```toml
[package]
name = "velocity-mcp-edge"
version = "3.2.0"

[dependencies]
wasi-http = "^0.2.0"

[module]
main = "target/wasm32-wasip1/release/velocity-edge.wasm"

[edge]
min_instances = 0
max_instances = 100
memory_mb = 512
instruction_limit = 10_000_000  # 10M instructions per request
```

### Phase 5: Benchmarking & Validation
Compare performance:
- Local NDA/shmem: ~7µs baseline
- Local WASM (Wasmer): ~4.1x overhead vs native
- Wasmer Edge: Expected <1ms cold start + network latency (~10-50ms depending on region)

## Limitations & Constraints

### Platform Restrictions
1. **No Process Spawning**: WASM sandbox prevents `fork()`/`exec()` - affects shell_exec tool
2. **Limited Filesystem**: Only mounted volumes accessible - impacts file_read/write tools
3. **No Raw Sockets**: TCP/UDP limited to WASI networking API - affects custom protocols
4. **Memory Limits**: Typically 512MB-2GB per instance

### Feature Gaps
The following VELOCITY-MCP features would need adaptation:
- **NDA-native transport**: Requires shared memory (not available in serverless)
- **Plugin hot-reload**: Filesystem watches not available on Edge
- **Local database resources**: Must migrate to external DB or Edge volumes
- **Process-based sandboxing**: Replaced by WASM sandbox itself

### Mitigation Strategies
1. **Conditional Compilation**: Gate OS-specific features behind `#[cfg(not(target_arch = "wasm32"))]`
2. **Feature Flags**: Add `cloud-mode` feature that disables incompatible features
3. **Graceful Degradation**: Detect runtime environment and adjust capabilities accordingly

## Recommendation

**Adopt Option 2 (Hybrid Two-Tier)** for the following reasons:

1. **Preserves Competitive Advantage**: Local NDA/shmem performance (~7µs) remains unmatched by any cloud solution
2. **Enables New Market**: Serverless deployment opens VELOCITY-MCP to users who don't want to manage infrastructure
3. **Natural Progression**: Metering (#343) and caching (#345) already being implemented serve both local and cloud modes
4. **Risk Mitigation**: Maintaining native server ensures business continuity if cloud adoption is slow

### Next Steps
1. Complete metering integration (current task #343)
2. Add instruction limit to ServerConfig (#344)
3. Create WASM HTTP adapter prototype
4. Benchmark single-tool execution on Wasmer Edge free tier
5. Evaluate cost per 1000 tool invocations
6. Design hybrid routing logic (local vs cloud decision tree)

## References

- [Wasmer Edge Documentation](https://docs.wasmer.io/edge/)
- [Wasmer Edge Product Page](https://wasmer.io/products/edge)
- [WASI HTTP Proposal](https://github.com/WebAssembly/wasi-http)
- VELOCITY-MCP Memory: [WASM vs Native Benchmark](project-wasm-vs-native-benchmark.md)

Sources:
- [Edge Introduction - Wasmer Docs](https://docs.wasmer.io/edge/)
- [Wasmer Edge](https://wasmer.io/products/edge)
- [Wasmer: Universal applications using WebAssembly](https://wasmer.io/)
- [Blog · Wasmer](https://wasmer.io/posts)
