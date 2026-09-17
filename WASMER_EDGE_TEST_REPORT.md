# Wasmer Edge MCP Deployment Test Report

> ## ⚠ HISTORICAL / SUPERSEDED — do not use as current status
>
> **Everything in this file below this banner describes artifacts and tests from
> 2026-09-16 and earlier. Its verdict — "tools/call NOT SUPPORTED", "Do NOT deploy
> to Wasmer Edge" — is FALSE for the current build and is retained only as a record
> of what was observed then.**
>
> **Current status (verified live 2026-09-17):** Wasmer Edge runs the full
> Hyper/Tokio MCP HTTP server compiled to `wasm32-wasmer-wasi`, taking HTTP over
> **WASIX sockets** (not the old WCGI/CGI path). It is genuinely **multi-threaded**
> (measured `available_parallelism()` = 2; Edge caps an instance at 2 CPUs),
> advertises **10 static tools** from `tools/list`, executes `tools/call`, and is
> deployed and live. Measured from a transatlantic client: 2150 ms cold vs ~740 ms
> warm, with ~4 ms of server compute.
>
> **Authoritative documents:**
> - [docs/wasmer_edge_deployment.md](docs/wasmer_edge_deployment.md) — how to build and deploy the current WASIX Edge artifact
> - [docs/edge_nested_poc_benchmark.md](docs/edge_nested_poc_benchmark.md) — measured nested-WASM plugin runtimes, dynamic registration and durability on Edge
>
> **Path note:** the Edge crate serves `/health` and `/mcp` at the root
> (`crates/velocity-mcp-edge/src/main.rs`), which is why the historical curl
> examples below use those paths. The native server is different: `/health` is
> top-level and every other route is mounted under `/v1` (e.g. `/v1/mcp`).

## Correction — 2026-09-17

The historical conclusions below are superseded. The deployed 3.17.0 WASM artifact contains real tool dispatch: direct CGI tests passed health, initialize, tools/list, echo and malformed-JSON handling. These are not successful live MCP tests.

On Edge, the official prebuilt `wasmer-examples/rust-wcgi-starter@=0.1.23` also failed with missing `REQUEST_METHOD`. A separate TCP HTTP control, deliberately retaining the same WCGI runner annotation, returned repeated HTTP 200 responses with `x-edge-request-outcome: success` at https://lnh8imc0le5l.id.wasmer.app/ (version `dav_R8EIJtWu1rY3`). This demonstrates socket-proxy behavior for that deployment, not a working CGI request environment. It does not prove a platform-wide outage or explain the internal runner selection.

Wasmer Edge is therefore not categorically unusable; adapting and verifying the MCP HTTP server remains necessary. Neither production readiness nor a successful live MCP deployment has been established. The observations below refer to older artifacts and must not be used as the current status.

## Historical report

**Date:** 2026-09-16  
**Tested Version:** v3.2.35 (deployed), v3.2.38 (source)  
**Endpoint:** https://velocity-mcp-edge.wasmer.app/mcp

## Executive Summary (historical — 2026-09-16 artifact)

**CRITICAL FINDING *AT THE TIME*:** the Edge deployment tested on 2026-09-16 was **NOT VIABLE** for production MCP use. That specific deployed WASM binary used a stub implementation that did not support tool execution: only `initialize` and `tools/list` worked, and `tools/call` returned "Method not found". **This finding no longer describes the current build**, which dispatches `tools/call` for 10 static tools over WASIX socket HTTP — see the banner above and [docs/wasmer_edge_deployment.md](docs/wasmer_edge_deployment.md).

## Test Results

### 1. Latency Benchmark

**First Call (Cold Start):**
- initialize: 2020ms
- tools/list: 809ms
- tool call: 769ms (failed - method not supported)

**Warm Calls:**
- initialize: 845ms
- tools/list: 787ms
- tool call: 787ms (failed - method not supported)

**Sustained Load (20 calls):**
- Average: 875ms
- Range: 785ms - 1082ms
- Total: 17.5s for 20 calls

**Analysis:**
- ~800ms latency is dominated by network round-trip (Namibia → US Ashburn)
- Cold start penalty: ~1.2s for first initialize call
- No significant warm-up benefit after first call
- **This is moot** - the deployment doesn't support actual tool execution

### 2. Tool Execution

**Status (historical, 2026-09-16 stub):** ❌ NOT WORKING — superseded; the current Edge build executes `tools/call`

**Test:**
```bash
curl -X POST https://velocity-mcp-edge.wasmer.app/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"edge_ping","arguments":{}}}'
```

**Response:**
```json
{"error":{"code":-32601,"message":"Method not found: tools/call"},"id":1,"jsonrpc":"2.0"}
```

**Root Cause:**
The WASM32 build path (`crates/velocity-mcp-edge/src/main.rs:1015-1072`) implements a hardcoded stub:
- Only handles `initialize` and `tools/list`
- `tools/list` returns hardcoded list with single `edge_ping` tool
- All other methods fall through to generic "ok" response
- No actual tool execution logic

**Code Evidence:**
```rust
#[cfg(target_arch = "wasm32")]
fn handle_request(method: &str, body: &[u8]) -> (u16, &'static str, String) {
    match method {
        "GET" => (200, "OK", r#"{"status":"healthy","version":"3.2.0"}"#.into()),
        "POST" => {
            // ... parse JSON ...
            match method_name {
                "initialize" => (200, "OK", /* hardcoded response */),
                "tools/list" => (200, "OK", /* hardcoded empty tools */),
                _ => (200, "OK", r#"{"jsonrpc":"2.0","result":{"status":"ok"},"id":0}"#.into()),
            }
        }
        _ => (405, "Method Not Allowed", /* ... */),
    }
}
```

### 3. Audit Log Functionality

**Status:** ❌ CANNOT TEST

**Reason:** The Edge deployment is a stateless stub that doesn't execute tools or maintain any state. Audit logging requires:
- Tool execution (not implemented)
- Persistent storage (not available in stateless Edge)
- Session tracking (not implemented in stub)

### 4. User Isolation

**Status:** ❌ CANNOT TEST

**Reason:** Same as audit logs - the deployment is stateless and doesn't support actual MCP operations.

### 5. Dynamic Tool Registration

**Status:** ❌ CANNOT TEST

**Reason:** The tool list is hardcoded in the WASM binary. No mechanism for dynamic registration exists in the stub implementation.

## Comparison: Native vs Edge (as of the 2026-09-16 stub — superseded)

| Feature | Native (Rust/Docker) | Wasmer Edge |
|---------|---------------------|-------------|
| **Tool Execution** | ✅ Full support | ❌ Not implemented |
| **Latency** | ~4-50µs (local) | ~800ms (network) |
| **Audit Logging** | ✅ Full support | ❌ Stateless stub |
| **User Isolation** | ✅ Session-based | ❌ Not implemented |
| **Dynamic Tools** | ✅ Plugin system | ❌ Hardcoded list |
| **Resources** | ✅ Full support | ❌ Not implemented |
| **Prompts** | ✅ Full support | ❌ Not implemented |
| **Sampling** | ✅ Full support | ❌ Not implemented |
| **Streaming** | ✅ SSE support | ❌ Not implemented |
| **WASM Runtimes** | ✅ 13 languages (7 production + 6 tree-walk) | ❌ Not available |

## Root Cause Analysis

The Edge deployment was built with two code paths:

1. **Native path** (`#[cfg(not(target_arch = "wasm32"))]`):
   - Full hyper/tokio HTTP server
   - Complete MCP protocol implementation
   - Tool executor with EdgeToolExecutor
   - Security layers (rate limiting, auth, CORS)
   - Audit logging and session management

2. **WASM32 path** (`#[cfg(target_arch = "wasm32")]`):
   - Raw WCGI stdin/stdout handler
   - Hardcoded stub responses
   - No tool execution
   - No state management
   - No security layers

**Why this happened:**
- Wasmer Edge WCGI runner doesn't provide standard CGI environment variables
- The `cgi` crate panics when `REQUEST_METHOD` is missing
- Workaround was to bypass the cgi crate and write raw WCGI
- This resulted in a minimal stub rather than porting the full implementation

## Viability Assessment (of the 2026-09-16 stub — superseded)

### Wasmer Edge was NOT viable for the stub deployment:
- ❌ Production MCP servers
- ❌ Tool execution workloads
- ❌ Multi-tenant deployments
- ❌ Audit/compliance requirements
- ❌ Dynamic tool registration
- ❌ Any real MCP client interactions

### Wasmer Edge might be viable for:
- ✅ Health check endpoints
- ✅ Static tool discovery (read-only)
- ✅ Demo/showcase purposes (with major limitations)

## Recommendations

### Short-term (as written on 2026-09-16, for the stub artifact):
1. **Do NOT deploy to Wasmer Edge** for production use — *withdrawn*: Edge is now deployed and serving MCP over the WASIX socket HTTP path; see [docs/wasmer_edge_deployment.md](docs/wasmer_edge_deployment.md)
2. Use Docker/Kubernetes deployment instead (proven, fully functional)
3. Document the limitation clearly in all deployment guides

### Long-term options:
1. **Port full implementation to WASM32:**
   - Rewrite WCGI handler to support all MCP methods
   - Implement stateless tool execution (no persistent state)
   - Add session tracking via cookies/headers
   - Estimated effort: 2-3 weeks

2. **Use WASIX HTTP server approach:**
   - The native hyper server might work on Wasmer Edge with WASIX
   - Requires multi-threading support (broken in wasmer 7.4.1 at the time of writing)
   - Monitor Wasmer runtime updates
   - **Done 2026-09-17:** this is the path now deployed — the Hyper/Tokio server builds for `wasm32-wasmer-wasi` via `cargo-wasix` and runs with 2 tokio workers on Edge

3. **Alternative edge platforms:**
   - Cloudflare Workers (WASM-based, better HTTP support)
   - Fastly Compute@Edge (WASM-based)
   - AWS Lambda (native Rust support)

## Conclusion (historical — superseded)

**As measured on 2026-09-16,** the Wasmer Edge deployment was a **non-functional stub** that could not execute tools or support real MCP workflows, and the ~800ms latency was irrelevant because the deployment did not work. At that time **native deployment (Docker/Kubernetes) was the only usable option.**

**Current conclusion (2026-09-17):** the stub was replaced. Edge now runs the real MCP HTTP server over WASIX sockets, multi-threaded with 2 workers and 10 static tools, live and serving `tools/call` — so the historical verdict above applies only to the older artifact.

## Final Benchmark Comparison (2026-09-16, stub artifact — historical numbers)

**Native (localhost:3000):**
- initialize: 598ms (cold), ~5ms (warm)
- tools/list: 5ms
- tools/call (bench_echo): 5ms (actual execution)

**Wasmer Edge (us-ashburn):**
- initialize: 1400ms (cold), 845ms (warm)
- tools/list: 1871ms
- tools/call: NOT SUPPORTED **by that build** (the current build supports it)

**Performance Gap:**
- Native tools/call: 5ms
- Edge tools/list: 1164ms
- **Speedup: 231x** (native vs edge)

**Critical Context:**
This comparison is misleading because:
1. Native actually executed tools (5ms includes full execution)
2. Edge only returned a hardcoded list (no execution)
3. That Edge build could not execute tools at all, so the comparison was invalid

**True Comparison (at the time):**
- Native: worked, 5ms per tool call
- Edge (2026-09-16 stub): did not work; a tool call was impossible

**Verdict (historical):** against that stub, native was the only usable option. The stub has since been replaced by a working WASIX HTTP deployment — see [docs/wasmer_edge_deployment.md](docs/wasmer_edge_deployment.md) for current latency figures.
