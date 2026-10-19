# NDA / NMCP Binary Protocol Reference

The binary wire format V.E.L.O.C.I.T.Y.-MCP speaks natively. NDA (Native Data
Array) is the message format; NMCP (Native MCP) is the frame magic that identifies
it on the wire. One framing, one TLV codec, three carriers (shared memory, stdio,
HTTP) and one JSON path that stays byte-compatible with stock MCP clients.

All byte counts, hex dumps and capability numbers in this document were measured
against `target/release/velocity_mcp.exe --mode http --addr 127.0.0.1:3900` on
2026-10-02 unless stated otherwise.

---

## 1. Why a binary protocol

A stock MCP call serializes twice before your code sees it: the client turns
arguments into JSON text, the server parses that text into a value tree, and the
result rides the same round trip in reverse. For a WASM tool the server then
re-serializes the arguments to hand them to the guest.

NDA removes the text layer. Arguments travel as TLV bytes, are parsed once into a
value representation, and for WASM guests are handed to the runtime **as the same
bytes** the client sent — the guest decodes them in-place with its own C TLV
decoder (`bench_tools/interp_core/` and the real-engine guests). No JSON parse, no
JSON re-serialize, no intermediate string.

A second property comes for free: the 32-byte SHA-256 root in the header makes
every frame self-verifying. The browser dashboard recomputes it with `crypto.subtle`
and rejects a mismatch, which is how the "corrupted frame is rejected" check in
`dashboard/probe_nda.mjs` proves tamper detection end to end rather than asserting
it in prose.

---

## 2. Frame layout

```
 offset  0                                                     header = 36 B
        ┌───────────────┬──────────────────────────────────┬─────────────────────────┐
        │  magic  4 B   │   merkle root  32 B              │  payload  N B           │
        │  "NMCP"       │   SHA-256(payload)               │  see §3 / §4            │
        └───────────────┴──────────────────────────────────┴─────────────────────────┘
        4e 4d 43 50     (32 bytes)

 encrypted variant: magic "NMCE" (b"NMCE", src/protocol/nda_native.rs:78) — see §7.3
```

Constants (`src/protocol/nda_native.rs`): `NDA_MAGIC` :77, `NDA_ENCRYPTED_MAGIC` :78,
`FRAME_HEADER_SIZE = 36` :79.

### 2.1 Payload shapes

```
request                          response
┌──────────┬──────────┬────────┐ ┌──────────┬──────────┬────────┐
│ method   │ id       │ data   │ │ status   │ id       │ result │
│ 1 B      │ TLV      │ TLV?   │ │ 1 B      │ TLV      │ TLV    │
└──────────┴──────────┴────────┘ └──────────┴──────────┴────────┘
                                   data and result are absent for
                                   parameterless methods (ping, …)
```

`parse_nda_request` :299 reads the method byte at `FRAME_HEADER_SIZE`, decodes the
id TLV, and decodes a second TLV as `data` only if bytes remain (`Value::Null`
otherwise). Response builders mirror this: `build_nda_response_raw` :598,
`build_nda_response_raw_text` :622, `build_nda_error_raw` :635.

### 2.2 Status byte

| byte | name | meaning |
|------|------|---------|
| `0x00` | `STATUS_OK` (:74) | `result` TLV carries the payload |
| `0x01` | `STATUS_ERROR` (:75) | `result` TLV is a string, sanitized by `sandbox::sanitize_error` before it reaches the client (`src/protocol/nmcp_binary.rs:273`) |

---

## 3. Method and notification codes

`src/protocol/nda_native.rs:58-72`. `method_name` :976 and `method_from_str` :997
map both directions for every code in the table below;
`test_method_tables_cover_every_declared_opcode` asserts that round trip, so a code
added without both arms fails the suite.

| code | constant | method | response result |
|------|----------|--------|-----------------|
| `0x01` | `METHOD_INITIALIZE` | initialize | capability object, built by `build_initialize_response(false)` — elicitation is omitted on the NDA path; `roots` is omitted on every path because it is a client capability |
| `0x02` | `METHOD_TOOLS_LIST` | tools/list | TLV-encoded tool list from a generation-keyed cache (`encoded_tools_list_result` :949) |
| `0x03` | `METHOD_TOOLS_CALL` | tools/call | `{"name", "arguments"}` data object; result is the tool's text |
| `0x04` | `METHOD_PING` | ping | empty object TLV `EMPTY_OBJECT_TLV = [0x06,0,0,0,0]` (:731) |
| `0x05` | `METHOD_LOGGING_SET_LEVEL` | logging/setLevel | empty object |
| `0x06` | `METHOD_HEALTH_CHECK` | health/check | pre-encoded health TLV (`health_result_tlv` :737) |
| `0x07` | `METHOD_RESOURCES_LIST` | resources/list | decoded via `decode_req_data` |
| `0x08` | `METHOD_RESOURCES_READ` | resources/read | `"uri"` keyed |
| `0x09` | `METHOD_RESOURCE_TEMPLATES_LIST` | resources/templates/list | |
| `0x0A` | `METHOD_PROMPTS_LIST` | prompts/list | |
| `0x0B` | `METHOD_PROMPTS_GET` | prompts/get | |
| `0x0C` | `METHOD_SAMPLING_CREATE` | sampling/createMessage | |
| `0x10` | `NOTIF_INITIALIZED` | notifications/initialized | empty object |
| `0x11` | `NOTIF_CANCELLED` | notifications/cancelled | acked and logged only: this loop handles one request at a time, so there is no in-flight call to abort and the id is not recorded into `CANCELLED_IDS` (only the JSON path records and honours it) |
| `0x12` | `NOTIF_PROGRESS` | notifications/progress | streaming |

### 3.1 tools/call data object

The dispatch path does not build a value tree for this. `extract_tools_call_fields`
:655 walks the object TLV once and returns **borrowed slices** for `name` and
`arguments`, in any key order, with either field optionally absent. That is the
zero-alloc hot path: `parse_nda_request_inplace` :494 + `skip_tlv_value` :402 let
ping, tools/list, health and tools/call be served without a single `serde_json::Value`
allocation per request.

---

## 4. TLV value encoding

Encoder `encode_json_value` :123, decoder `decode_json_value` :181 → :185.
Integers and floats are separate tags so no round trip through `f64` is needed.

| tag | type | wire form |
|-----|------|-----------|
| `0x01` | string | `01` + `len:u32be` + UTF-8 bytes |
| `0x02` | signed int | `02` + `i64` big-endian, 8 B |
| `0x03` | bool | `03` + 1 byte (`00`/`01`) |
| `0x04` | null | `04` alone |
| `0x05` | array | `05` + `count:u32be` + each element TLV |
| `0x06` | object | `06` + `count:u32be` + per pair: `keylen:u16be` + key + value TLV |
| `0x07` | float | `07` + `f64` big-endian, 8 B |

Numbers: an `i64`-representable JSON number is tag `0x02`; otherwise `0x07`; a
number that is neither decodes to `0i64` (:134-143). Key length is `u16` — keys
longer than 65 535 bytes are a hard error (:160-166).

Numeric tags carry **no length prefix** — tag then 8 raw bytes. That is the single
most common guest-side decoding mistake, because the separate flat codec (§11,
`encode_flat_value`) is little-endian and also has no prefix but reuses tag `0x05`
for float. A guest that reads `02` + `u32be len` + 8 bytes fails every numeric
argument on this frame format; see §8.3.

### 4.1 Parser limits

| limit | value | constant | enforcement (server parser only; `client/src/transport/nda_codec.rs` and both dashboard JS ports check none of these three) |
|-------|-------|----------|-------------|
| nesting depth | 32 | `TLV_MAX_DEPTH` :177 | `decode_json_value_inner` :185 and `skip_tlv_value_inner` — the second one matters: the zero-alloc walker had a stack-overflow hole that was fixed separately |
| string length | 10 000 000 B | `TLV_MAX_STRING_LEN` :178 | :198 |
| array / object elements | 100 000 | `TLV_MAX_ELEMENTS` :179 | checked before the loop, so a hostile count cannot drive an allocation |

Carrier-level caps: HTTP enforces `max_request_size` (`src/transport/http.rs`);
stdio rejects a length prefix above 10 MB and closes (`src/protocol/json_rpc.rs:579-584`).

---

## 5. Worked example — one real frame, byte by byte

Request built by `dashboard/probe_nda.mjs` for `tools/call` `echo {message: "hello nda"}`,
id `"req-1"`. **106 bytes total.**

| offset | len | field | bytes |
|--------|-----|-------|-------|
| 0 | 4 | magic | `4e 4d 43 50` → "NMCP" |
| 4 | 32 | merkle root | `ff 5f 3d 37 c1 1a df 06 41 6c 89 ee 82 b2 8c 53 b7 7e ce 0f 96 a0 28 18 60 04 40 56 d9 ec 82 7f` |
| 36 | 1 | method | `03` → tools/call |
| 37 | 10 | id TLV | `01 00 00 00 05` + `72 65 71 2d 31` → "req-1" |
| 47 | 59 | data TLV | see below |

data TLV expanded (59 B):

| bytes | decode |
|-------|--------|
| `06 00 00 00 02` | object, 2 pairs |
| `00 04 6e 61 6d 65` | key (u16 len 4) = "name" |
| `01 00 00 00 04 65 63 68 6f` | value string = "echo" |
| `00 09 61 72 67 75 6d 65 6e 74 73` | key (u16 len 9) = "arguments" |
| `06 00 00 00 01` | value object, 1 pair |
| `00 07 6d 65 73 73 61 67 65` | key = "message" |
| `01 00 00 00 09 68 65 6c 6c 6f 20 6e 64 61` | value string = "hello nda" |

Response, 62 bytes (status `00`, the same id echoed, string result):

```
4e 4d 43 50                                   │ magic "NMCP"
00 × 32                                       │ zero-filled root (§7.2)
00                                            │ status = STATUS_OK
01 00 00 00 05 72 65 71 2d 31                 │ id TLV, echoed verbatim
01 00 00 00 0a 68 65 6c 6c 6f 20 6e 64 61 0a  │ result string TLV "hello nda\n"
```

Same call over JSON-RPC: request body 113 B, and the response is an
`{"jsonrpc","id","result":{"content":[{"type","text"}]}}` envelope rather than the
bare text. The binary payload here is 70 B (everything after the 36-byte header);
the other 36 bytes are magic + integrity root, which the JSON path has no
equivalent of.

---

## 6. Carriers

The same frame bytes go over three different pipes. Nothing in
`nmcp_binary::dispatch_nda_request` (:140) knows which one carried it.

```
                    ┌───────────────────────────────────────────────────────┐
  shared memory ───▶│ src/protocol/nmcp_binary.rs:724  is_nda_frame(&raw) ? │  else JSON-RPC
  stdio ───────────▶│ src/protocol/json_rpc.rs:531   peeked u32be length,   │  else JSON line mode
                    │                                  36 < len < 10 MiB    │
  HTTP ────────────▶│ src/transport/http.rs:388      POST /v1/mcp/nda       │  octet-stream body
                    └───────────────────────┬───────────────────────────────┘
                                            ▼
                        nda_native::parse_nda_request_inplace  (:494)
                          magic check · Merkle verify (when enabled) · TLV walk
                                            ▼
                        dispatch by method byte (§3)
                                            ▼
                            ┌───────────────────────────┐
                            │ registry::call_tool_binary │  (:744)
                            └───────────────────────────┘
```

- **Shared memory** (Windows `Global\VELOCITY_NMCP_REQ_*` / `*_RES_*` events,
  `src/ipc/shmem.rs:115-116`): the frame is detected by magic; the JSON path stays
  available on the same region, so a client can migrate incrementally. This is
  where NDA pays off most — there is no HTTP or kernel copy to hide the codec cost.
- **stdio**: length-prefixed frames (4-byte BE length, then exactly that many
  bytes). Responses are framed the same way. The mode is picked from the first
  peeked bytes (`src/protocol/json_rpc.rs:527-541`); a first length that is not a plausible
  frame size means JSON line mode.
- **HTTP**: `POST /v1/mcp/nda` with `Content-Type: application/octet-stream`.
  Parse failure ⇒ HTTP 400; a well-formed frame whose tool fails ⇒ HTTP 200 with
  `status=1`, so integrity errors and execution errors stay distinguishable.

---

## 7. Integrity, encryption, and the toggles

### 7.1 Merkle root

The header field is `SHA-256(payload)`. `build_nda_frame` :758 fills it when
merkle is enabled, and both parsers (`parse_nda_request` :299 and
`parse_nda_request_inplace` :494) reject a mismatch with
"NDA frame Merkle root mismatch" before any TLV is decoded — an attacker cannot
flip a payload byte without breaking the root, and cannot forge a root because it
is the only content binding. What that costs is quantified in the table below, from
the only run that isolates it (`bench_nda_2026-09-17.txt` — a local capture; raw
benchmark output is not committed, and `cargo run --release --manifest-path
bench_nda/Cargo.toml` regenerates it).

Toggle: `features.nda_merkle` (`src/config.rs:125`, default `true`), env override
`VELOCITY_NDA_MERKLE=0|false|no` (`src/config.rs:491-492`), wired at startup
(`src/main.rs:143`). With merkle off, `build_nda_frame` writes 32 zero bytes and
every validator skips the check — so the toggle is a per-deployment trade, not a
per-frame negotiation.

What the hash costs, from the same run's "Merkle cost as fraction of total NDA
pipeline round-trip" table (`bench_nda_2026-09-17.txt`, measured 2026-09-17,
10 000 iterations). That capture is a local artifact no clone carries, so its rows are
keyed here by the header the harness prints — "Merkle cost as fraction of total NDA
pipeline round-trip" — not by a line range into it. That 10 000 is not the pipeline
phases' 500: the Merkle phase floors its own count independently
(`let merkle_iters = iterations.max(10000)` in `bench_nda/src/main.rs` — cited by
expression rather than line number, because that file sits outside CI's gate and its
lines move):

| payload | NDA/HTTP round trip | of which Merkle | share | NDA/shmem share |
|---------|--------------------|-----------------|-------|-----------------|
| ping | 216.0 µs | 0.000 µs | 0.0% | 0.0% |
| 64 B | 312.7 µs | 0.228 µs | 0.1% | 11.7% |
| 1 KB | 235.6 µs | 1.145 µs | 0.5% | 41.0% |
| 4 KB | 231.0 µs | 4.436 µs | 1.9% | **127.1%** |
| 16 KB | 282.8 µs | 17.659 µs | 6.2% | 393.2% |

So integrity is effectively free wherever HTTP or stdio already dominates, and it
is the item that eats the shared-memory pipeline once payloads pass ~1 KB — which is
why the toggle exists rather than a fixed policy. Two caveats on those columns: the
table charges **two** SHA-256 per round trip ("hashes request + validates
response"), while today's server zero-fills response roots (§7.2), so a real
deployment pays about half; and the shmem column omits 64 KB+ because the output
buffer is 60 KB.

### 7.2 Response roots are zero-filled

`build_nda_response_raw` :598 and `build_nda_response_frame` :611 write
`[0u8; 32]` instead of hashing. Over TCP and shared memory the response is already
authenticated by the transport hop and the request it answers; hashing it bought a
SHA-256 per response for no added property. Client verifiers must therefore treat
an all-zero root as "not signed", not as "invalid" — the dashboard's parser reports
`skipped (all-zero)` and only verifies when any root byte is non-zero
(`dashboard/probe_nda.mjs`, `parseResponse`).

Audit provenance follows the same rule: `src/protocol/nmcp_binary.rs:204` records the frame's
root in the audit entry only when merkle verification actually ran.

### 7.3 Encrypted variant (feature `oauth2`)

Frames with magic `NMCE` are AES-256-GCM. Layout: magic, root over the **plaintext**,
12-byte nonce, ciphertext (`build_nda_encrypted_frame` :787, `decrypt_nda_frame_payload`
:846). The method byte carries `FLAG_ENCRYPTED = 0x80` (:83) and is masked off
before dispatch. Receiving an `NMCE` frame without the feature compiled in is a
hard error — there is no silent plaintext fallback, and a decryption failure aborts
rather than downgrading.

---

## 8. tools/call: how a call is executed

The dashboard's "Both" mode compares encodings, so the question "did that tool
really run as binary?" is a real question, not a rhetorical one. This is the
decision tree in `src/registry.rs`.

```
 NDA tools/call(name, args_tlv)
        │
        ▼
 get_wasm_tool_language(name)  ─ runtime metadata, if already registered
   └─ none → manifest_wasm_language(name) ─ plugin manifest, otherwise
        │
        ▼
 no language at all ─────────────────────────────▶ decode TLV → JSON → call_tool
        │                                           (built-ins, plugin/native tools)
        ▼
 json_fallback_recorded(name)  ── yes ─────────────▶ decode TLV → JSON → call_tool
        │ no
        ▼
 metadata cold → register the tool source first
        │       ├─ Err ─▶ WARN + record_json_fallback(name)
        │       │         + decode TLV → JSON → call_tool
        │ registered
        ▼
 call_wasm_tool_binary(name, language, args_tlv)   ◀── TLV bytes handed to the guest
        │
        ├─ Ok(output) ─────────────────────────────▶ return it (true binary path)
        │
        └─ Err ─▶ classify the error:
                  • request-level ("tlv decode")  ─▶ decode TLV → JSON → call_tool
                  │                                 for THIS request only; the tool
                  │                                 keeps its binary capability
                  │
                  • pre-execution ("unknown … tool", "failed to register",
                    "tool not loaded", "not registered",
                    "wrapper not cached")  ─▶ retry once after re-registering the
                                             tool source into the runtime
                       ├─ retry Ok ──────────────▶ binary path restored, return it
                       └─ retry Err ─────────────▶ record_json_fallback(name)
                                                    (permanent for this process)
                                                    + decode TLV → JSON → call_tool
                  • any other error  ─────────────▶ return Err — the guest already
                                                    ran, so retrying would execute
                                                    the tool twice
```

Three properties worth defending:

1. **A tool is never executed twice by the fallback.** Every retry and every
   JSON fall-through happens only on errors that prove nothing ran. A guest that
   executed and failed surfaces its error, as it should. The up-front registration
   on the cold-metadata branch runs no tool either — it hands the source to the
   runtime and calls it once.
   The proof is the message text, so it is only as good as the producers' wording:
   a guest exception that happens to quote one of those phrases would be misread
   as pre-execution. QuickJS's `TypeError: … is not a function` used to be on the
   list for exactly that reason and was removed — the guest throws it from inside
   tool bodies. The structural fix is for the entry point to report "nothing ran"
   out of band rather than in prose.
2. **The language lookup is metadata-then-manifest** (`manifest_wasm_language`).
   Runtime metadata (`WASM_TOOL_METADATA`, `src/plugins/mod.rs`) is written only by
   a successful `register_tool`, so on a fresh server it is empty for every tool.
   Keying dispatch on it alone meant the first NDA call of a tool quietly took the
   decoded-JSON path, so the binary capability the server advertises was never
   exercised — and never falsifiable either.
3. **Registration is retried off the hot path.** Tool sources are registered by
   the JSON `tools/call` path; a metering trap (instruction-budget exhaustion)
   evicts the cached runtime and silently drops those registrations. Instead of
   adding a registration check to every call, the binary path registers up-front
   when metadata is cold and retries once on the failure that eviction produces
   (`register_wasm_tool_source` in `src/plugins/mod.rs`), which is also why
   `rust.rs` is the one runtime without a register-unchanged dedup — it is read
   and compiled per call, so it must not be poked on the hot path.

### 8.1 Capability discovery

`GET /v1/nda/capabilities` (`src/transport/http.rs:902`, route :1530) reports, per
WASM plugin tool, whether a binary (TLV) executor is expected:

```json
{
  "protocol": "NDA",
  "binary_executors": 22,
  "json_fallback": 0,
  "tools": [
    { "name": "perl_greet", "language": "perl", "binaryExecutor": true },
    { "name": "py_text_analyzer", "language": "python", "binaryExecutor": true }
  ]
}
```

`binaryExecutor` is `false` for a tool whose binary attempt was permanently recorded
as pre-execution-failing — that is now the *only* way it goes false. Until 2026-10-09
a language blocklist (`NO_BINARY_EXEC_LANGUAGES`) also forced it false for six
tree-walk tools whose guest entry point was broken; the guest is fixed and the
blocklist is deleted (§9.3). A client uses the flag to decide whether a
JSON-vs-binary comparison is meaningful; the dashboard marks such tools `· json`,
skips the NDA leg of "Both" mode, and states
why instead of showing a difference that is not one
(`dashboard/index.html`: `loadTools` :436 adds the `· json` badge and the explanatory title at :452-454, `loadCaps` :468 fills the no-executor set, and `callTool` :480 skips the NDA leg at :497-500).

Measured on a cold server (2026-10-02): **22 WASM tools → 16 binary, 6 fallback.**
Measured 2026-10-09 in-process through `registry::nda_binary_capabilities()`, the
function this route serves, with `plugins/` loaded: **22 → 22 binary, 0 fallback.**
Only one thing moves that number, and it is a real capability gap rather than a
payload. A request-level rejection (§8, `binary_failure_is_request_level`) never
moves it: measured across the 16 binary-capable tools plus deliberate wrong-shaped
calls (`{a, b}` instead of `{action, input}` to `lua_string_utils`), the counts went
16/6 → 15/7 exactly once, at the `analyze_logs_go` leg, and held at 15/7 through
every rejection after that. Before the two classes were split, one such probe
permanently downgraded a working Lua tool from binary to fallback.

The report itself is built from each plugin *manifest* (`nda_binary_capabilities`,
`src/registry.rs`, reads `tool.executor.language`), so on a cold server it can carry
one claim this host cannot honour. That is now a starting hypothesis rather than a
standing error, because dispatch exercises the claim on the tool's first NDA call
instead of skipping the branch: `analyze_logs_go` is advertised `binaryExecutor:
true` while nothing has run, its first NDA call tries registration, that fails with
`Compilation failed: TinyGo not found`, the tool is recorded as needing the decoded
path, and the report reads **21 binary / 1 fallback** with `analyze_logs_go=false`.
`go_text_stats` — same language, working guest — is unaffected, and the WARN line
names the tool, language and reason. Recording is per tool rather than per language,
so the report converges on the host's real capability one claimed-binary tool at a
time, each costing a single decoded-JSON serving. Whether a TinyGo-present machine
runs that leg as binary is not measured here.

### 8.2 Guest-side argument isolation

A guest with two entry points must declare, on each one, where its arguments come
from. The Lua guest (`bench_tools/lua_wasm/lua_wasi.c`) hosts every Lua tool in one
`lua_State`: a JSON call parses the static `args_buf` through `get_tool_args()`,
while a binary call pre-decodes its TLV into the registry field
`__tool_args_binary`, which `get_tool_args()` prefers. Nothing used to clear that
field, so after any NDA call every later call in the process — JSON-RPC included —
was handed the *previous* request's arguments. Measured on the pre-fix guest, a
binary call to `lua_math_tools {operation:"sqrt", value:9}` left `{"error":"unknown
operation: "}` for the following call and made `lua_string_utils` echo `AGAIN` from
an earlier request.

Both entry points now retire the slot: `lua_wasi_call_tool` nulls
`__tool_args_binary` before parsing `args_buf`, and `lua_wasi_call_tool_binary`
nulls it once the wrapper has consumed it. The rebuilt guest passes the four-step
cross-path sequence (binary call, the same tool over JSON-RPC, a wrong-shaped
payload, then a valid one — §9.1) and the 22-tool differential in §9
(`dashboard/diff_nda.mjs`, which calls every tool over both transports). Its legs run
JSON first and then NDA, so it would also diverge on a Lua tool if the slot survived a
call, but it is the Rust-side four-step sequence that pins the order.

### 8.3 Numeric TLV framing in the Lua guest

`lua_math_tools` returned correct results over NDA while logging
`WARN binary arguments rejected … TLV decode error` on *every* request: the tool was
advertised binary and was in fact served by the decoded-JSON fall-through each time.
The guest's `TLV_TAG_INTEGER` and `TLV_TAG_FLOAT` branches read a `u32be` length
before the 8 payload bytes, a form this frame codec never emits — `encode_json_value`
pushes tag then 8 big-endian bytes with no prefix (§4). Any `i64` below 2^32
therefore decoded as `len = 0`, failed the `len != 8` check, and aborted the parse.
The string, array and object branches matched the codec, which is why
`lua_string_utils` and `analyze_logs_lua` never showed the defect and why it survived
the guest's original integration.

The float branch had a second bug behind the first: it `memcpy`'d the wire bytes
straight into a `double`, which byte-swaps them on a little-endian WASM host. Both
branches now read 8 raw bytes and assemble them big-endian, the float through a
`u64` bit pattern.

Two things made this hard to see, and both are why §9.1 exists: the pre-fix
metadata-keyed dispatch skipped the binary branch on a tool's first call, so the
error looked like normal traffic; and a request-level rejection is *designed* to be
invisible to the client, so correct output was no evidence of a binary execution.
`interp_core`, the C core behind the six tree-walk guests, already decoded numeric
tags correctly (`read_i64_be`), so the framing mistake was confined to the Lua guest.
Measured after the rebuild, on a fresh server: `sqrt(16)` → `4`, `2 ^ 10` → `1024`,
`2.5 ^ 2` → `6.25`, with zero `WARN` lines for any Lua tool, and the §9 differential
unchanged.

One consequence for §9.1: the wrong-shaped payloads that used to demonstrate the
request-level fall-through now decode cleanly in the guest, so the tool itself answers
them. `lua_string_utils {a:1, b:2}` over NDA returns `{"error":"unknown action: "}`
with no `WARN` and no capability change — a guest argument error, not a framing
rejection.

---

## 9. Reference matrix — measured 2026-10-02, revised 2026-10-09

`dashboard/probe_nda.mjs` (6 checks), `dashboard/diff_nda.mjs` (the 22-tool
NDA-vs-JSON payload differential over every WASM tool, §8.1/§8.3), and the dashboard
suites, against freshly started servers:

| outcome | count | detail |
|---------|-------|--------|
| NDA frame codec + transport | 6/6 | `probe_nda.mjs`: ping, tools/list (40 tools), echo call, honest error path, corrupted frame → HTTP 400, garbage bytes → HTTP 400. Re-run 2026-10-10, 6/6, and its **exit status** is now part of the check (see §9.4) |
| NDA-vs-JSON payload differential | **22/22 identical**, exit 0 | `dashboard/diff_nda.mjs` is the runner now in the working tree (not yet committed — see the open-work note at the end of §9), written 2026-10-09 because the repo had no runner behind this row's earlier 16-tool figure. Measured twice the same day: against the long-running release server on `:3900`, and against a debug build of current source on `:3903` — 22/22 payload-identical on both, exit 0. One tool errors on *both* legs: `analyze_logs_go` answers `Compilation failed: TinyGo not found`, TinyGo being absent from this machine, so the runner reports it as `identical-but-erroring` instead of counting a transport split. The old 15/16 was the runner requiring NDA `status == 0`, which made a tool broken in both paths look like a protocol divergence; the `Error type:` / `Hint:` footer asymmetry (`src/protocol/json_rpc.rs:297`) the old row described is normalized away by the runner. Re-run on 2026-10-10 against the same long-running server: 22 tools, payload identical 22, diverged or skipped 0, exit 0, the single identical-but-erroring tool still `analyze_logs_go`, and the capability report answering 15 binary / 7 fallback both before and after the pass |
| error-arm classification parity | **5/5 identical**, exit 0 (was **0/5** before 2026-10-11) | `dashboard/classify_parity.mjs` calls the same five failing tools over `POST /v1/mcp` and `POST /v1/mcp/nda` and compares the `Error type:` each returns -- the `Hint:` line is captured and printed for both paths, but the gate is the two kinds alone. Until 2026-10-11 only the JSON-RPC dispatcher appended the classification: the shared-memory and NDA sites built `Error running tool 'name': cause` and stopped there, so a client on the binary path could not tell `NOT_FOUND` from `EXECUTION_ERROR` -- which the row above could not catch, because its differential ran against 22 successful calls. Both paths now render through one helper in `src/protocol/json_rpc.rs`, which is the real reason they agree -- measured three call sites, one in that dispatcher and two in the shmem/NMCP binary file. A unit test in `src/protocol/nmcp_binary.rs` pins that helper's own classification table by calling it directly; no test drives a dispatcher's error arm, so an edit that drops one of the three call sites would not be caught by the suite. **Wire consequence:** NDA/shmem error payloads are longer by the suffix, so the bytes of an error response and the Merkle root over it changed; successful-call payloads are untouched. |
| capability self-correction | current source 22/0 → **21/1**; the stale `:3900` image **16/6 from startup** — image age, not build profile: a current-source release binary measures **22/0** cold, re-measured 2026-10-11 on `--config dashboard/demo.toml` | The cause the previous revision recorded as open is now measured, by A/B on the same machine at the same time: a server started from `target/release/velocity_mcp.exe` on `:3902` advertised **16 binary / 6 fallback** the moment it came up, while a debug build of current source on `:3903` advertised **22 / 0** with an empty fallback set. So 22/0 is a property of current *source*, and the demo server's 16/6 was never warm-state drift — that exe predates the manifest-keyed capability report. What genuinely drifts is small and specific: one differential run on `:3903` moved it 22/0 → 21/1, and the single downgrade is `analyze_logs_go`, which the §9.1 note further below now attributes to the registration arm rather than to a classifier -- with registration never having succeeded there is no runtime metadata, so `call_tool_binary` takes the metadata-absent branch at `src/registry.rs:764`, warns and records at `src/registry.rs:769`, and returns at `:777` without ever reaching the two classifiers at `:785` and `:792`; re-measured 2026-10-13 on a debug build, where the same 22/0 → 21/1 cost exactly one warn line and a second pass read 21/1 as its starting state. `record_json_fallback` (`src/registry.rs:626`) is the only writer of `false` and has no removal site, so the verdict is per-process. The six tree-walk greets keep `binaryExecutor: true` on current source across that run, and are `false` only on the old binary. On `:3902` the count fell to 13/9 after the malformed-arguments probe below, which is why that probe is opt-in. |
| gated tree-walk tools | 6 → **0 on current source**, 6 on the stale `:3900` image | the host blocklist that routed these six to decoded JSON is deleted, so on current source they advertise as binary executors and stay that way through the differential. On the release exe they are `binaryExecutor: false` from startup, which is the state the dashboard's capability gate exercises. `test_interactive.mjs` now pins both halves: one block selects `perl_greet` as the running server lists it, the other drives the page's own `noBinary` set so the skipped-leg rendering is asserted even on a cold 22/0 server (`docs/DASHBOARD.md`) |
| dashboard suites | `probe_nda.mjs` 6/6, `diff_nda.mjs` **22 tools / 22 payload-identical / 0 diverged**, `test_dashboard.sh` **14 / 0**, `test_cdp.mjs` **5 / 0**, `test_interactive.mjs` **38 / 0** -- all five re-measured 2026-10-11 on a rebuilt release binary (exit 0 each): the two node probes against a bare `--mode http` server, the three browser suites against `--config dashboard/demo.toml`, because that page opens as a `file://` document whose `null` origin the default CORS predicate rejects | The interactive suite gained the SSE wire-frame leg, which is why its count moved from the 37 this row previously published to **38**; the four capability-gate checks added in the prior revision are inside that total. The same harness against the older image still serving port 3900 returns **36 passed / 2 failed, exit 1**, and both red legs are the SSE checks -- that pair is the control for the double-framing fix, and it is also why the capability-gate block is server-list-driven instead of naming a tool. Today's two re-runs answer the already-running server, so they gate the dashboard pages and the live transport rather than this round's source, and the CDP suite's SSE leg is a page-log check that structurally cannot see double framing -- the wire-frame assertion inside `test_interactive.mjs` is the leg that can. None of the DOM/CDP legs reach the tree-walk guests: their only WASM tool is QuickJS `js_calculator` (`dashboard/index.html:674`) **2026-10-11 image-age control, same harness, two servers:** the interactive suite against the older image still on `:3900` returns **36 / 2 with exit 1** and both red legs are the SSE framing checks (`0 comment / 2 data-framed`), while against the current-source release build the same two legs are green (`1 comment in 0.0s, 0 double-framed`). So the double-framing is carried by that stale image, not by current source, and the row's earlier hedge that these legs "gate the live transport rather than this round's source" no longer holds for the browser suites, which today ran against a server built from this tree. **Re-measured 2026-10-13, all five suites, against a debug build of current source** (`target/debug/velocity_mcp.exe`, 40,583,168 B, mtime 2026-10-13 06:34 local, which postdates every build input: `find src crates benches tests Cargo.toml Cargo.lock -newer target/debug/velocity_mcp.exe` prints 0 lines, so this is a build of this tree and not a stale image the way the `:3900` exe is) served on a spare port from a config outside the repo -- `[http] addr = "127.0.0.1:3911"`, every other value copied from `dashboard/demo.toml` so the run stays comparable to the `:3900` attestations -- and every verdict below is attributed to *that* server by counting its own log's line growth across the run (375 → 521 → 674 → 791), not by trusting the command line: `probe_nda.mjs` **6/6**, `diff_nda.mjs` **22/22 identical, 0 diverged** with the same single `identical-but-erroring` tool, `test_dashboard.sh` **14/0**, `test_cdp.mjs` **5/0**, `test_interactive.mjs` **38/0**, exit 0 each. The interactive suite's auth leg needed a tooling fix to run: `dashboard/test_interactive.mjs:23` hardcoded `target/release/velocity_mcp.exe` for its second server, and with no release exe on disk the suite returned **35 passed / 1 failed, exit 1** whose only red was `auth server failed to start — auth path untested`; that line now honours `VELOCITY_DASH_EXE`, and pointing it at the debug build reproduced the published 38/0. Two things these numbers are not: the latencies the CDP and interactive legs print are from an unoptimised build and appear here only because their assertions check distribution *shape*, not as product timings; and the auth server's `--addr 127.0.0.1:3901` beats `demo.toml`'s `0.0.0.0:3900` at `src/main.rs:156`, which is what makes that leg unable to collide with the operator's `:3900` demo.**Re-measured 2026-10-14, six suites, against a debug build of current source** (`target/debug/velocity_mcp.exe`, 45,229,056 B, mtime 14:28 local, `find src crates benches tests Cargo.toml Cargo.lock -newer target/debug/velocity_mcp.exe` printing 0 lines) served on `127.0.0.1:3912` from a config outside the repo: `test_codec.mjs` **30 passed / 0 failed** (new offline codec-parity gate, no server, no network), `probe_nda.mjs` **6/0**, `diff_nda.mjs` **22/22 identical / 0 diverged** with the same single `identical-but-erroring` tool and capabilities **21/1 → 21/1** unmoved, `test_dashboard.sh` **14/0**, `classify_parity.mjs` **5/5**, `test_interactive.mjs` **38/0**, exit 0 each. Two attribution notes from that pass. An earlier interactive run this round printed **36 passed / 2 failed** with both red legs being the SSE checks, and the cause was the *harness default* rather than the tree: that invocation carried no port argument, so it read `http://127.0.0.1:3900` -- exactly the stale image this row's own control describes -- and re-pointed at 3912 both legs are green. And `dashboard/classify_parity.mjs` had to be fixed before its verdict could be read from its exit code at all: `process.exit(0)` on a 5/5 green run aborted inside libuv teardown and returned **127**, the same abort `dashboard/probe_nda.mjs` documents for its own suite, so the line now sets `process.exitCode` and a green run measures exit 0. |
| workspace test suite | **1,170 passed / 0 failed / 55 ignored** at default features, measured 2026-10-13 on the current tree (`REAL_EXIT_WS=0`, 31 result rows) with the main crate's own `--lib` row at **765 / 0 / 38**; the CI feature set reads **1,243 / 0 / 55** over 31 result rows, measured 2026-10-13 on the current tree and re-measured 2026-10-14 on the finished #615 tree at the identical total (`REAL_EXIT_FEATURED=0`; this crate's own two lib rows re-ran the same day at 765 / 0 / 38 default and 832 / 0 / 38 featured, both unmoved, and fmt plus the all-features clippy leg stayed at 0); the same pass left the default workspace figure at its 2026-10-13 reading rather than restating it, because that leg exited 101 fail-fast after 15 of 31 rows, 1,069 passed / 1 failed / 48 ignored, on the #618 guest-registration flake -- a truncated walk is not a new total, and that single red is not this tree's failure; the last featured measurement was **1,235 / 0 / 55** on 2026-10-12, and the seven cases that landed since (one lib witness, one integration test, one more lib witness, one `#[cfg(windows)]` lib witness, that round's pair and the #564 schema-declaration pin) had been carried as a derived 1,242, which this leg confirms to within the one oauth2-gated witness #568 adds -- **73** above the measured default, the profile gap this row has always cross-checked, widened by one because #568 sits in a feature-gated module the default profile does not compile; on this host's basis, the CI matrix's ubuntu and macOS legs each compile one fewer because the #560 witness is Windows-gated. Superseded readings kept for the drift: default 1,154 / featured 1,226 on 2026-10-12, both pairs re-run on the #525/#526 tree, and because #525's pin lives inside the `http`-gated module it lands in the featured profile alone: the default totals are unmoved at 760/0/38 and 1,154/0/55 while featured took the +1, which is how the two profiles went from 71 apart to **72**; the ignored count stays 55 in both; the pass before them re-ran after the pagination-overflow fix landed one witness in the main crate and one in the dispatcher test file, and `--lib` moved with them | the default total moved 1,148 -> 1,149 -> 1,150 -> 1,152 -> **1,154**, the CHANGELOG's most recent gate bullets each publishing tests above the last (one, one, the overflow-band cursor pair, then #523's walk pair), and `--lib` with it, 754 -> 755 -> 756 -> 758 -> **760 / 0 / 38**. The 756 figure was re-measured four times on 2026-10-12 before the registry-test locks landed, then eleven more times with them in place -- one gate run plus two five-run stability loops -- exit 0 at the identical 756/0/38 every time; the 758 row was that pair with the two cursor witnesses on top and the 760 row adds #523's two walk-vs-decoder pins -- it is what the five legs of the 2026-10-12 gate run report (fmt 0, clippy exit 0 on CI's literal all-targets feature-set command, 760/0/38, 1,154/0/55 over 30 test-result rows, 1,226/0/55 over the same 30, and the main crate's own featured lib row 826/0/38 -- the +1 that row and the featured total carry is #525's pin, while #526 changed no test's premise and so moved no total: it replaced fifteen unsynchronised process-environment writes in the lib test binary with one `ENV_TEST_LOCK` guard per test, and enrolled the two `load_with_env` readers that writer-only serialising left exposed -- the leg it fixed was the one that had been printing 759 passed / 1 failed). The 791/1 `--include-ignored` reconciliation two clauses below is a 2026-10-11 statement built on the old 754 and has **not** been re-derived: against this row's 2026-10-13 read of 765 + 38 = 803 it is eleven tests behind, so what it can still tell you is which single attribute is red, not what the tally totals. That 1,148 / 1,219 pair was re-measured after the classifier fix landed two more tests, the transport-parity fix a third and the notification-contract tests a fourth and fifth: default went 1,142 -> 1,145 -> 1,148 and featured 1,213 -> 1,216 -> 1,219, so the newest addition at that measurement was the +3 passed / +2 ignored of `client/tests/notification_test.rs` alone, which reports 3 passed / 0 failed / 2 ignored identically in both profiles and leaves the `--lib` binary unmoved (754 / 0 / 38 default, 819 / 0 / 38 featured); exit 0 over 30 test binaries each, the new one being that file, and the two profiles still differ by exactly 71 tests, which is the check that neither has silently lost a binary and that the new file is feature-invariant; the 55 "ignored" in both totals are 54 `#[ignore]` attributes (38 in the main crate's `--lib`, 6 in `tests/real_tool_test.rs`, 8 in `client/tests/shmem_integration_test.rs`, 2 in the new file) plus one `ignore`-tagged doctest, and **53 of the 54** pass today when run with `--include-ignored` -- re-measured on this tree rather than carried: `cargo +stable test --lib -- --include-ignored` -> 791 passed / 1 failed in 37.67 s, the single red being `wasm_runtime::go::tests::test_compile_go_to_wasm` with TinyGo absent, reconciling exactly against the normal run's 754 + 38 = 792 -- and the new file's server-backed pair is green (5 passed / 0 failed in 1.53 s against the release exe built today); the reason the count moved from 50-of-52 to 53-of-54 is the `plugins::tests::test_wasm_py_list_operations` expectation fixed this round plus these two new attributes, and the feature-set command is literally `.github/workflows/ci.yml:58` (this supersedes the 2026-10-09 pair of 1,144 and 1,211 and the 1,141 this row published on 2026-10-10; the single test between 1,141 and 1,142 is `protocol::nda_native::tests::test_method_tables_cover_every_declared_opcode`, named rather than subtracted, and the difference against 1,144 is arithmetic the CHANGELOG shows test-for-test, not a suite that stopped running; the featured total was re-measured the same evening, after the two newest classifier tests landed, so it described the tree as at then -- and the deferral recorded in the CHANGELOG was itself wrong about its own cost, because an incremental featured run takes **2 m 27 s**, not the clean-rebuild disk cost that justified skipping it): `cargo test --workspace`, then `cargo test --workspace --features http,database,oauth2`. Covers the §9.1 malformed-payload test, the interpreter binary/JSON differential and its capability-gate half, and since 2026-10-12 the overflow-band cursor pair that pins the saturating page arithmetic on both the direct handlers and the JSON-RPC dispatcher, and since the same day the two walk-vs-decoder pins that record which over-cap TLV frames the zero-alloc path forwards, plus #525's `cancelled_notification_leaks_across_http_sessions`, the featured-only test that records what a `notifications/cancelled` does across two HTTP sessions today. The gap between the two runs is all in the main crate; it was decomposed on the superseded pair as 61 more `src/lib.rs` unit tests (747 → 808) and 6 more `tests/integration.rs` (39 → 45), and that per-binary breakdown belongs to those 2026-10-09 figures — it has not been re-derived against the current pair, so treat the 61/6 split as indicative rather than as a check on the two numbers above. A default `cargo test` never compiles the `oauth2` module at all (`src/lib.rs:29`, `default = []` in `Cargo.toml:45`), so only `.github/workflows/ci.yml:58` gates it. Both commands above are now the CI commands: `ci.yml`'s build and test steps gained `--workspace` the same day, after measuring that they had gated only the root package's 853 tests and skipped 291 across the four sibling crates (CHANGELOG, same date) |
| zero-alloc TLV walk vs decoder caps | **3 frames, both verdicts differ** (decoder `exceeds maximum`, walk `Ok`) -- pinned, not fixed | `skip_tlv_value_inner` (`src/protocol/nda_native.rs:406`) re-reads the same string-length and element-count headers as `decode_json_value_inner` with `from_be_bytes` (`src/protocol/nda_native.rs:420`, `src/protocol/nda_native.rs:443`, `src/protocol/nda_native.rs:454`) but enforces neither `TLV_MAX_STRING_LEN` (`src/protocol/nda_native.rs:178`) nor `TLV_MAX_ELEMENTS` (`src/protocol/nda_native.rs:179`), so a 100,006-byte array, a 300,008-byte object and a 10,000,006-byte string are refused on the allocating parse path and walked-and-forwarded on the in-place one, where `extract_tools_call_fields` (`src/protocol/nmcp_binary.rs:209`) and `extract_tlv_field` (`src/registry.rs:2106`) hand the complete over-cap TLV to the guest. Band: the smallest such frame is 100,006 bytes against a request region of `OUTPUT_BUFFER_OFFSET` (`src/ipc/shmem.rs:12`) minus `INPUT_BUFFER_OFFSET` (`src/ipc/shmem.rs:11`) = 4,080 bytes, so shmem cannot carry it and the 10 MiB `default_max_request_size` (`src/config.rs:326`) can. The comment that asserted the walk mirrors the decoder is corrected in place, and `mod zero_alloc_walk_value_caps` (`src/protocol/nda_native.rs:3107`) pins the measured verdicts in `walk_accepts_over_cap_values_the_decoder_refuses` (`src/protocol/nda_native.rs:3144`); making the walk strict is #524, wire-visible, in the bless-or-revert batch alongside #495 and #516 (the shared C core caps TLV elements at 100k silently today) |
| frame-executor built-in arms vs the dispatcher's payload caps | **2 tools, both verdicts differ** (dispatcher `exceeds maximum` / `File too large`, frame arm `Ok`) -- pinned, not fixed | `execute_cached_nmcp_frame` (`src/registry.rs:2059`) re-implements `bench_echo` and `file_read` in its zero-alloc arms (`src/registry.rs:2105`, `src/registry.rs:2115`) and calls `bench_echo_impl` (`src/registry.rs:2039`, an unbounded `String::repeat`) and `file_read_impl` (`src/registry.rs:2043`, an unbounded `read_to_string`), so neither `MAX_BENCH_SIZE` (`src/registry.rs:1203`) nor `MAX_FILE_READ_SIZE` (`src/registry.rs:929`) is reached on that leg; `tlv_to_i64` (`src/registry.rs:2012`) additionally decodes a signed `i64` and the arm casts it to `u64` (`src/registry.rs:2107`), so a `size` of -1 carried in a frame demands `u64::MAX` bytes while the dispatcher's `as_u64()` read (`src/registry.rs:1202`) defaults the same number to 64 (`src/registry.rs:1201`). Reach is not the wire: the dispatcher matches both tool names (`src/registry.rs:922`) before its frame-cache lookup (`src/registry.rs:1948`), so the only in-tree caller that reaches the arms is the `--benchmark` harness (`src/benchmark.rs:882`, `src/benchmark.rs:974`), which is consistent with this section's 22-of-22 payload-identical NDA-vs-JSON differential. It is reachable through the published crate API (`pub mod registry`, `src/lib.rs:44`). Two tests pin the verdicts: `test_binary_frame_paths_skip_the_json_payload_caps` (`src/registry.rs:6237`) drives 16,777,217 bytes and a 10,485,761-byte file through both legs, and `test_binary_frame_bench_echo_negative_size_reaches_the_repeat_cast` (`src/registry.rs:6282`) pins the decode and the cast without running the allocation, because `handle_alloc_error` aborts rather than unwinds. Capping the two impls is #563, crate-API-visible, filed in the bless-or-revert batch alongside #524 |
| built-in tool schema vs dispatch-arm parameter census | **16/16 reconcile**, 0 read-but-undeclared, 0 declared-but-unread, exit 0 (was 1 undeclared) | `scripts/check_tool_schema_params.py` compares each `input_schema`'s declared properties against the keys its own dispatch arm reads, in both directions: a read-but-undeclared key is invisible to any client that builds its call from the schema, a declared-but-unread key is a promise the server does not keep. It found one case in the whole corpus: `directory_tree` read `maxDepth` at `src/registry.rs:993` (`.as_u64().unwrap_or(10).min(20)` -- default 10, cap 20) while `src/registry.rs:343`-`src/registry.rs:354` advertised only `path` and `excludePatterns`, so a working, already-tested knob no client could discover. Repaired by declaring it on 2026-10-13. Live effect, measured against the current debug build over `--mode stdio`: `tools/list` still answers 40 tools, the same 40 the frame-codec row above counts, `directory_tree`'s compact entry measures 575 bytes and the added property costs 104. Neither tool count nor binary count moves -- both transports serialize from the same `get_tools()`, so the 22/22 payload-identical differential two rows up is structurally untouched and no tool crosses between the binary and fallback sets. `test_directory_tree_schema_advertises_its_depth_knob` (`src/registry.rs:6301`) pins the three advertised keys and, because an advertised cap that is not enforced is a worse lie than none, pins the ceiling itself: a 21-level tree asked for at `maxDepth: 25` must emit the `(max depth 20)` marker of `src/registry.rs:1014` and must not reach the deepest file. Deleting the declaration turns it red on the `maxDepth` assertion at `src/registry.rs:6317`, leg exit 101. The Node comparison server hides its own identical knob (`bench_nodejs/server.js:109` implements default 10 / cap 20, `bench_nodejs/server.js:205`-`bench_nodejs/server.js:212` advertises two keys) -- recorded as a peer gap rather than edited, because that file is the benchmark baseline whose numbers this document quotes. |
| markdown table structure vs each table's own header | **32 files, 101 tables, 718 body rows, 0 hard, 0 advisory**, exit 0 (was 0 rows checked in `docs/API.md`'s 21-row method census, plus one unclosed row here) | `scripts/check_md_tables.py` audits cell count against the header of the table each row belongs to, in both directions. It exists because the citation census is blind to the defect: merging two adjacent rows of this very section into one line left the census answering the byte-identical tuple it gives on a clean tree, since resolution asks about the anchor's *target* line, not about whether the citing line is still a well-formed row -- and GFM drops a merged row's surplus cells silently, so the rendered table stops saying what the bytes say. Two hard classes: a body or delimiter row whose count differs from the header, and a line that neither opens nor closes a pipe yet carries exactly the header's count, i.e. a row that lost its leading pipe. That second class was priced before it shipped: across all 32 owned pages, **0** non-blank lines sit in any table's post-block gap at either width, so it has no noise floor here. Three advisories: a row that opens without closing, a delimiter row with no column markers, a run of pipe-initial lines whose second line is not a delimiter. Two findings came back about the instrument itself, both recorded because they are the class of bug that makes a green auditor useless. Its first delimiter pattern demanded three or more dashes per column with the alignment colons outside it, so it rejected the legal \|--------\|:--:\|---\| spelling, declared this document's own method-census table in `docs/API.md` not a table at all, and skipped **21 real rows** while printing a denominator smaller than what it had looked at; the corpus read went from 100 tables and 697 rows to 101 and 718 once the pattern tested each cell, and to the 719 this section's own tally reported the moment this row was added -- a figure that has since moved one body row off, to the 718 printed today, inside a later docs round that never re-ran this gate, which nothing here can catch because these gates read structure while the arithmetic lives in prose. And the one repo defect it found on first run was pre-existing, not new: a row of this section opened itself and never closed, repaired by appending the missing trailing pipe, line-neutral, no cell text changed. Verified in both directions: a fixture outside the repo carrying one instance of every class reads 6 tables, 16 body rows, 3 hard, 2 advisory, exit 1 (that fixture is the 2026-10-13 run's own and no longer exists on this host, so the class coverage was re-attested 2026-10-14 on a fresh outside-repo fixture carrying one instance of each of the five classes, which reads 1 file, 2 tables, 5 body rows, 3 hard, 3 advisory at exit 1 -- both hard classes and all three advisories firing), while the repaired tree reads the zero pair above on two consecutive runs with byte-identical output, alongside the census at 796 checked / 619 resolved / 0 hard / 169 advisory / 8 allowed, 58 relative links across 25 files, and every gated JSON example parsing -- a companion set that belongs to the 2026-10-13 run that added this row rather than to its standing header, and re-measured 2026-10-14 on the finished #615 tree the same instruments read census 1086 checked / 854 resolving to a populated line / 0 hard / 220 advisory / 12 allowed, 59 relative links and 55 anchor claims across 31 files, 96 gated JSON blocks across 32 owned pages, and 16 tools with a declared schema against 16 with a production dispatch arm, with all eight doc and script instruments exit 0 and byte-identical output on consecutive runs. Not wired into CI, in the same standing owner call as the other doc and script gates. |

Per language, on the wire:

| group | tools | NDA leg takes the binary entry point | served via decoded JSON |
|-------|-------|--------------------------------------|-------------------------|
| real engines | python (4), javascript/TypeScript (5), lua (3), ruby (1), rust (1), go (2) | yes — no fall-through `WARN` | — |
| tree-walk interpreters | `perl_greet`, `r_greet`, `php_greet`, `java_greet`, `julia_greet`, `csharp_greet` | yes since 2026-10-09 (§9.3) | — |

"Binary entry point" means the request reaches `call_tool_binary` for that tool; it
does *not* mean the guest decoded the TLV. 13 of the 22 tools do decode in the sandbox
— Lua (3), the six tree-walk guests, and MicroPython (4) since its guest was rebuilt to
export the binary entry (§9.2); the other 9 are host-decoded. See §9.2. That 13 is a
*this working tree* count: the rebuilt MicroPython guest and the `Makefile.wasi` line that
exports its symbol are both uncommitted, so a clean clone of main decodes 9 in the sandbox
and puts the four Python tools on the host fall-through.

### 9.3 The tree-walk guests' binary entry point, fixed 2026-10-09

Root cause, in one line: `interp_register_tool`
(`bench_tools/interp_core/interp.c:2676`) keeps a single global `tool_source` slot
and ignores the name, while `interp_call_tool_binary` looked the tool name
up in the interpreter scope it had just reset and never evaluated that stored
source — so the binary entry point answered `ERROR: Unknown tool '<name>'` before
any guest code ran. The JSON entry point of the same guest worked, which is why these
tools stayed callable.

Fix: both entry points now call one shared `exec_registered_tool()` that runs the
registered source in the scope holding the bound arguments, so the two encodings
cannot diverge again by construction. The tool name stays advisory (a guest holds one
source) — the honest statement of the gap is that this group is single-tool per
instance, which is a guest design property, not the defect. Six rebuilt guests
(`bench_tools/{perl,r,php,java,julia}_wasm/*.wasm`, `csharp_wasm/dotnet.wasm`, 271 KB
each, one `wasi-sdk` clang call apiece), the deleted `NO_BINARY_EXEC_LANGUAGES`
blocklist in `src/registry.rs`, and
`tests/binary_protocol_test.rs::test_interpreter_guests_binary_matches_json` ship
together. The gate is deliberately two-part, because payload equality alone is
vacuous: a guest that fails before executing is retried through decoded JSON and
returns the same text. So the test also asserts `binaryExecutor == true`, and that
half was proven non-vacuous by re-running it against the pre-fix `perl.wasm` from
`git show HEAD:` — red at the capability assertion — before restoring the rebuilt
guest.

Open work this exposes: what remained was re-measured the same day, against a debug build
of current source on `:3903` — `test_interactive.mjs` is **37 passed / 0 failed, exit 0**,
and `dashboard/diff_nda.mjs` reports **22/22 payload-identical**. The §9 rows that talk to
the already-running server were re-greened the same day. One item is genuinely still open,
and it is a build, not a test: `target/release/velocity_mcp.exe` predates this change and
still advertised 16/6. The file lock named here as the blocker is no longer the question —
that artifact has since been removed with the rest of `target/` during a disk cleanup,
while the server process started from it goes on serving from its loaded image. So what
remains is a from-scratch release build rather than a relink, and until one exists any
running demo server answers with the guests that were on disk when *it* started. That is the
same effect §9.2 measures from the other side: a capability count read back from a live
process is evidence about that process, not about the tree.

### 9.1 How "it ran as binary" is actually observed

Both encodings return the same text, so a passing call proves nothing on its own.
Three discriminators exist, and the log is only one of them. Measured on the fresh
server that ran the §9 differential plus the probes below:

| probe | what happened | evidence |
|-------|---------------|----------|
| `lua_math_tools {operation:"power", value:2, exponent:3}` | binary entry point, TLV decoded in the Lua guest | result `8`, no `WARN` for the call |
| `lua_string_utils {a:1, b:2}` | binary entry point; the *tool* rejected the argument shape in-guest | `{"error":"unknown action: "}`, no `WARN`, capability counts unchanged (15/7) |
| `tools/call` frame whose `arguments` object declares a 60 000-byte string the frame never carries | rejected during field extraction, before any guest is reached | status byte `1`, body `Malformed tools/call payload: TLV skip: truncated string body`, one `WARN Rejected malformed tools/call payload (NDA-native)` |
| `perl_greet {name:"Perl"}` | measured 2026-10-02: never attempted (language in `NO_BINARY_EXEC_LANGUAGES`) — no `WARN`, no `binaryExecutor` claim. Fixed 2026-10-09 (§9.3); the same frame now decodes in the guest | host blocklist, then nothing ran |

In that process the fallback tally was exactly one `WARN` — `analyze_logs_go`,
downgraded after its registration failed (§8.1) — and zero for any Lua, Python,
JavaScript or TypeScript tool.

Third row in detail, because it was a bug: `dispatch_nda_request` used to write
`extract_tools_call_fields(req.data).unwrap_or((None, None))`, so a payload the
zero-alloc extractor could not walk was dispatched as a call to the tool named `""`,
which hit the catch-all executor and came back as `C# core engine not found` — a
confusing answer to a well-posed question. The extraction error is now reported
(`src/protocol/nmcp_binary.rs:209-224`, regression test
`malformed_tools_call_payload_is_rejected_not_dispatched`), and nothing runs.

`binary_failure_is_request_level` — the "guest rejected the payload, fall through
quietly" classifier — was a guard rather than an observed path when only the Lua
guest decoded in-guest. The reason was measured: the host extractor and the Lua guest
enforce identical limits (depth 32, string 10 MB, 100 000 elements —
`src/protocol/nda_native.rs:177-179` and `bench_tools/lua_wasm/lua_wasi.c:352-354`),
and every field is walked before dispatch, so a payload a guest would refuse is
refused at the frame first. Since 2026-10-09 the six tree-walk guests decode in-guest
too, and the branch is reached: passing a `0x99` type tag straight to
`registry::call_tool_binary` for `perl_greet` produced the guest's own refusal, the
decoded-JSON retry, and then the host codec's `Unknown TLV type tag: 0x99` — with the
tool's `binaryExecutor` still `true` afterwards. Same shape for `lua_math_tools`. So for
the two groups that answer with a *refusal*, the classifier's contract holds: a rejected
payload costs one request, never the capability. A guest that **traps** instead of
refusing breaks that contract, and MicroPython does — see the downgrade measured below.

That probe is an **internal-call** measurement and does not survive the HTTP boundary,
which is the distinction §9.2's runner now makes. Measured 2026-10-09 by
sending a `0x99` tag inside a `tools/call` frame to `/v1/mcp/nda` for all 22 tools: every
one was refused with `Malformed tools/call payload: TLV skip:` before any guest ran,
because `extract_tools_call_fields` (`src/protocol/nda_native.rs:646`) calls
`skip_tlv_value` on every field value first, so an unknown tag fails at the frame walk.
The 0x99 probe therefore has no committed producer and cannot be replayed by a reviewer
over the wire. `dashboard/diff_nda.mjs --poison` sends a frame that *does* get through the
host walk: `arguments` is extracted as a raw TLV slice with no shape requirement, so
encoding it as an empty TLV array (`0x05,0,0,0,0`) arrives at whichever decoder reads it.
That probe is advisory, not a proof of decode site, and the measurement is what showed it:
on current source `perl_greet` answered `{"error":"undefined variable 'name' at line 1"}`
and `lua_math_tools` `{"error":"unknown operation: "}`. Both prove the guest evaluated its
program, but neither proves the guest decoded the TLV, because a host-decoded call hands
the same guest a JSON `[]` and the missing field produces the same complaint. Only an
explicit `TLV decode failed or not an object` refusal or a trap separates the cases, and
the decode-site split in §9.2 continues to rest on the export census for that reason.

The probe is off by default because it is not read-only. Measured on a debug build of
current source: one malformed frame for `py_unit_converter` moved its capability record
from `binaryExecutor: true` to `false`, and `analyze_logs` on the same run trapped
(`exceeded resource limits or trapped`) without being downgraded. On the release server a
full pass took the report from 16/6 to 13/9, adding `analyze_logs` and
`py_unit_converter` to the six greets. A trap on malformed input is the guest's own doing
— MicroPython raises through a WASM trap rather than returning a refusal, and the runtime
is then discarded — so the runner reports it and leaves the capability counts to the
before/after line rather than pretending the run was passive.

The default pass is not passive either, which re-running it on 2026-10-13 showed. With no
poison leg at all, the first pass moved `analyze_logs_go` from `binaryExecutor: true` to
`false`, and the second pass read `21/1` as its *starting* state. Nothing malformed was
sent: the frame was a well-formed `tools/call`, and the guest never ran because its
registration fails on a host without TinyGo
(`Failed to register WASM tool 'analyze_logs_go': Compilation failed: TinyGo not found`).
That is the registration arm of `call_tool_binary` (`src/registry.rs:764-777`), and it
records the decoded-JSON fallback the same way and for the same lifetime as the
pre-execution arm does: the record has no removal site, and every later binary call for
that name short-circuits at `src/registry.rs:756` without re-attempting registration, so
the verdict stands for the life of the process. The two arms are not treated alike,
though: the pre-execution arm re-registers and retries first (`src/registry.rs:792-799`),
because it concedes a cold or evicted cached runtime is indistinguishable from a guest
with no binary entry point, while the registration arm latches on the first failure.
Whether a transient registration failure should be retried — per call, or on a TTL — is
the open half of #558. The differential itself was unaffected: 22 tools, 22 payload
identical, 0 diverged, on both passes.

### 9.2 Where the TLV is actually decoded

"Binary arguments" means two different things, and only one of them holds for every
advertised tool. The wire claim holds for all 22: the client sends TLV, and no JSON is
parsed or serialized in the transport. The guest claim does not: **13 of the 22 tools**
decode the arguments inside the sandbox — the three Lua tools, the six tree-walk
interpreter tools (§9.3) and the four MicroPython tools (`py_text_analyzer`,
`py_unit_converter`, `py_list_operations`, `analyze_logs`). The other nine are
host-decoded: JavaScript (4) and TypeScript (1) through QuickJS, whose guest exports
`qjs_tool_call` but not `qjs_tool_call_binary`, and Go (2), Ruby (1) and Rust (1),
which have no binary entry point at all. Both halves of that split are measured, but
from static inputs: the per-runtime basis is the export census below, read from the
`.wasm` bytes, and the tool counts are read from the plugin manifests on 2026-10-09.
Which halves are observational has now been settled: the differential in §9 ran against a
debug build of current source (`:3903`) and against the release server (`:3900`), and all
22 tools — the four MicroPython ones included — returned payloads identical to their
JSON-RPC twins, so the Python row is now observed on the wire rather than derived from its
artifact. The *decode site* still rests on the export census, for the reason §9.1 gives.

| runtime | binary export the host looks for | present in the configured guest? | who decodes the arguments |
|---------|----------------------------------|----------------------------------|---------------------------|
| Lua | `lua_wasi_call_tool_binary` | yes, on both sides (680 060 B here, 680 094 B committed) | the guest, in C (§8.2) |
| tree-walk (Perl/R/PHP/Java/Julia/C#) | `{perl,r,php,java,julia}_wasi_call_tool_binary`, `dotnet_wasi_call_tool_binary` | yes, on both sides (six files: 276 888-277 005 B here, 277 630-277 747 B committed) | the guest, in C. The `ERROR: TLV decode failed or not an object` refusal (§9.3) was observed by handing a malformed frame to `call_tool_binary` in-process; it is not reachable over `/v1/mcp/nda`, where the host walk refuses first, and no committed test produces it either way. The wire probe does show these guests evaluating their own program (§9.1) |
| Python | `mp_wasi_call_tool_binary` | **working tree only** (588 146 B, 16 exports); the committed blob is 587 776 B with 15 exports and no such symbol, and HEAD's `Makefile.wasi` still omits the `--export` line, so a rebuild from a clean clone yields the 15-export guest | the guest, in C (`bench_tools/micropython_wasm/wasi-reactor/wasi_main.c:774` on `tlv_decode_value`), measured: 7 555–7 649 ns/call vs 8 391–8 419 ns on the JSON fall-through |
| JavaScript / TypeScript | `qjs_tool_call_binary` | no — `qjs_tool_call` only (754 433 B) | host, then JSON into the guest (the `None =>` fall-through inside `src/wasm_runtime/quickjs.rs:589-598`) |
| Ruby, Rust | no override at all | — | host, via the trait default (`src/wasm_runtime/mod.rs:349-355`) |
| Go | no export lookup; its own override decodes host-side | — | host (`src/wasm_runtime/go.rs:279-284`, the same `decode_json_value` → `call_tool` shape as the trait default) |

Method: for each path in `src/config.rs:347-395`, read the export section of the loaded
`.wasm` bytes (section id 7, parsed entry by entry) for the exact export name the runtime
resolves at init (`get_function(...)` in `src/wasm_runtime/*.rs`), measured 2026-10-09
against both sides of the repository — the file on disk and `git show HEAD:<path>` — because
eight of the thirteen tracked `.wasm` artifacts are modified in this working tree, so
a size or an export read only from disk describes this machine rather than a clone. A
substring scan would not have separated the two MicroPython guests: the name occurs in more
than one section, which is why the census walks the export section entry by entry instead of
counting occurrences. Absent means the runtime's own `None =>` branch or the trait
default runs. Corroborated 2026-10-09 by a second, behavioural probe — in-process only,
no committed producer, and unreachable over HTTP for the reason §9.1 gives: send a
deliberately malformed TLV frame (a `0x99` type tag) straight to
`plugins::call_wasm_tool_binary` and read whose decoder refuses it. The Lua and
tree-walk guests answer in their own words; JavaScript answers with the
host codec's `Unknown TLV type tag: 0x99`
(`src/protocol/nda_native.rs:288`), which is only reachable if the host decoded. Python
answered that way until the 2026-10-09 rebuild: `mp_wasi_call_tool_binary` is now in the
export section, and the A/B in §9's table below is only reachable through the guest's
decoder — the host-decoded fall-through returns all 40 siblings whatever the guest code
says, which is why `test_tlv_sibling_containers_do_not_accumulate_depth` had to be proven
red against a rebuilt HEAD guest rather than trusted on a green run alone.

So `binaryExecutor: true` in `GET /v1/nda/capabilities` is accurate about the
transport and optimistic about the interpreter boundary: for nine of the advertised
tools the TLV is decoded by the host and re-serialized as JSON one layer deeper, which
is the serialization the binary protocol set out to remove. Closing the remaining gap
is guest work, not Rust work, and the MicroPython rebuild on 2026-10-09 proved the shape
of it: `mp_wasi_call_tool_binary` (`bench_tools/micropython_wasm/wasi-reactor/wasi_main.c:774`,
on its own `tlv_decode_value` at `:696`) already existed in source — the shipped blob
just never exported it, because `Makefile.wasi` omitted the symbol — and adding it to the
export list switched those four tools onto the guest's decoder. JavaScript and the
TypeScript tool that shares its guest are in that same position, with one important
difference about where the source lives:
`bench_tools/quickjs_wasm/src/c/interface.c:1492` defines an exported
`qjs_tool_call_binary` (TLV → JS objects, no `JSON.parse`) and the host already resolves
it (`src/wasm_runtime/quickjs.rs:463-466`, used at `:590`), so the wiring is a build rather than a design
task — but that `src/` directory is **not** ordinary tracked source. The parent repo
records it as a gitlink (`mode 160000`, commit `00c4171`) with no `.gitmodules`, so a clean
clone gets an empty directory and `git submodule status` exits fatal; the pointer targets
`github.com/vercel-labs/quickjs-wasi`, whose `main` is one unpushed local commit ahead, so
upstream cannot serve that SHA to anyone else. Measured on the shipped
`bench_tools/quickjs_wasm/quickjs.wasm` (754,433 B, 445 function exports): `qjs_tool_call`
is present, `qjs_tool_call_binary` is **not** — so the blob in the repo was built without
it, and the 253 lines that add it exist only in that clone's uncommitted working tree
(`git diff --numstat` reports 253/0 against `c/interface.c`). "One guest build away" is
therefore true of this machine and false of a fresh clone, where the first step is
recovering the source. Ruby, Go and Rust are further behind: those guests have no binary
entry point to export, and Ruby and Rust resolve no export name at all, so closing them is
guest source plus host wiring.

### 9.4 Two harness defects found by running the probes

Both surfaced only because the suites were executed rather than read.

**A green probe that exits 127.** `probe_nda.mjs` finished 6/6 and still returned a
failure status. The last check leaves undici keep-alive sockets closing, and
`process.exit()` landed inside that window; on Windows libuv asserts
`!(handle->flags & UV_HANDLE_CLOSING)` (in libuv's `src/win/async.c`) and aborts. Fixed by
setting `process.exitCode` and letting Node drain. Verified both ways: 6/6 against the
live server exits 0 in 0.2 s, and against an unreachable base URL it exits 1. Worth
stating because every caller of this probe (CI, `docs/DASHBOARD.md`) trusts the status,
not the printed tally.

**A hardcoded debug port.** `test_cdp.mjs`, `test_interactive.mjs` and `screenshot.mjs`
each launched Chrome on a fixed `--remote-debugging-port` (9333/9336/9335) and then
drove whichever target `/json/list` offered — so a browser already listening there
(which the operator's own Chrome can be) would be navigated by the suite instead of the
headless instance it spawned. That hazard is why `test_cdp.mjs` had not been run since
2026-09-18. All three now use port 0 and read the real port from line 1 of
`<user-data-dir>/DevToolsActivePort`. Unblocked, it passes 5/5 with the burst latency
distribution it was written to assert. Those figures are per-run, so both measurements are
recorded rather than one: avg 1.6 ms / p50 1.5 ms / p95 2.6 ms over 50 pings on 2026-10-09,
and avg 2.53 ms / p50 2.3 ms / p95 4.5 ms on the 2026-10-10 re-run against the long-running
server. Both satisfy the assertion, and neither approaches the 10 ms quantum the leg exists
to catch.

**Which build those milliseconds came from.** Neither recorded run says, and the build
profile is worth more than the difference between them: both drive one instrument, the demo
server on `127.0.0.1:3900`, a release-profile process whose loaded image path is
`target/release/velocity_mcp.exe` (started 2026-10-09 02:58:41). Re-measured 2026-10-11 with
the same harness on the same machine within the same minute, both legs 5/5 exit 0: that
release image gives **avg 4.14 ms / p50 4.2 ms / p95 6.2 ms**, and a debug build of the
current tree (spawned for the run on `:3988`, stopped afterwards) gives
**avg 6.93 ms / p50 6.3 ms / p95 9.6 ms** — 1.7x on the mean, and that spread is why a
figure here is only comparable to another figure from the same profile, so any new one must
name its build. It also matters for how the leg's own failure mode reads: the burst check
reports `latency pinned to 10.00 ms` when the average *and* the median both come out at
exactly 10, a coincidence guard against clock quantization rather than a speed limit, and a
slower profile walks real latency toward that same pair of values. Naming the profile with
the number is what keeps a build choice from being diagnosed as a broken clock. Two
provenance facts the same run exposed: the live `:3900` process is the last copy of the
instrument behind the two recorded distributions, because the file at its image path has
since been overwritten by a later release build (13 243 904 B, 2026-10-10 19:41), so that
server can no longer be restarted as what it currently is; and the debug exe (47 076 352 B,
2026-10-10 21:31) postdates the newest `src/protocol/json_rpc.rs` edit while the release exe
predates it, which makes the debug build, not the release one, the instrument for the tree
as it stands in the working directory.

---

## 10. Implementing a client

- **Encoder/decoder**: ~40 lines. `dashboard/probe_nda.mjs` (JS, tags §4) is the
  shortest complete reference; `client/src/transport/nda_codec.rs` is the Rust one.
  The dashboard's in-browser codec is a line-for-line port of the Rust client.
- **Verify the request root before sending** if you want the server-side check to
  mean something; expect an all-zero root on responses (§7.2).
- **Warm up** with `ping` before measuring. Two fresh connections race the accept
  loop on Windows and one pays a scheduling tax that looks like an encoding
  difference and is not one — that is why the dashboard pings both endpoints on
  connect and runs the "Both" legs sequentially.
- **Error handling**: check the status byte, not the HTTP code, for execution
  failures. Status 1 means the frame was valid and the tool failed.
- **Do not assume a binary executor exists.** Read `GET /v1/nda/capabilities` (§8.1)
  rather than inferring it from the language name.

## 11. Where to look in the code

| concern | file |
|---------|------|
| frame + TLV codec, merkle, encryption | `src/protocol/nda_native.rs` |
| method dispatch, shmem loop, audit provenance | `src/protocol/nmcp_binary.rs` |
| JSON-RPC parity (same tools, text encoding) | `src/protocol/json_rpc.rs` |
| HTTP routes (`/v1/mcp`, `/v1/mcp/nda`, `/v1/nda/capabilities`) | `src/transport/http.rs` |
| tool execution + capability gating | `src/registry.rs` |
| WASM runtime dispatch | `src/plugins/mod.rs`, `src/wasm_runtime/` |
| shared-memory region + events | `src/ipc/shmem.rs` |
| benchmark harness for the pipelines | `bench_nda/`, `src/benchmark.rs` |
| browser codec + probes | `dashboard/probe_nda.mjs`, `dashboard/index.html`, `dashboard/test_interactive.mjs` |

A "flat" codec (`encode_flat_value` :1037, `build_flat_request` :1157) also exists
in `nda_native.rs`. It is a benchmark comparison only — no server dispatch path
reads it, so do not implement it as a client.
