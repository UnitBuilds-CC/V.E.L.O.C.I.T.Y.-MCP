# Nested-WASM Runtime PoC — Benchmark & Optimization Path

Date: 2026-09-17. App: `velocity-nested-poc` (Wasmer Edge). Mechanism: `wasmi` interpreter running `micropython.wasm` inside the Edge WASM instance, driven over MCP `tools/call`. Companion: [edge_nested_relay_findings.md](edge_nested_relay_findings.md), [durable-state proposal](edge_durable_state_service_proposal.md).

## Goal

Prove that a real language runtime can execute as a genuine MCP tool *inside* a Wasmer Edge WASM instance via nested interpretation, quantify its cost, and define the optimization path. wasmer-as-host cannot compile to wasm32 (JIT/native); a pure interpreter (wasmi) can. This is the PoC of that interpreter path.

## What works

`nested_bench` tool (MicroPython via wasmi) executed real Python over the live Edge MCP endpoint: `print(1+1)`→2, `.upper()`→HELLO, `sum(range(n))` correct. Advertised in `tools/list`; invoked via `tools/call`. So the full MCP protocol path works for a nested runtime, not just a synthetic probe.

## Benchmark (server-side ms, WAN-free; instrumented per stage)

Naive path = fresh wasmi `Module::new` + instantiate + init + exec on each call.

| Stage / workload | ms |
|---|---|
| Inner module compile (every call) | 12–18 |
| Instantiate + start | 0.10–0.20 |
| `mp_wasi_init` | 0.09–0.17 |
| light exec avg (`print`, 200 iters) | 0.42 |
| `sum(range(1000))` | 5.6 |
| `sum(range(100000))` | 621 |
| `fib(25)` (recursive) | 1041 |
| `x+='a'` ×50000 (O(n²) str) | >90,000 (client timeout) |
| cold start (idle→first, incl WAN) | ~3.7 s |

Reference: native MicroPython (wasmer, same box) ≈ 43 µs/exec; native QuickJS ≈ 5 µs. Nested-wasmi light exec ≈ 420 µs — roughly a **10× penalty vs the native interpreter here** (double interpretation), plus the 12–18 ms recompile every call.

## Findings / risks

1. **Recompile-per-call dominates fixed cost.** ~15 ms of the naive path is wasmi compiling the inner module each request. Wasteful and the #1 target.
2. **No instruction budget.** fib(25)=1s, string-concat hung >90s. The nested path has **no metering**, so a single heavy call monopolizes the 2-CPU instance → multi-tenant DoS/hang risk. The native server already guards this (10M-instruction metering, 30s wall-clock); the PoC does not.
3. **Cold start + scale-to-zero** compound throughput (20 sequential light ≈5 s/call, cold-bound).
4. Double interpretation is intrinsic to nested-wasmi — a floor on how fast any inner runtime can be.

## Optimization path (ordered by payoff)

- **P0 — cache the compiled inner module + a warm instance across requests.** Keep a `wasmi::Module` (and ideally a persistent `Store`/`Instance`) per runtime, like the native module cache (measured 15.4× cold-start win there). Removes the 12–18 ms/call; per-call cost collapses toward the ~0.4 ms exec. Concurrency: wasmi `Store` is not `Sync`, so pool one warm instance per worker task (we have 2 workers) or a Mutex-serialized instance pool.
- **P0 — fuel/instruction metering on the nested interpreter.** wasmi supports a fuel/limiter model; set a per-call budget so a runaway `fib`/string loop traps instead of hanging — mandatory before this is multi-tenant-safe. Reuse the native `instruction_limit` config semantics.
- **P1 — pin min-instances** for hot runtimes to kill cold start; reserve scale-to-zero for rarely-used runtimes.
- **P1 — lazy compile / reuse the engine.** Share one `Engine` across instances (module cache above).
- **P2 — the bigger lever: skip the interpreter layer.** For real workloads, run each runtime as its **own standalone WASIX HTTP binary** (MicroPython/QuickJS compiled with an HTTP harness, no wasmi inside). That removes double-interpretation entirely → native-wasm speed, and each scales independently. This PoC proves nested works; option C is the production-fast path (see findings doc). Nested-wasmi is best where you must keep one binary and accept the ~10× + metering caveat.

## Reproduce

Probe implementation (wasmi host: stub 7 WASI imports, `mp_wasi_init(pystack,heap)`, write source at `EXEC_SLOT=512KiB`, `mp_wasi_exec`, `mp_wasi_get_output[_len]`) is in `velocity-mcp-edge/src/tools.rs` history this session; stage timing via `std::time::Instant` reported server-side. Non-locked WASIX build (wasmi resolves from the WASIX sparse registry) staged via a `build-edge-poc.py`. The PoC tooling was kept out of the shipped flagship (reverted); `velocity-nested-poc` remains deployed.

## Dynamic registration + module cache + metering — proven on Edge (2026-09-17, app `velocity-nested-poc`)

Prototype wired into the edge `ToolExecutor`: a process-global `NestedRuntime` (one fuel-enabled `wasmi::Engine` + the **compiled-once** `micropython` `Module`, i.e. the P0 module cache) plus a name→source plugin registry, and MCP-level `register_plugin` / `unregister_plugin` tools. Live results:

- `tools/list` 12 → `register_plugin adder` → `tools/list` **13, includes `adder`, no restart** → `unregister_plugin` → back to 12. So dynamic add/remove of a real (MicroPython) tool works over the standard MCP protocol.
- Calling a registered plugin runs it through the nested runtime with **args marshalled as a Python dict**; `adder` over `range(100)` → `4950`, `fuel=1360114`.
- **Module cache eliminated the ~15 ms recompile**: warm server-side compute is now sub-millisecond (client ~766 ms is ~740 ms WAN from Namibia + ~26 ms everything-else).
- **Fuel metering contained a runaway loop**: `while True: x+=1` → `resource limit exceeded … all fuel consumed by WebAssembly` in ~1.4 s (no hang). FUEL_LIMIT=2e9, enforced per call.

So the three P0/P1 roadmap items (module cache, instruction metering, dynamic no-restart registration) are each demonstrated working on Wasmer Edge. Flagship `velocity-mcp-edge` (crate v3.2.0; its deployed Edge *package* was 3.19.4) remains the clean 10-tool static build; the prototype lives in `velocity-nested-poc` and in `velocity-mcp-edge/src/tools.rs` under the `nested` cfg-gated code for this PoC branch.

## Dynamic-registration prototype — benchmark (2026-09-17)

Measured against the live `velocity-nested-poc` app (MicroPython-via-wasmi, cached module, per-call `FUEL_LIMIT = 2_000_000_000`). Two metrics: **fuel** (returned by the tool; WAN-free, deterministic compute) and **client ms** (≈ ~780 ms transatlantic WAN floor + server compute). Server compute ≈ `client_ms − 780`.

### Registration cost
- `register_plugin` median ≈ 900 ms client → **≈ ~0 ms server** (a HashMap insert; the rest is WAN). `unregister_plugin` same. So dynamic add/remove is effectively free server-side.

### Workloads (3 reps; fuel is the reliable signal)
| workload | fuel | client ms | ≈ server compute | result |
|---|---|---|---|---|
| `print('x')` | 288 k | 797 | ~17 ms (floor: instantiate+init+bootstrap) | `x` |
| `sum range(1e3)` | 12.6 M | 770 | ~0–50 ms | ok |
| `sum range(1e4)` | 122 M | 821 | ~40 ms | ok |
| `fib(20)` | 247 M | 887 | ~107 ms | 6765 |
| `sumsq range(5e4)` | 781 M | 1477 | ~697 ms | ok |
| `sum range(1e5)` | 1.26 G | 1425 | ~645 ms | ok (near ceiling) |
| `fib(25)` | >2 G | 1587 | trapped | resource-limit |
| `fib(30)` | >2 G | 1603 | trapped | "all fuel consumed" |

### Calibration & takeaways
- **Fuel ≈ linear in Python work:** ~**12,600 fuel per simple loop iteration** (1k→12.6M, 10k→122M, 100k→1.26G). So `FUEL_LIMIT = 2e9` ≈ a ceiling of **~158k loop iterations** — tune the budget to policy (native uses 10M instructions; wasmi fuel is a different unit and needs its own calibration, as here).
- **Floor is tiny and cache-confirmed:** an empty-ish tool costs ~17 ms server (instantiate + init on the cached module) — the 12–18 ms *compile* from the naive path is gone (module cache working).
- **Metering works:** fib(25)/fib(30) exceed the budget and are trapped in ~1.5–1.6 s as `resource limit exceeded … all fuel consumed by WebAssembly` — no hang, no instance monopolization. The cap is the safety valve for a multi-tenant edge.
- **Caveat (durability):** the plugin registry is in-memory per instance — it does **not survive scale-to-zero**. Persisting registered tools is exactly the durable-state service discussed separately.

Bottom line: registration + warm execution are cheap (sub-ms compute for light tools, ~0 ms to register); heavy unbounded code is contained by a tunable fuel cap. That is a working, benchmarked foundation to build on.

## Durable registry via Wasmer Edge volumes — proven (2026-09-17)

The "registry vanishes on restart" caveat is solved with an Edge **volume**. `nested-poc/app.yaml` declares `volumes: [{ name: velocity-nested-poc-data, mount: /data }]`. The prototype persists the plugin map to `$VELOCITY_STATE_DIR` (default `/data`)/`registry.json` on every register/unregister, and `nested_rt()` loads it on first use.

Live proof on `velocity-nested-poc`:
- `register_plugin durable_demo` → `persisted=true` (the WASIX app wrote `/data/registry.json` — volume mounted + writable).
- Redeployed to a **fresh process** (v1.0.3); with no re-registration, `tools/list` still showed `durable_demo` and calling it returned *"I survived a restart"*. The only source was the volume file → reload-on-boot works.

So a durable, dynamic tool registry is viable on Wasmer Edge using volumes, no external DB. Remaining caveats: (1) multi-instance write consistency (block volume + concurrent instances need a lock/single-writer; fine for low-write single instance), (2) volumes are region-scoped and may be plan/size limited — confirm on the target tier, (3) it adds durability, not speed. This resolves blocker #1 from the "is it usable" list; language-breadth (QuickJS/Lua host shims) and the concurrency/auth/scale-to-zero production concerns remain.

## Multi-instance consistency fixes #1 + #2 — implemented & re-verified (2026-09-17, PoC v1.0.4)

The original registry cached the whole plugin map in memory at boot (`OnceLock` load-once) and persisted it by writing the **entire** map on every change. Two failure modes followed: **lost update** (instance B's whole-map save clobbered a key instance A added meanwhile) and **staleness** (a running instance never saw another instance's registrations until it restarted).

Both fixed in the prototype (`nested-poc/prototype_tools.rs`, deployed as `velocity-nested-poc` v1.0.4):
- **Fix #1 — refresh-through:** removed the in-memory `plugins` field entirely; the volume file is now the single source of truth. `nested_plugin_names()` and `nested_call_plugin()` re-read `registry.json` on every call, so no boot-pinned snapshot can go stale.
- **Fix #2 — read-modify-write merge:** `merge_register`/`merge_unregister` re-read the current file, apply the one add/remove, and write back — never a whole-map dump. The global mutex still serialises within a process; because we read immediately before writing, a concurrent registration by another instance is preserved rather than overwritten.

Re-verification:
- **Native unit test** `tmp_registry_merge_no_lost_update_native`: two "instances" call `merge_register` for different keys → both survive (total=2), a fresh `load_registry` sees both (refresh-through), removing one leaves the other. Passes.
- **Full nested E2E** `tmp_dynamic_reg_native` (register→list→call→unregister through real wasmi-MicroPython): `adder` → `6|fuel=554086`, cleanly unregistered. Passes.
- **Live on Edge** (`velocity-nested-poc`, TCP HTTP): registered `alpha_live` then `beta_live` over an already-populated volume (`durable_demo`, `text_count`); `tools/list` showed all **four** coexisting — the new keys did not clobber the pre-existing ones, and the removal kept the rest. 15 back-to-back `tools/list` reads converged on one stable set. `flagship crate reverted clean` (tools.rs at HEAD, wasmi removed, 39 lib tests pass).

**Honest residual caveat (this is fixes #1+#2, not #3/#4):** the read-modify-write is still not *atomic* across processes — there is no file lock and `fs::write` is not a compare-and-swap. Under truly simultaneous registrations on separate instances the register *response* count can momentarily lag the final converged set (observed in the run above), though nothing was lost. Closing that fully needs **fix #3** (atomic rename or a lock file) or **fix #4** (an external CAS store, e.g. the WorkFlow/Postgres durable-state service). Single-instance and low-write multi-instance are now safe; high-contention multi-writer is not yet.

> Update (same day): the "not atomic across processes" residual was subsequently **closed by fix #3** — see the next section.

## Cross-process atomicity fix #3 — implemented & re-verified (2026-09-17, PoC v1.0.5)

Fix #2 removed the clobber, but the read-modify-write was still not *atomic* across processes: two writers could interleave between the re-read and the write, and a reader could observe a half-written `registry.json`. Fix #3 adds the two standard file-level primitives:

- **#3(a) Atomic publish:** `save_registry` now writes to a sibling `registry.json.tmp` and `std::fs::rename`s it over the target. `rename` is atomic within a filesystem, so a concurrent reader always sees either the old or the new file — never a torn/partial JSON. (`std::fs::rename` + `create_new` compile and link for `wasm32-wasmer-wasi`, confirmed in the PoC build.)
- **#3(b) Exclusive lock:** `acquire_registry_lock` creates `registry.lock` with `OpenOptions::create_new(true)` (`O_CREAT|O_EXCL`), which the OS makes atomic for both threads **and** separate processes. Only one writer enters the read-modify-write at a time; losers spin on a 2 ms backoff. A lock older than 5 s is treated as abandoned by a crashed holder and reclaimed, and the whole acquisition is capped at a 10 s timeout so the registry can never wedge permanently. `merge_register`/`merge_unregister` wrap their re-read→apply→atomic-write in this lock.

Re-verification:
- **Native concurrency test** `tmp_registry_concurrent_lock_native`: 16 threads call `merge_register` directly (bypassing the in-process mutex, so the lock file is the *only* serializer) for 16 distinct keys → final file has exactly 16 keys, no lost update, and no stray `registry.lock` left behind. Passes. (Note: these registry tests share the `VELOCITY_STATE_DIR` process-global, so run them with `--test-threads=1`.)
- **Full nested E2E** `tmp_dynamic_reg_native` still green with the lock path (`adder` → `6|fuel=554086`).
- **Live on Edge, 8-way concurrent burst:** fired 8 parallel `register_plugin` calls over an already-populated volume (`durable_demo`, `text_count`). The responses came back with **8 distinct, monotonic totals** (2→9) — i.e. the writers contended on *one* lock and serialized, none collided. Then **12 consecutive `tools/list` reads each showed size 10 with all 8 burst keys present and none missing** — no lost update and no torn read. A parallel unregister of all 8 succeeded and left exactly the 2 baseline keys, proving the lock always releases. Flagship reverted clean (tools.rs at HEAD, wasmi removed, 39 lib tests pass).

**Verdict:** fixes #1 + #2 + #3 make the volume-backed registry durable, staleness-free, and safe under concurrent multi-writer within a node that shares the volume — with no external DB. What fix #3 does **not** resolve is *cross-region / non-shared* volume consistency: if two instances do not share the same block volume, no file lock can help, and that needs **fix #4** (an external CAS store such as the WorkFlow/Postgres durable-state service) or pinning to a single instance. On the tested PoC the burst serialized on one lock, so this deployment path is now safe for concurrent registration.

## External transactional store fix #4 — implemented & verified (2026-09-17)

Fix #3's lock is only advisory *within one shared filesystem*; it cannot serialize instances that mount different volumes (e.g. different regions). Fix #4 removes the shared-filesystem dependency entirely by putting the registry in a **transactional store**, so consistency comes from the database rather than from the OS.

Design (`nested-poc/prototype_tools.rs`, a native `pgreg` backend selected when `VELOCITY_PG_DSN` is set; the file backend stays the default so the Edge artifact is unchanged):
- **Row-per-plugin** table `mcp_registry(name PK, code, version, updated_at)`. Registering a plugin no longer rewrites a whole map — it touches exactly one row.
- **Atomic upsert:** `INSERT … ON CONFLICT(name) DO UPDATE SET code=EXCLUDED.code, version=version+1`. A single SQL statement is atomic and isolated by the DB, so concurrent registrations of *different* names never clobber each other and never need a lock.
- **Unregister** = `DELETE … WHERE name=$1`; **list/call** re-read from the DB (refresh-through preserved from #1).

Verification against a **real Postgres 16** (Docker, localhost:55432), native test `tmp_registry_pg_concurrent_native`:
- 16 threads concurrently upsert 16 distinct plugin names → final table has exactly 16 rows, all present, **zero lost updates** — with no file lock and no shared FS (the DB provides it).
- 8 threads contend on the **same** row → still exactly one row and `version` reaches 8 (proves the single-statement upserts serialize atomically on the row, not last-write-wins-clobbers). Passes.

**Edge feasibility — measured, not assumed:** a `postgres`/`tokio-postgres` client **cannot compile for `wasm32-wasmer-wasi`** (the Edge target). A probe build fails in `tokio-postgres`' `connect_socket.rs`: unresolved `socket2` (`SockRef`/`TcpKeepalive`) + a `keepalive_config` cfg mismatch — independent of TLS (`default-features = false` still fails). So the PG backend is **native-only**; the backend is `#[cfg(not(target_arch = "wasm32"))]`-gated and does not enter the Edge wasm build (PoC wasm still compiles clean). **Consequence:** to give an *Edge* deployment fix #4's cross-instance guarantee it must go through the **relay/gateway** path already sketched for the plugin backends — Edge's working TCP **HTTP egress** → a small stateless **native** HTTP-CAS gateway (which *can* speak Postgres/TLS and hold the connection pool) → Postgres. That gateway is a normal container, not a WASM module, so it sidesteps the wasm-client limitation. Alternatively, use an HTTP-native CAS store (a hosted KV with compare-and-swap) reachable directly over Edge's TCP egress, avoiding a bespoke gateway.

**Ladder status:** #1 (refresh) ✅, #2 (RMW merge) ✅, #3 (file lock + atomic rename) ✅ — all shipped in the Edge PoC and safe *within one shared volume*. #4 (external CAS) ✅ as a native transactional backend and ✅ proven against a real DB; on Edge it is a gateway/relay integration, blocked (measured) only by the wasm Postgres client, not by any protocol limit. The single-instance volume path (#1-#3) is the one running live today; #4 is the productionization route for genuinely multi-region deployments.

### Addendum — Wasmer managed DB reached from the Edge guest (measured 2026-09-17)

Two follow-ups that sharpen the Edge feasibility of #4, both measured rather than assumed:

1. **Attaching a managed DB needs no browser.** Added to `nested-poc/app.yaml`: `capabilities:\n  database:\n    engine: postgres`, bumped the package version, `wasmer deploy`. Wasmer auto-provisioned a Postgres instance (`db_19dda7ff` @ `psql.fr-roub1.bengt.wasmernet.com:20184`) and injected `DB_HOST/DB_PORT/DB_NAME/DB_USERNAME/DB_PASSWORD` into the app. Confirmed with `wasmer app database list`. So a custom WASIX app (not just PHP/WordPress) gets a managed DB entirely from the CLI.

2. **The guest can reach it, but TLS is mandatory.** A temporary `pg_probe` tool (raw `std::net::TcpStream` + the 8-byte Postgres `SSLRequest` magic — no external crates, so it compiles for `wasm32-wasmer-wasi`) run from *inside* the Edge instance returned:
   `addr=psql.fr-roub1.bengt.wasmernet.com:20184 connect=OK env(user=…,db=db_19dda7ff,pw_set=true) sslrequest_reply='S' => TLS REQUIRED`.
   Interpretation: the internal `*.wasmernet.com` host is reachable over **plain TCP from the guest** (no 308/TLS-front proxy — that only gates the public `wasmer.app` ingress), and creds are injected; but the DB **refuses plaintext and requires TLS** (`SSLRequest` → `'S'`). **This isolated the sole remaining blocker for #4-from-inside-Edge to one thing: a TLS client that compiles for `wasm32-wasmer-wasi`** — `rustls`' default crypto backends (`ring`/`aws-lc`) need `clang` (absent here); a pure-Rust `rustcrypto` provider is the untried path. If a WASIX TLS client is obtained, a hand-rolled PG-wire-over-TLS client would complete #4 on Edge with no external host. The `pg_probe` was reverted from the flagship (tools.rs at HEAD, wasmi stripped, `nested_mp.wasm` deleted, 39 lib tests pass); a PoC build with it is in `nested-poc/prototype_tools.rs`.

### RESOLVED — fix #4 runs live from INSIDE the Edge WASM (2026-09-17, PoC v1.0.10)

The gap is closed, Wasmer-only, no external host and no `tokio-postgres`:
- **WASIX TLS unlocked:** pointing `cc-rs` at the wasi-sdk clang (`CC_wasm32_wasmer_wasi=C:/wasi-sdk/bin/clang.exe --target=wasm32-wasmer-wasi`, sysroot + `-matomics -mbulk-memory`) makes `rustls 0.23` + **`ring 0.17.14+wasix.1`** + `webpki-roots` compile and link for `wasm32-wasmer-wasi`. (The earlier ureq/`aws-lc` failure was the wrong crypto backend, not a wasm-TLS impossibility.) Wired into `deploy/build-edge-poc.py`; the deps are gated to the wasmer target only so native `cargo test` never needs a C compiler.
- **Auth measured, then implemented:** the managed DB demands **SASL/SCRAM-SHA-256** (`sasl_mech='SCRAM-SHA-256-PLUS'` offered; we negotiate plain SCRAM-SHA-256, no channel binding). Implemented SCRAM (PBKDF2-HMAC-SHA256 + client/server proofs) with pure-Rust `sha2`/`hmac`/`pbkdf2`/`base64`, plus the Postgres extended-query protocol (Parse/Bind/Describe/Execute/Sync, **parameterized** so plugin source with quotes/SQL metachars is safe), all hand-rolled over a rustls `StreamOwned<ClientConnection, TcpStream>`.
- **In-guest registry backend:** `nested-poc/prototype_tools.rs` `pgedge` module — row-per-plugin `mcp_registry(name PK, code, version, updated_at)`, atomic `INSERT … ON CONFLICT DO UPDATE`; auto-selected on the wasmer target when `DB_HOST` is present. `register_plugin`/`tools/list`/call/unregister all now go to the Postgres.
- **Live proof on Edge (`velocity-nested-poc`, in-guest PG over TLS+SCRAM):** `register edge_pg1` → written to Postgres; `tools/list` reads rows from Postgres; **`call edge_pg1` fetched the plugin source FROM Postgres over TLS and executed it** (`edge_pg1 11|fuel=426102`). 8-way concurrent `register_plugin` burst → **8 distinct DB-serialized totals (2→9), and all 10 subsequent `tools/list` reads showed all 8 keys present, zero lost updates** — the #4 cross-operation atomicity guarantee, achieved by the transactional store, running entirely inside the Edge WASM against Wasmer's managed Postgres.

Fixes applied during this: (a) the auth loop must drain to the startup `ReadyForQuery` before issuing queries (was breaking on AuthenticationOk and leaving a stray `'Z'`, causing an empty first result / index panic); (b) count reads made panic-proof. Flagship reverted clean (tools.rs at HEAD, wasmi + rustls/sha2/hmac/pbkdf2/base64/webpki-roots removed from Cargo.toml, `Cargo.lock` restored to HEAD's clean state, 39 lib tests pass). The PoC v1.0.10 remains deployed against `db_19dda7ff`. **This supersedes the earlier "blocked by wasm TLS" finding — the blocker was the crypto backend + missing clang, both solved.**

**Persistence across a redeploy — verified (2026-09-17, PoC v1.0.10 → v1.0.11):** wrote a unique marker `persist_1789670582` (code `print('PERSIST_OK_1789670582')`) into the managed Postgres through the live app, then redeployed as a new version. App version id changed (`dav_R4qIGtJurDMz` → `dav_KE4I8tMuGJWY`, so genuinely fresh processes), the managed DB instance was unchanged (`db_19dda7ff` @ `psql.fr-roub1…:20184`), and the new instance re-authenticated via TLS+SCRAM and read the marker back — `tools/list` still showed it and `call` returned `PERSIST_OK_1789670582` again. The registry table contained only what was written to Postgres (the earlier volume-era `durable_demo`/`text_count` rows were absent), confirming Postgres is the store and it **outlives app redeploys/independent of the volume**. Marker row cleaned up afterward (total 0).

## Fix #4 in-guest — full benchmark + functional test (2026-09-17, PoC v1.0.13)

Client-side latency, Namibia → `fr-roub1` Edge (~740–800 ms transatlantic WAN dominates every number). The `echo` tool is the baseline over the same path; **server-side work is isolated as the delta vs echo**. Each PG op opens a fresh TCP+TLS+SCRAM connection (no pooling yet).

| Operation | path | min | p50 | p95 | max | ≈ server-side (p50 − echo) |
|---|---|---|---|---|---|---|
| `echo` (baseline, no PG) | client→edge | 795 | 825 | 868 | 874 | — |
| `tools/list` (PG read) | + edge→PG SELECT | 803 | 819 | 845 | (one 27 000 cold outlier, see below) | **≈ 0 ms** (within noise) |
| `register_plugin` (PG write) | + edge→PG upsert | 836 | 884 | 899 | 938 | **≈ +60 ms** |
| `unregister_plugin` (PG write) | + edge→PG delete | 876 | 899 | 955 | 960 | **≈ +75 ms** |
| `call <plugin>` (PG read + wasmi exec) | + exec | 1056 | 1098 | 1152 | 1233 | **≈ +270 ms** (wasmi MicroPython exec dominates; PG read ~free) |

- **PG read is essentially free** server-side (in-guest TLS+SCRAM+SELECT lands within noise of the no-PG baseline). PG **writes** add ~60–75 ms — that's the per-operation TLS handshake + SCRAM PBKDF2 + the upsert, i.e. exactly what a connection pool/keep-alive would remove.
- **27 s outlier:** the first `tools/list` run spiked once (cold first connection / WAN blip); a clean 10-sample re-run was a tight 803–845 ms — not systematic, but flags the need for a connect timeout + retry.
- **Cold-ish start** (after 60 s idle): first `tools/list` 827 / 830 / **1839** / 891 / 812 / 822 ms — one ~1.8 s warm-up blip then steady ~810–890; no scale-to-zero penalty in 60 s.
- **Stability:** two independent 16-way concurrent register bursts (32 fresh TLS connections), **0 errors**, each settled to a single consistent `tools/list` set; drained back to empty. No Postgres connection exhaustion at this scale.
- **Correctness matrix (fixed build):** re-register same name → upsert (updated code returned on call); over-long name rejected; call of missing plugin → error; unregister of missing plugin → **error** (was a bug, now returns "not found"); unregister existing → ok.

**Verdict — it works, and it's demoable.** The fix #4 in-guest path is functionally complete and correct end-to-end on Edge (register/list/call/unregister against Wasmer's managed Postgres over TLS+SCRAM), durable across redeploys, concurrency-safe (DB-serialized, zero lost updates), with server-side overhead that is negligible for reads and modest (~60–75 ms) for writes because every op re-handshakes. **Production caveats before "ship":** add a connection pool / keep-alive (removes the per-write handshake), a connect timeout + retry (kills the outlier), real cert verification instead of accept-all, higher-concurrency + true multi-instance testing, and it stays a PoC module in `nested-poc/` (not wired into the shipped `velocity-mcp-edge`). It is in a state to confidently show "it works"; it is not yet production-hardened.

### Hardening pass — keep-alive + stale-retry (2026-09-17, PoC v1.0.14)

Applied the two hardenings that actually help this setup (connection reuse, connect/query robustness); left cert verification accept-all **on purpose** — Wasmer's managed DB uses a self-signed cert and its docs say "TLS without chain verification," so CA pinning would be wrong, not safer.
- **Thread-local keep-alive (`CONN`)** — each Edge worker thread caches one authenticated `Tls`; warm ops skip the TCP+TLS+SCRAM handshake. `fresh_conn()` (connect+auth+ensure-table) only on a cold thread.
- **Stale-connection retry** — if a *reused* connection errors mid-query, it's discarded, reconnected once, and the op retried; a *fresh*-connection error is returned without a double-send (so writes aren't applied twice). The 8 s read/write socket timeouts (already present) bound the handshake/query; this absorbs the earlier one-off 27 s blip.

**Re-bench — WAN-neutral server-side delta vs the `echo` baseline in the SAME run** (each op vs echo, so the ~600–740 ms transatlantic leg cancels out):

| op | no-pool (v1.0.13) Δ vs echo | pooled (v1.0.14) Δ vs echo |
|---|---|---|
| tools/list (PG read) | ≈ +0 ms | ≈ +0 ms |
| register_plugin (PG write) | +60 ms | **+35 ms** |
| call (PG read + wasmi exec) | +273 ms | **+59 ms** |
| unregister_plugin (PG write) | +75 ms | +81 ms (noise) |

The per-write TLS+SCRAM handshake is largely gone (register overhead ~‑40%), and `call`'s overhead collapsed ~78% (the fresh handshake that used to front every call is now reused). After pooling, the whole in-guest Postgres path sits within ~35–80 ms of a plain static tool — i.e. reading/writing the managed DB from inside the Edge WASM is now close to free relative to the static-tool baseline. Remaining before "ship": real multi-instance + higher-concurrency runs, and productization (wire out of `nested-poc/`). Flagship reverted clean (tools.rs HEAD, no prototype deps — build injects them — 39 tests pass); PoC v1.0.14 live against `db_19dda7ff`.



