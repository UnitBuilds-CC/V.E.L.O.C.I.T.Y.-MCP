# NMCP Protocol Development

## Description
Develop and extend the NMCP dual-protocol system in the V.E.L.O.C.I.T.Y. NMCP Server. Covers the JSON-RPC stdio handler, NMCP binary frame parser, shared memory IPC, and tool registry. Use when adding new protocol modes, extending the binary frame format, modifying the IPC state machine, or integrating new tools.

## When to Use
- Adding a new protocol mode (e.g., WebSocket, TCP, named pipes)
- Extending the NMCP binary frame format (new fields, versioning)
- Modifying the shared memory buffer layout or state machine
- Adding new tools to the registry
- Changing the C# delegation mechanism
- Optimizing IPC latency or throughput

## Key Files

| File | Role | LOC |
|------|------|-----|
| `src/main.rs` | CLI arg parsing, mode dispatch | 92 |
| `src/protocol/json_rpc.rs` | Stdio JSON-RPC v2.0 handler | 113 |
| `src/protocol/nmcp_binary.rs` | Shmem loop + binary frame parser | 141 |
| `src/protocol/mod.rs` | Protocol module declarations | 3 |
| `src/ipc/shmem.rs` | Memory-mapped buffer + state machine | 117 |
| `src/ipc/mod.rs` | IPC module declarations | 2 |
| `src/registry.rs` | Tool definitions + C# delegation | 136 |

## Adding a New Protocol Mode

1. Create `src/protocol/<new_mode>.rs`
2. Add `pub mod <new_mode>;` to `src/protocol/mod.rs`
3. Implement the mode's main loop function (e.g., `run_<mode>_loop()`)
4. Add the mode string to the `match mode` block in `src/main.rs`
5. Update `print_help()` with the new mode description
6. Ensure the new mode uses `registry::get_tools()` and `registry::call_tool()`

## Adding a New Tool

1. Add a `Tool` struct to `get_tools()` in `src/registry.rs`:
   - Define `name`, `description`, and `input_schema` (JSON Schema)
2. Add a match arm in `call_tool()`:
   - Extract required parameters from `arguments`
   - Implement natively or delegate to C# via `execute_csharp_mcp_tool()`
3. Test via stdio mode:
   ```powershell
   echo '{"jsonrpc":"2.0","method":"tools/list","id":1}' | cargo run -- --mode stdio
   ```

## Modifying the Shared Memory Layout

**WARNING**: Changes to the buffer layout require coordinated updates to both the server and all host processes.

1. Update offset constants in `src/ipc/shmem.rs`:
   - `STATE_OFFSET`, `INPUT_LEN_OFFSET`, `OUTPUT_LEN_OFFSET`
   - `INPUT_BUFFER_OFFSET`, `OUTPUT_BUFFER_OFFSET`, `TOTAL_BUFFER_SIZE`
2. Update all methods that reference the changed offsets
3. Verify buffer capacity calculations:
   - Input capacity: `OUTPUT_BUFFER_OFFSET - INPUT_BUFFER_OFFSET`
   - Output capacity: `TOTAL_BUFFER_SIZE - OUTPUT_BUFFER_OFFSET`
4. Run the benchmark suite to verify no regression:
   ```powershell
   cargo run -- --benchmark
   ```

## Extending the Binary Frame Format

1. Update `NmcpBinaryFrame` struct in `src/protocol/nmcp_binary.rs`
2. Update the `parse()` method:
   - Adjust minimum size check
   - Add new field extraction with unsafe pointer cast
   - Validate new fields
3. Update the benchmark to include the new format
4. Document the new frame layout in the knowledge cards

## IPC State Machine Extension

Current states: IDLE (0) → REQ_READY (1) → PROCESSING (2) → RES_READY (3) → ERROR (4)

When adding new states:
1. Add a new `STATE_*` constant in `src/ipc/shmem.rs`
2. Update the polling loop in `src/protocol/nmcp_binary.rs` to handle the new state
3. Document the new state transition in the content pages
4. Ensure no two processes can write the state byte simultaneously

## Checklist for Protocol Changes

- [ ] `cargo check` passes with no warnings
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo fmt --all -- --check` passes
- [ ] Benchmark suite runs without regression
- [ ] Both stdio and shmem modes tested
- [ ] Buffer overflow protection verified
- [ ] Error paths tested (invalid input, missing tools, state machine errors)
- [ ] Documentation updated (content pages + knowledge cards)
