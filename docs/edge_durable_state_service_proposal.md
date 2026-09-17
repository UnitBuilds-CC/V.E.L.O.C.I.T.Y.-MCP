# Design Proposal: Durable State as a Wasmer-Managed Internal Service

Status: proposal / discussion. Author: VELOCITY (UnitBuilds). Date: 2026-09-17.
Companion: [edge_nested_relay_findings.md](edge_nested_relay_findings.md). Reference engine: V.E.L.O.C.I.T.Y.-WorkFlow (Rust durable-execution engine; Temporal/Restate/DBOS alternative; VCTP/UDP transport).

## Problem

Wasmer Edge compute is stateless and scales to zero, but MCP workloads need state: resumable sessions, `elicitation/create` waiting on a human for minutes, and cross-call tool/session memory. Today a user who wants that on Edge must self-host a stateful server (container + WAL + Postgres) — which is exactly what Edge is supposed to eliminate. Result: the flagship VELOCITY capabilities (dynamic tools, NDA/shmem µs transport, sessions) only run on native/Docker, so Edge stays a narrow stateless subset.

## Proposal

Wasmer offers **durable state as an internal, managed, always-on service**. An Edge app adds `enable: durable_state` and calls `state_get/state_put/state_delete` on `(object_type, key)`. The platform runs and multi-tenants the state layer; the user never operates a stateful process. This keeps compute serverless (scale-to-zero, global) while making state a first-class, shared internal service.

## Two models — pick per latency need

- **(a) State-as-a-service (RPC).** Edge function → managed state service over a network call. Works today with no new primitives; simple, multi-tenant, central single-writer-per-key serialization. Latency = one intra-region hop: ~ms (warm, keepalive) to low-tens-of-ms; plus the caller's own cold start (~1.4s measured) on first call.
- **(b) Colocated durable object (actor model).** The keyed state *is* the compute instance, with key→instance affinity, so handler and state share a node: no hop, µs-class, non-serializing (VCTP/shmem). Higher-value, but needs platform primitives Edge does not expose today.

Both solve the "no self-hosted container" problem; only (b) also solves latency. Ship (a) for viability, design toward (b).

## What already exists

**On Edge (measured this session):** WASIX socket HTTP works, multi-threaded (2 workers), live. Outbound **TCP** egress works; **UDP does not** (`send_to`→ConnectionReset); **no cross-instance shared memory** (isolated instances); cold start ~1.4s; server compute ~4ms.

**In WorkFlow (the reference engine, per code map):** key-generic WAL (`WalManager::append(type,key,bytes)`) with fsync group-commit + `recover_from_wal`; Restate-style keyed state already scaffolded but **unwired** (`virtual_objects.rs`: `ObjectKey`/`ObjectState`/`state_get`/`state_set` + awakeables + idempotency); CAS versioning idiom (`MemoStore::set_versioned` / `VersionMismatch`); `DatabaseAdapter` + `workflow_checkpoints(snapshot BYTEA, merkle_root)`; cross-host single-writer via `PgAdvisoryLockManager`. Note: `ReplayEngine` exists but is not wired to recovery, so durable-*workflow* value is mechanics-only (no deterministic user-code replay or value-level exactly-once) — durable *state* (a)-(b) is the honest, shippable win.

## API surface (proposed)

- Edge manifest: `enable: durable_state` → injects a per-instance scoped token; rejects direct external calls to the state service.
- Client: `state_get(type,key) -> {value, version}`, `state_put(type,key,value,expected_version?) -> {version} | VersionMismatch`, `state_delete`, plus a compare-and-set for concurrency. Namespaced by `(owner, app, object_type)`.
- Same service reached over: HTTP for model (a) (gRPC/HTTP sidecar); VCTP/µs path reserved for model (b).

## Mapping to WorkFlow (what each side contributes)

| Piece | WorkFlow engine provides | Wasmer platform provides |
|---|---|---|
| State model | promote `virtual_objects` to hot path (`RwLock` like `memo.rs`) | tenancy, quotas, enable-gate |
| Durability | key-generic WAL + `StatePut/StateDelete` events + state-recovery loop; snapshot via checkpoint-table shape | managed Postgres/object store, backups, HA |
| Concurrency | `set_versioned` CAS; `PgAdvisoryLock` single-writer-per-key | (b) same-node scheduler + affinity + fast IPC |
| Transport | VCTP/HTTP `StateService` beside `WorkflowService` | private low-latency mesh; colocate for (b) |

## Phasing

1. **(a) MVP, shippable now:** managed keyed state-over-HTTP, WAL + snapshot persistence, CAS. Enough for MCP sessions/elicitation; ms-class.
2. **Hardening:** awakeables (timers), durable promises (human-in-the-loop wait), idempotency — all already stubbed in WorkFlow.
3. **(b) The ask:** same-node colocation + cross-instance shared memory (or a fast local socket) → durable-object model, µs VCTP path. This is the one platform capability that turns viable→superior.

## Risks / honesty

- (a) latency is a real hop; fine for LLM tool calls, not for tight in-process fan-out. (b) is blocked purely on the colocation+shmem primitive.
- Cold start (~1.4s) is independent of the transport; mitigate with pinned min-instances for hot paths.
- The state service is genuinely stateful → it lives on the always-on/container substrate, which reinforces: stateful MCP belongs on native/container or (b); serverless scale-to-zero suits the stateless subset only.
- WorkFlow's durable-state primitives exist but are unverified in production when wired; the win is "activate + persist + expose," not a flag flip — real integration work.

## The ask (for Wasmer)

Two concrete, prioritized capabilities: **(1) expose durable state (WAL + snapshot, single-writer-per-key) as an internal managed service** — unblocks stateful MCP on Edge immediately; **(2) same-node colocation + shared-memory/low-latency IPC** — lets (1) graduate to the µs durable-object model. WorkFlow is the reference kernel/API for both.
