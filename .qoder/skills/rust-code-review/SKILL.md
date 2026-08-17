# Rust Code Review

## Description
Review Rust code changes in the V.E.L.O.C.I.T.Y. NMCP Server for correctness, safety, performance, and adherence to project conventions. Use when reviewing changes to protocol handlers, IPC subsystem, tool registry, or benchmark code.

## When to Use
- Reviewing a PR or code change set
- Evaluating agent-generated code quality
- Before merging changes to high-risk areas (unsafe blocks, IPC state machine, binary parsing)
- When checking if a new module follows project conventions

## Review Checklist

### 1. Project Conventions
- [ ] Code is formatted with `cargo fmt`
- [ ] No clippy warnings (`cargo clippy -- -D warnings`)
- [ ] Public items have `///` doc comments
- [ ] Module has `//!` module-level docs
- [ ] Naming follows Rust conventions (snake_case, PascalCase, UPPER_SNAKE_CASE)
- [ ] Error handling uses `Result<T, Box<dyn Error>>` consistently

### 2. Safety and Correctness
- [ ] No `unsafe` blocks without clear justification and safety comment
- [ ] The `unsafe` pointer casts in `nmcp_binary.rs` maintain lifetime safety
- [ ] No `unwrap()` or `expect()` in production paths (benchmarks may use them)
- [ ] Error types are specific and informative
- [ ] Resource cleanup is handled (Drop, RAII, file handles)

### 3. Architecture Compliance
- [ ] Changes respect module boundaries (protocol ↔ ipc ↔ registry)
- [ ] New tools are registered in `registry::get_tools()` and `call_tool()`
- [ ] New protocol modes are dispatched from `main.rs`
- [ ] Shared memory layout changes update all offset constants consistently
- [ ] C# delegation path is handled correctly (or made configurable)

### 4. Performance
- [ ] No unnecessary allocations in hot paths (shmem loop, binary parser)
- [ ] `black_box()` used correctly in benchmarks
- [ ] No blocking calls in the polling loop beyond `thread::sleep`
- [ ] Buffer copies are minimized (zero-copy where possible)
- [ ] `stdin.lock()` acquired once, not per-line

### 5. IPC Safety
- [ ] State machine transitions are correct (no skipped states)
- [ ] `flush()` called after data writes and before state changes
- [ ] Buffer overflow checks present on all read/write operations
- [ ] Length fields use consistent little-endian encoding
- [ ] No concurrent write contention by protocol design

### 6. High-Risk Area Checks

#### Unsafe Binary Parsing (`src/protocol/nmcp_binary.rs`)
- [ ] Minimum size check (≥ 36 bytes) is present
- [ ] Magic signature validation is exact (`b"NMCP"`)
- [ ] Pointer casts preserve lifetime bounds
- [ ] No buffer overread possible via crafted input

#### Shared Memory State Machine (`src/ipc/shmem.rs`)
- [ ] State constants are correct (0-4)
- [ ] Buffer offsets match the documented layout
- [ ] Input/output length bounds are enforced
- [ ] File creation sets correct initial size (64KB)

#### C# Delegation (`src/registry.rs`)
- [ ] Process spawn uses piped stdin/stdout
- [ ] Response parsing handles missing fields gracefully
- [ ] Error propagation preserves error messages
- [ ] Non-zero exit codes are reported

## Severity Levels

| Level | Meaning | Action |
|-------|---------|--------|
| **Critical** | Memory safety bug, data corruption, security issue | Must fix before merge |
| **High** | IPC deadlock potential, buffer overflow, architecture violation | Must fix before merge |
| **Medium** | Missing error handling, performance regression, convention violation | Should fix before merge |
| **Low** | Style, naming, documentation gaps | Can fix in follow-up |

## Output Format

```
## Code Review: <module/file>

### Summary
<1-2 sentence overview>

### Issues Found

#### [Critical/High/Medium/Low] <title>
- **File**: `path/to/file.rs:L42`
- **Issue**: <description>
- **Fix**: <suggested fix>

### Verdict
<PASS / PASS WITH COMMENTS / REQUEST CHANGES>
```
