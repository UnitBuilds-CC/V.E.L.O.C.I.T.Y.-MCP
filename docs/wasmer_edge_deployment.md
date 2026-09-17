# Wasmer Edge Deployment Strategy for VELOCITY-MCP

## Current Supported Path: WASIX HTTP (2026-09-17)

The full existing Hyper/Tokio MCP HTTP server builds for `wasm32-wasmer-wasi` using `cargo-wasix` 0.1.34 and the installed rustup `+wasix` toolchain. HTTP is enabled by `cfg(any(not(target_arch = "wasm32"), target_vendor = "wasmer"))`, including the existing auth, request/rate limits and CORS. No replacement HTTP adapter is required. The build compiles with a shared linear memory (`+atomics,+bulk-memory,+mutable-globals`), so tokio `rt-multi-thread` runs a genuine multi-threaded worker pool (measured `available_parallelism()` = 2 on the live Edge instance; Edge caps an instance at 2 CPUs). This is a multi-threaded, not single-threaded, deployment.

Prerequisites: Python 3, `cargo-wasix`, the installed `+wasix` toolchain and Wasmer CLI. From the repository root:

```bash
python deploy/build-edge.py
```

This reproducible entrypoint builds current core/edge sources in an isolated temporary workspace using `deploy/edge-wasix.lock`, the official sparse registry `https://cargo-registry.wasix.org/`, and `CARGO_HTTP_MULTIPLEXING=false`. It leaves the native root `Cargo.lock` and `.cargo/config.toml` untouched. The artifact is `target/wasm32-wasmer-wasi/release/velocity-edge.wasm`.

The package manifest uses the WASI runner, not the old `[module] main` layout:

```toml
[[module]]
name = "velocity-edge"
source = "target/wasm32-wasmer-wasi/release/velocity-edge.wasm"
abi = "wasi"

[[command]]
name = "velocity-edge"
module = "velocity-edge"
runner = "https://webc.org/runner/wasi"

[command.annotations.wasi]
env = ["PORT=80"]
```

Verified live 2026-09-17 on the public default (`https://velocity-mcp-edge.wasmer.app`): health, `initialize`, `tools/list` (request `id` preserved, 10 tools), repeated echo calls, and malformed-JSON rejection all work with the approximately 1 MB WASIX binary. Activation is complete. Cold-start and latency figures remain unmeasured. See the [Edge User Guide](edge_user_guide.md) for preview steps.

Plain WASI CGI remains separate. The earlier `wasm32-wasip1` CGI entrypoint failed in its deployed configuration; that does not establish that WCGI is globally broken.

For getting language runtimes (WASM plugins) onto Edge — nested interpreter vs relay, with measured latency and the shmem/VCTP analysis — see [Edge Nested-WASM vs Relay — Findings](edge_nested_relay_findings.md).

## Historical Strategy Notes — Not Current Deployment Instructions

The remaining options, roadmap and platform assumptions are retained as historical proposals, not validated capabilities or recommendations for the current artifact. The supported path above supersedes the proposed HTTP adapter and build pipeline. Pricing, limits and runtime integrations require separate validation.

## Overview (Historical)

Wasmer Edge is Wasmer's serverless platform designed for WebAssembly workloads. The following platform features were considered during planning; they are not performance guarantees for this server.

## Key Features (Historical)

### Performance
- **Cold Start Time**: Not measured for this build; earlier sub-millisecond estimates were speculative.
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
VELOCITY-MCP's 7 production language runtimes (and 6 planned runtimes) must be compiled as:
- WASI-compatible WASM modules
- Export `_start` or HTTP handler function
- Maximum module size: ~50MB per runtime

### Historical Runtime Candidates (Not Validated Edge Support)

**Native Runtimes (7; not included in the current Edge artifact)**:
- Rust/WASM (wasm32-wasi target)
- Lua/WASI reactor
- MicroPython/WASI reactor
- QuickJS/WASI reactor (JavaScript)
- QuickJS/WASI reactor (TypeScript)
- Ruby/mruby (with WASI imports)
- Go/TinyGo (wasm32-wasi)

**Planned Runtimes (6, currently toy interpreters)**:
- PHP/WASI
- Perl/WASI
- Java/TeaVM (with WASI polyfill)
- R/WebR (with WASI networking)
- Julia/WASI
- C#/.NET (with WASI runtime)

WASI imports alone do not establish Edge compatibility. The current artifact does not embed the native Wasmer/Cranelift plugin subsystem. Nested interpreters or runtime integrations require separate implementation and testing; they are neither automatically supported nor categorically impossible.

## Deployment Architecture Options (Historical)

### Option 1: Full Server Migration (Historical Proposal)
The original proposal was to deploy the entire native server as a WASM module:
- Compile the Rust binary to a plain WASI target (superseded by the WASIX HTTP build above)
- Replace Axum with a WASI-HTTP handler (not needed for the existing Hyper/Tokio Edge server)
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
- Each of the 7 production runtimes becomes an independent Edge deployment (planned runtimes added as they reach production readiness)
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
