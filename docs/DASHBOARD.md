# Live Dashboard

A zero-dependency browser console for demonstrating V.E.L.O.C.I.T.Y.-MCP over
HTTP. One static HTML file — no build step, no framework, no Electron.

- **File:** `dashboard/index.html` (vanilla JS + CSS, ~350 lines)
- **Demo config:** `dashboard/demo.toml`
- **Automated tests:** `dashboard/test_dashboard.sh` (DOM assertions),
  `dashboard/test_cdp.mjs` (real-browser SSE + latency checks),
  `dashboard/test_interactive.mjs` (drives the UI over CDP),
  `dashboard/probe_nda.mjs` (raw NMCP frames, no browser) and
  `dashboard/diff_nda.mjs` (NDA-vs-JSON payload differential over every WASM tool)
- **Screenshot tool:** `dashboard/screenshot.mjs`

## Quick start

```bash
# 1. Build with the http feature and start the server (listens on :3900)
cargo build --release --features http
target/release/velocity_mcp --mode http --config dashboard/demo.toml

# 2. Open dashboard/index.html in a browser and click Connect
```

## What it shows

| Panel | Source | Notes |
|-------|--------|-------|
| Connection pill + version | `GET /health` | status, `v3.2.0 · http` |
| Metric cards (total reqs, req/s, avg latency, uptime, sessions, auth fails / rate hits) | `GET /v1/performance`, polled every 1.5 s | req/s is computed as Δ total-requests between polls — the server's own `requests_per_second` is a lifetime average and reads ~0 on an idle box |
| Requests-per-second chart | same poll | canvas line, 120-point window |
| Tool grid (40 tools) | `tools/list` + `GET /v1/nda/capabilities` | native, plugin and WASM-runtime tools indistinguishable; WASM tools whose guest has no NDA binary executor are tagged `·json` |
| Tool execution | `tools/call` via `/v1/mcp` (JSON) or `/v1/mcp/nda` (NDA binary TLV) | arguments prefilled from each tool's real `inputSchema`; client-measured round-trip + running median of last 50 |
| Transport toggle (default: Both) | same call over both encodings, sequentially | JSON-RPC vs NDA binary side-by-side: latency, request/response byte counts, payload-equality check, and in-browser SHA-256 Merkle-root verification of the NDA frame. The JS codec is a port of `client/src/transport/nda_codec.rs` that enforces the same value bounds (truncated tag, string length/body, array-object count and key-length checks, fatal UTF-8, the `u16` key cap) but not the server parser's three caps (depth, element count, string length) that `docs/NDA_PROTOCOL.md` §4.1 tabulates, and an integer above 2^53-1 loses precision as a JS number; `dashboard/test_codec.mjs` pins this module and the inline copy in `dashboard/index.html` to identical verdicts on identical bytes. Both endpoints are ping-warmed on connect so the comparison never shows cold-connection skew. For a `·json` tool the NDA leg is not run: the server would decode the frame's TLV arguments into the same JSON path, so the second row reads `skipped` instead of reporting a second number for one path |
| Ping burst | 50 sequential `ping` calls (×2 in Both mode) | avg / p50 / p95 round-trips per encoding; stays under the 100-request burst of the token-bucket rate limiter |
| Live events | `GET /v1/sse` | EventSource stream + local action log |

Honest framing baked into the panel: over localhost HTTP the two encodings run at
near-identical speed (measured 2026-09-18: burst avg JSON 1.05 ms vs NDA 1.21 ms;
single warm call 3.10 vs 1.80 ms; frame 121 vs 119 B) — the NDA design pays off in
the shared-memory transport, which is what `docs/COMPARISON.md` benchmarks. The
toggle proves binary-protocol *correctness*, integrity and compactness in the
browser, not a speed claim over HTTP.

Which WASM tools take TLV bytes directly is a property of the guest, not of the
frame: measured 2026-10-09 through `registry::nda_binary_capabilities()` — the same
function `GET /v1/nda/capabilities` serves — with `plugins/` loaded, all 22
registered WASM tools report `binaryExecutor: true`. The six tree-walk interpreter
tools (`perl_greet`, `r_greet`, `php_greet`, `java_greet`, `julia_greet`,
`csharp_greet`) used to be the exception: their guests answered every NDA call with
`ERROR: Unknown tool`, so the host carried a language blocklist that routed those six
to decoded JSON and the dashboard tagged them `· json`. The defect was in the guest
(`interp_call_tool_binary` looked the name up in the scope it had just reset instead
of running the source registered through `interp_register_tool`); it is fixed, the
blocklist is deleted, and `test_interpreter_guests_binary_matches_json` in
`tests/binary_protocol_test.rs` pins both halves — same payload through both
encodings, and `binaryExecutor == true`. The report still self-corrects per tool —
`analyze_logs_go` cannot register on a machine without TinyGo, and after its first
failed NDA call the report moves that tool to the decoded JSON group (22/0 → 21/1)
while `go_text_stats` stays binary. See [docs/NDA_PROTOCOL.md](NDA_PROTOCOL.md) §8.1
for that path and §9 for the measured per-tool matrix.
One caveat the panel cannot show: "binary executor" means the request reaches the
runtime's binary path, which is the transport boundary the toggle is comparing. It
does not mean the guest decoded the TLV. Measured 2026-10-09 by sending a
deliberately malformed frame and reading whose decoder refuses it: 9 of the 22 tools
decode arguments inside the sandbox — the three Lua tools and the six interpreter
tools, each of which answers in its own words (`ERROR: TLV decode failed or not an
object`). Python, JavaScript and TypeScript reach the binary path but their shipped
guest blobs predate the `*_call_tool_binary` export, so the host decodes and
re-serializes as JSON one layer deeper; Ruby, Go and Rust have no binary entry point
in the runtime at all. §9.2 of `NDA_PROTOCOL.md` lists each runtime and what closing
its gap costs.

## URL parameters

| Parameter | Effect |
|-----------|--------|
| `?demo=1` | Auto-run the full flow: connect → `echo` → `js_calculator` (QuickJS WASM) → 50-ping burst. Zero-click demo. |
| `?capture=1` | Skip the SSE connection. An open EventSource freezes Chrome's headless virtual clock, so this is required for automated screenshots and `--dump-dom` tests. |
| `?api=http://host:port` | Override the server URL field (used by the test harness, including its dead-port error path). |

## Automated test

```bash
bash dashboard/test_dashboard.sh
```

Renders the page in headless Chrome (`--dump-dom --virtual-time-budget`), then
asserts on the serialized DOM. 14 checks (2 preflight + 10 golden path + 2
error path):

- **Golden path** (`?demo=1&capture=1`): healthy pill, v3.2.0 pill, 40 tools
  rendered, QuickJS `js_calculator` result `1066` present, latency rendered in
  ms, burst completed "over 50 pings", echo call logged, total-requests and
  req/s stats populated, and a negative check that no sub-10 µs HTTP latency
  is displayed (that would mean a units bug).
- **Error path** (`?api=http://127.0.0.1:1`): pill turns `unreachable`,
  failure is logged to the event feed.

Requirements: the server running on `:3900` (or `DASH_URL=...`), and Chrome or
Edge (`CHROME=/path/to/chrome.exe`). Last full run: 2026-10-02, **14/14 pass**;
re-run 2026-10-09 and again 2026-10-10, **14 passed / 0 failed, exit 0** on both days. None of the 14 checks depend on the
interpreter guests rebuilt on 2026-10-09 — the suite's only WASM legs are QuickJS
`js_calculator` and the native `echo` tool — so this run is valid against a server
started before that rebuild, unlike the 22-tool differential in
`docs/NDA_PROTOCOL.md` §9.

`node dashboard/probe_nda.mjs` (no browser: builds NMCP frames, POSTs them to
`/v1/mcp/nda`, verifies the SHA-256 root) ran on 2026-10-09 and again on 2026-10-10: **6 pass / 0 fail, exit 0** both days. Its exit
code was wrong until then — the summary printed `6 pass, 0 fail` and the process
then aborted inside libuv on Windows (`Assertion failed:
!(handle->flags & UV_HANDLE_CLOSING), src/win/async.c`, exit 127), because it called
`process.exit()` while the checks' keep-alive sockets were still closing. A scripted
gate on that suite therefore reported red on a green run. It now sets
`process.exitCode` and lets the loop drain: 0 in 0.2 s on the passing path, and 1
with the honest `fetch failed` lines against a dead port (both branches verified).

`node dashboard/diff_nda.mjs [baseUrl] [--poison]` is the NDA-vs-JSON payload
differential: one fixture per WASM tool (22 of them), the JSON-RPC leg then the NDA leg,
normalized comparison, and the capability report re-read before and after so a run can see
its own downgrades. Exit 0 only when every payload matches. Last run 2026-10-10:
**22/22 payload-identical, exit 0** against `:3900`, with capabilities 15/7 both before and after
(the 2026-10-09 run matched that against the same server and against a debug build of current
source, which cannot be relaunched now that `target/` is gone); it also prints
`identical-but-erroring (tool broken in this build, not a transport split): 1 — analyze_logs_go`, which
fails over *both* transports because TinyGo is not installed on this machine — a tool
health fact, not a transport split, and an earlier version of this runner wrongly counted
it as one. The frame codec both scripts use lives in `dashboard/nda_codec.mjs`, which
performs no I/O on import (the probe briefly grew `export` keywords on itself, and
importing it then ran its six checks and printed a stray `6 pass, 0 fail`). `dashboard/test_codec.mjs` gates the codec offline -- 11 well-formed frames decode, 16 corrupt ones reject with a `TLV ...` error instead of a fabricated value, and the inline `dashboard/index.html` copy must agree on all 27 -- no server, no network, exit 0 at 30 checks; run against the pre-fix decoder it returns 16 failures, so the gate is proven to be able to fail.
`--poison` is off by default and needs a disposable server: it sends one malformed frame
per tool, and on a debug build one such frame moved `py_unit_converter` to
`binaryExecutor: false`, while a full pass on the release server took the report from 16/6
to 13/9. Its decode-site column is advisory — a guest that tolerates the missing field
answers the same whether it decoded the TLV or received the host's re-serialized JSON `[]`
(`docs/NDA_PROTOCOL.md` §9.1 has the measurements).

`test_interactive.mjs` also spawns its own API-keyed server from
`target/release/velocity_mcp.exe`, so that exe must be a `--features http` build.
That path is also this suite's single hard dependency on a compiled artifact: with
`target/` absent it stops at the auth-server spawn while the checks that address an
already-running server still pass. `probe_nda.mjs` and `diff_nda.mjs` are therefore
the legs that need no build step at all, only a server that is already up; the two
browser legs additionally need Chrome, and the interactive one needs the exe.
Mind the ordering with the workspace suite: `cargo test --workspace` resolves default
features (`default = []`), relinks that same path without HTTP, and the interactive
suite then reports `auth server failed to start` while every other check still passes
against the already-running server. Rebuild `--features http` after a workspace test
run; run 2026-10-02 hit exactly that and re-passed 33/33 after the rebuild.

Start the server with `--config dashboard/demo.toml`, not a bare `--addr`: the page
is opened from `file://`, so its `Origin` is `null` and the production default CORS
policy (localhost origins only) rejects every fetch. The failure looks like a dead
server — the pill reads `unreachable` while `curl /health` is healthy — and it takes
down all eight DOM checks at once.

The SSE panel and the latency-distribution checks can't be exercised by
`--dump-dom`: an open EventSource freezes headless virtual time, and
`--virtual-time-budget` quantizes **every** clock (including Resource Timing
entries) to 10 ms ticks — so a flat "10.00 ms" in a dump-dom render is a
harness artifact, not a measurement. These cases run in a real browser session
over Chrome DevTools Protocol with wall-clock waits:

```bash
node dashboard/test_cdp.mjs        # requires Node >= 22 (global WebSocket)
```

5 checks: healthy pill after connect, SSE event feed active in the log, burst
completed, burst shows a real latency distribution, and an avg plausible for
localhost HTTP. The distribution check fails only when avg *and* p50 both read
exactly 10.00 ms — a quantized-clock signature, not a speed limit — so a p95
over 10 is not a failure, which is how the pre-fix run of 2026-09-18 passed 5/5
at avg 8.39 / p50 8.1 / p95 10.4 ms. That leg then could not run again until
2026-10-09: the debugging port was hardcoded to 9333 with no override, that port
belongs to a running Chrome of the operator's, and driving the suite against it
would navigate the operator's real tabs. The three CDP drivers (`test_cdp.mjs`,
`test_interactive.mjs`, `screenshot.mjs`) now use `--remote-debugging-port=0` and
read Chrome's chosen port from `DevToolsActivePort`. Re-runs on :3900: 2026-10-09
avg 1.6 / p50 1.5 / p95 2.6 ms, 2026-10-10 avg 2.53 / p50 2.3 / p95 4.5 ms, both
5/5; a third run on 2026-10-11 also passed 5/5 and its distribution, with the
release-vs-debug comparison, is in `docs/NDA_PROTOCOL.md` §9.4, because the build
profile is worth more than the spread between runs and must be named with it.
The SSE leg asserts a page-log line, so it cannot see frame doubling: the
wire-frame check in the interactive suite is the one that can.

A third suite drives the full interactive UI over CDP — clicks chips and
buttons, edits fields, and asserts on resulting DOM state:

```bash
node dashboard/test_interactive.mjs
```

38 checks: connect + 40 chips; tool calls across runtimes (native echo /
text_transform / bench_echo, QuickJS, MicroPython, Lua, Go, Rust); schema
prefill; invalid-JSON rejection; **NDA binary transport** (identical payload
via both encodings, in-browser SHA-256 root verification, honest row labels,
smaller wire frame, NDA-only WASM execution, and the capability gate: a tool the
capability report marks as having no binary executor is still run through the JSON
leg only — its NDA row reads `skipped` with no latency or byte count, the verdict
line states why, and the JSON result still renders); **auth against a second server
instance with `VELOCITY_API_KEY` set** (wrong key rejected + logged, correct
key connects and calls tools, SSE correctly reported as needing an
unauthenticated endpoint); double-Connect idempotency; back-to-back bursts vs
the rate limiter; SSE named events *and* the raw wire framing of the keep-alive;
metrics card liveness.
Last run: 2026-10-10, **38 passed / 0 failed, exit 0** against a release build of
current source (`--config` file with `cors_origins = ["*"]`, capability report
**22 binary / 0 without** at startup). The same harness run unchanged against the
older image that has been serving `:3900` all day gives **36 passed / 2 failed,
exit 1**, and the two red lines are exactly the SSE pair, with the wire check's own
detail printing `0 comment / 2 data-framed: data: event: connected | data: data:
{"sessionId": …}`. That is the harness's negative control: it fails on the defect it
was written to catch.

The count moved from 37 to 38 when the single "heartbeat appears in the page log"
check was split in two, because that check could not see the thing under test: a
browser `EventSource` discards SSE comment lines *by specification*, so a keep-alive
that is correctly a bare `: heartbeat` comment is invisible to the page, and one that
is wrongly double-framed as `data: : heartbeat` arrives as a `message` event. The page
side now asserts what the page can actually observe (`SSE stream open` plus a delivered
named `connected` event, which only works because `openSSE()` registers
`addEventListener` handlers), and a raw `node:http` subscription
(`sseWireFrames()`) counts comment frames and double-framed lines straight off the
socket, resolving as soon as the first heartbeat lands so the run pays the server's
15 s interval and nothing more.

Two capability-gate blocks make up the four checks above the old count. The gate is
learned per process, so its population is a property of the answering server: the
long-running demo instance measured **15 binary executors / 7 without** on 2026-10-10
(one more than the 16/6 recorded earlier the same day, because `analyze_logs_go` loses
its binary executor to this host's missing TinyGo on its first NDA call), while a
release build of the same source starts at **22 / 0** — `docs/NDA_PROTOCOL.md` §8.1
records why the count belongs to the process, not the build. The first block therefore
selects whatever the *server* lists as lacking a binary executor (`noBinary`'s first
member, falling back to `perl_greet` only when the set is empty) and prints which arm it
took: on a JSON-only selection it asserts the chip, the skipped NDA row, the verdict line
and the page's own `compare <tool>: JSON only (no NDA binary executor)` log line; on a
binary-capable server it asserts the rendered payload instead. The second block does not
depend on the server's state at all: it adds `perl_greet` to the dashboard's own
`noBinary` set, reloads the tool list, asserts the same skipped rendering, then removes
it — which is what proves the gated branch on a cold 22/0 server, where the first block
necessarily takes the other arm. Both blocks assert the cause-neutral verdict wording
(`reports no binary NDA executor`), because
`record_json_fallback` (`src/registry.rs:626`) is the only writer of that flag and the
server never states *why* a guest lost its executor.

Getting that run required fixing two defects in the harness itself, not the product:

- **A green run that printed nothing and exited non-zero.** Teardown threw `EPERM` from
  `rmSync` while Chrome's children still held the temporary profile dir, and because the
  throw happened inside `finally` it destroyed both the pass/fail tally and the exit
  code. It now retries with a delay and prints `(left temporary profile dir: …)` rather
  than crashing the report.
- **A result that depended on where the operator launched node.** The suite `spawn()`s
  its own API-keyed server without a `cwd`, so that child inherited the launch
  directory. Started from `dashboard/` it advertised 16 tools instead of 40 and a tool
  call came back `C# core engine not found` — i.e. the two auth checks were measuring
  the working directory, not the server. `spawn()` now passes the repo root explicitly,
  and both checks pass from either directory.

It caught two real dashboard bugs during development: `?api=` was
re-applied on every Connect (clobbering manual server-URL edits — now applied
once at startup), and an SSE failure overwrote the connection pill (now
logged only; the pill tracks the HTTP connection).

## Screenshot (real clock, recommended)

```bash
node dashboard/screenshot.mjs demo.png   # runs the auto-demo, captures after ~13 s
```

This drives a wall-clock browser session over CDP, so the latency figures in
the image are genuine. Avoid `chrome --headless --screenshot
--virtual-time-budget=…` for anything showing numbers: virtual time
quantizes all clocks to 10 ms ticks and the burst renders as a fake-flat
"10.00 ms".

## Honest numbers

Measured through the dashboard on 2026-09-18 (Core 5 210H, localhost HTTP),
using browser Resource Timing (`responseEnd − startTime`, stamped by the
network stack's high-resolution clock — plain `await fetch` wall-time on
Windows is inflated and flattened to ~10 ms by event-loop timer granularity,
which the dashboard no longer reports): ping-burst round-trips measured
avg 2.06 / p50 2.00 / p95 3.40 ms, avg 5.12 / p50 4.90 / p95 8.60 ms and
avg 8.39 / p50 8.10 / p95 10.40 ms across three runs; and on 2026-10-09 a fourth
run at avg 1.60 / p50 1.50 / p95 2.60 ms. The spread across runs (1.6–8.4 ms) is
wider than any protocol effect and tracks what else the machine was doing — quote
the number on the screen, not a remembered one. Server-reported average
latency ~2.3–6.8 ms (card value, lifetime average). The first tool call after
connect measures slower (66–123 ms: TCP setup + cold dispatch path) and the
dashboard shows that honestly rather than hiding it. These are **HTTP transport**
numbers. The microsecond
figures elsewhere in the docs (e.g. ~2 µs ping) are the NDA/shared-memory
pipeline — a different transport. When demoing the dashboard, quote the HTTP
numbers from the screen and point to `docs/COMPARISON.md` for the shmem
pipeline.

## Security notes

- `dashboard/demo.toml` sets `cors_origins = ["*"]` so the page works opened
  as a local file (Origin `null`). This is **demo-only**: the production
  default (no `cors_origins` in config) restricts CORS to localhost origins.
- The API-key field sends `Authorization: Bearer <key>`; leave empty only for
  unauthenticated local demos. Note: browser `EventSource` cannot send
  headers, so the SSE panel only works against a server without `api_key`
  set; everything else (metrics, tools, calls) works fully with auth enabled.
- The dashboard talks to the same public HTTP API any MCP client uses — no
  privileged access, no server-side code.
