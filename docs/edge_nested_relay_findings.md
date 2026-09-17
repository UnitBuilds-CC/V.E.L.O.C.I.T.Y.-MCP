# Edge Nested-WASM vs Relay — Findings

Date: 2026-09-17. Live target: `velocity-mcp-edge` on Wasmer Edge (multi-threaded WASIX HTTP, package 3.19.x). Purpose: determine how to expose our WASM language runtimes (QuickJS/Lua/MicroPython/…) on Edge, given Edge itself runs our server *as* a WASM module.

For the strategic follow-on — making Edge viable for *stateful* MCP via a Wasmer-managed durable-state service — see [Durable State as a Wasmer-Managed Internal Service](edge_durable_state_service_proposal.md).

## Method

Empirical: each claim below was built and run, not reasoned. Scratch crates and a throwaway backend app were used and cleaned up; none modified the shipped deployment (except the documented revert cycles). Local Wasmer CLI is 7.4.1; Edge runs a newer WASIX runtime.

## 1. Nested WASM (run a plugin *inside* the Edge instance)

- **wasmer as the nested host: impossible.** `wasmer = "5"` will not compile for `wasm32-wasmer-wasi` — its own gate raises `compile_error!("The sys feature must be enabled only for non-wasm32 target")` + unresolved `wasmer_vm`/`wasmer_compiler`. Cranelift JITs to native code (mmap/executable-memory/threads); none exist in a wasm32 sandbox.
- **A pure-Rust interpreter (wasmi) as the nested host: works.** `wasmi = "0.32"` compiles cleanly for `wasm32-wasmer-wasi`. Deployed to Edge and called over HTTPS: it parsed, instantiated, and called an inner WASM module and returned `{parse_ok, instantiate_ok, call_ok, result: 42}`. So wasm-in-wasm is genuinely viable on Edge via an interpreter. (Cost: wasmi added ~1.3 MB, binary 1.03 → 2.31 MB.)
- **But real plugins are not trivial.** `quickjs.wasm` exports the raw QuickJS C API (`JS_NewRuntime`, `JS_Eval`, `js_malloc*`, …) + `_initialize` and imports 6 custom `env::host_*` callbacks plus `wasi_snapshot_preview1`. Running it nested under wasmi needs a WASI shim, all 6 host functions reimplemented, and the whole native orchestration redone — *and* it is interpret-QuickJS-while-wasmi-interprets (double interpretation) on a 2-CPU/instruction-budgeted instance. Nested is the weakest option for production plugins.

## 2. Relay (frontend routes each call to a separate per-runtime Edge app)

Deployed a distinct backend Edge app (`velocity-relay-backend`, same echo binary) and measured a relay-shaped hop.

| Signal | Measured | Meaning |
|---|---|---|
| Cold start (idle → first) | 2150 ms vs ~740 ms warm | **~1.3–1.4 s scale-from-zero penalty** (location-independent) |
| TCP connect (client → us-ashburn) | 242–364 ms / RTT | Namibia↔US geography |
| TLS handshake | 255–310 ms | ~1–2 extra WAN round-trips |
| Warm total (backend) | 740–880 ms | ~3 WAN RTTs |
| Warm total (frontend, same region) | ~790 ms | identical ⇒ warm cost is network, not the relay |
| Concurrent ×20 | 20/20 ok, p50 850 ms | concurrency fine |
| **Server compute (ttfb − network)** | **≈ 4 ms** | Wasmer processing is negligible |

Interpretation: the ~740 ms is *our vantage* (transatlantic client), not an intra-region `core→backend` hop. A real relay stays inside one Edge region: expect **low-tens-of-ms on a fresh connection, a few ms with keepalive**, plus the ~1.4 s tax only on a cold backend. The relay contract + auth (a backend is a public URL; core→backend needs a shared secret and must reject direct calls) is sound and outbound TCP/HTTP egress from a WASIX instance was confirmed working.

## 3. Platform transport constraints (measured)

- **UDP unavailable:** `UdpSocket::bind` succeeds but `send_to` fails with `ConnectionReset (os error 15)`. TCP egress works (control connects to 8.8.8.8:53 and 1.1.1.1:53).
- **No cross-instance shared memory:** each Edge instance is an isolated sandbox; there is no shared region to hand a shmem handle across, and ingress is HTTP-only (port 80).

These are the same two capabilities a same-node, non-serializing transport (e.g. our VCTP/shmem) would need. They are a platform ask, not something shippable on Edge today — which is why the relay is framed as a proof-of-contract, not a v1 path.

## 4. What a faster interface (shmem / VCTP) would change

Latency of a relay tool call decomposes into independent legs; a same-node shared-memory transport only collapses ONE of them.

```
client ──(A: client↔Edge front, WAN)──> core(front) ──(B: core↔backend hop)──> backend-runtime
                    unavoidable                                    THIS is what shmem replaces
```

- **Leg B (the relay hop) today:** intra-region HTTPS = a few RTTs + TLS + per-request JSON framing. With keepalive, single-digit-to-low-tens of ms; ~4 ms of that is compute we measured, the rest is HTTP/TLS + serialization overhead.
- **Leg B with our shmem / VCTP:** the native numbers apply — NDA/shmem ping ≈ 2 µs, tools/call ≈ 3–7 µs, and the binary TLV path avoids double-JSON serialization. So Leg B drops from ~10⁴ µs to ~10⁰–10¹ µs — the "few microseconds, not 1000×" the VCTP idea targets. This is real and large *for that hop*.
- **Leg A (client↔front):** unchanged. A remote client still pays its geographic RTT; no backend transport fixes it. For a same-region client (e.g. an app co-located in us-ashburn) Leg A is small and Leg B becomes the visible cost — that's where shmem pays off most.
- **Cold start (~1.4 s):** unchanged. It's instance spin-up from zero, orthogonal to the transport. Mitigate with pinned min-instances for hot runtimes; reserve scale-to-zero for rarely-used languages where a first-call penalty is acceptable.

Net: a same-node shmem/VCTP interface makes the *internal* fan-out effectively free (µs, non-serializing) and removes the relay's main drawback, but it does **not** touch client-side WAN latency or the scale-to-zero cold penalty. The architecture is therefore "per-runtime scale + µs internal routing" *if* Wasmer exposes same-node shared memory (or a fast local socket) to colocated instances — the concrete platform ask.

## 5. Recommendations

1. Do not pursue nested real runtimes (wasmi + QuickJS C-API) — worst effort/cost of the options.
2. If plugin breadth on Edge matters, the clean backend is **each interpreter as its own standalone WASIX HTTP binary** (no wasmi, no nesting, native-wasm speed, independent scaling) — QuickJS is the right first proof.
3. Frame any relay/same-node pitch around the measured numbers above; the deciding platform capabilities are UDP (absent), cross-instance shared memory (absent), and cold-start behavior (~1.4 s).
4. Keep the shipped Edge product as-is: JSON-RPC/HTTP + 10 pure-Rust tools, multi-threaded, live. Plugins + NDA/shmem stay native.

## 6. Nested-wasmi plugin gate on Edge (measured 2026-09-17, section 1 updated)

Earlier "do not pursue nested" reflected cost/latency for a *full runtime*, but the **mechanism is now confirmed to work on Edge**. A temporary `nested_gate` tool used wasmi inside the Edge instance to load and run a real WASI plugin module (`example_tool.wasm`, imports only `environ_get`/`environ_sizes_get`/`fd_write`/`proc_exit`, exports `_initialize`/`get_input_ptr`/`tool_execute`), driving the exact native ABI (`get_input_ptr`→write args→`tool_execute(len)→i64 packed ptr<<32|len`→read result).

Live Edge HTTPS result (3 calls): `instantiate_ok:true, initialize_ok:true, execute_ok:true, output:"{...}", total_ms:2–3`. Binary grew 1.03 → 2.39 MB (wasmi ≈ +1.36 MB). Validated natively first (same ABI, 16 ms).

What this proves: wasmi running a real WASI plugin inside an Edge WASM instance works, fast, per-call.

**Escalated to a real interpreter (2026-09-17):** MicroPython (`micropython.wasm`, WASI-only, `mp_wasi_*` API) was wired as a genuine MCP tool `micronest` (advertised via `tools/list`, invoked via `tools/call`) and run inside the Edge instance over HTTPS. Live results: `print(1+1)`→`2`, `print('hello'.upper())`→`HELLO`, `print(sum(range(10)))`→`45`; `tools/list` grew to 11. Binary 3.0 MB. So B is **functional as a real MCP server on Edge** — a language runtime executing through the full protocol. All probe code was then reverted; the shipped artifact (3.19.4) is back to the clean 10-tool build with JSON-RPC fixes intact and the committed lock unchanged.

Remaining to "full tool servers": real interpreters via wasmi work (MicroPython proven); QuickJS/Lua still need their `env::host_*`/heavier WASI shim reimplemented under wasmi; and per-runtime instruction/CPU cost under double-interpretation for heavy workloads is the open risk to measure.

