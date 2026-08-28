# VELOCITY-MCP Server Upgrade Plan

## MCP Server Comparison

### What v2.0.0 (cloned repo) already does better
These should be **kept as-is** — the temp version is behind on all of these:

| Feature | v2.0.0 | Temp v1.0.0 |
|---------|--------|-------------|
| **JSON-RPC protocol** | Graceful shutdown (AtomicBool), reader thread + mpsc channel, rate limiting, audit logging, health check, 1MB request size limit, notification handling, 10 tests | Blocking stdin, no shutdown, no rate limit, no audit, no health check, no tests |
| **NMCP binary protocol** | Shutdown-aware, SeqCst fences, buffer cleanup on exit, 5 tests | No shutdown, no cleanup, no tests |
| **IPC (shmem)** | Atomic Acquire/Release on state byte, SeqCst fences for length fields, 7 tests | Plain byte reads/writes (no atomic ordering), no tests |
| **NDA core** | Standalone `nda_document`, `nda_converter`, `nda_executor`, `sandbox` modules | NDA code embedded in editor module, not standalone |
| **TLV encoding** | Full encode/decode with security limits (max depth 32, max string 10MB, max elements 100K), 5 security tests | Not present |
| **C# engine discovery** | Dynamic tool discovery via tools/list to C# engine | Not present |
| **Rate limiting** | Token bucket rate limiter | Not present |
| **Audit** | Ring buffer audit log for tool calls | Not present |

### What Temp v1.0.0 has that v2.0.0 lacks
These are the **upgrade targets** — MCP server capabilities to port:

#### 1. Expanded Tool Registry (100+ tools vs 4)

**System Tools (26 tools):**
- `read_file`, `write_file`, `list_dir`, `delete_file` — file operations
- `grep_search` — content search
- `run_command` — shell command execution
- `fetch_panel_data` — data fetching
- `agent_checkpoint_create/restore/list` — checkpoint management
- `agent_memory_reremember/recall/forget` — agent memory
- `code_generate_tests`, `code_coverage_analyze` — code quality
- `knowledge_ingest`, `knowledge_search` — knowledge base
- `workflow_run` — workflow execution
- `connector_call` — external connector invocation
- `generate_image`, `describe_image` — image AI
- `convert_to_nda` (with seal option) — enhanced NDA conversion

**Browser Tools (50+ tools):**
- Session management: `browser_create_session`, `browser_list_sessions`, `browser_get_session`, `browser_get_storage`, `browser_set_storage`
- Navigation: `web_navigate`, `browser_session_navigate`
- Runtime (Go chromedp): `browser_runtime_capture`, `browser_runtime_visual_capture`, `runtime_create_session`, `runtime_session_click`, `runtime_session_fill`, `runtime_session_evaluate`, etc.
- Snapshots: `browser_list_snapshots`, `browser_read_snapshot`, `browser_diff_snapshots`
- Auth: `browser_auth_diagnostics`, `browser_save_auth_profile`, `browser_apply_auth_profile`, `browser_reseed_auth`
- Network: `browser_get_session_network`, `browser_set_session_network`
- Cookies: `browser_get_cookies`, `browser_set_cookies`
- Tracing: `browser_get_trace_summary`, `browser_get_trace_logs`
- Visual fallbacks: `browser_read_visual_fallback`

**Windows Automation Tools (50+ tools):**
- Sessions: `wa_create_session`, `wa_get_session`, `wa_list_sessions`, `wa_save_snapshot`, `wa_capture_windows_snapshot`, `wa_read_snapshot`
- Scripts: `wa_save_script`, `wa_read_script`, `wa_list_scripts`, `wa_run_script`, `wa_read_run`, `wa_list_runs`
- Selectors/actions: `wa_resolve_selector`, `wa_plan_action`, `wa_execute_windows_action`, `wa_wait_for_windows_condition`
- Clipboard: `wa_clipboard_read/write/clear`
- Processes: `wa_process_launch/terminate/list`
- Windows: `wa_window_list`, `wa_window_action`
- Virtual desktops: `wa_virtual_desktop_list/switch`
- OCR: `wa_ocr_screen`
- Notifications: `wa_notifications_list/dismiss`
- Registry: `wa_registry_read/write`
- System: `wa_system_dark_mode`
- Triggers: `wa_trigger_register/list/fire/remove`
- Recovery: `wa_recovery_set_policy/get_status`
- Events: `wa_event_subscribe`

**Team Tools (21 tools):**
- `create_expert_team`, `list_expert_teams`, `update_expert_team`, `clone_expert_team`, `export_expert_team`, `import_expert_team`
- `add_team_member`, `remove_team_member`, `update_team_member`, `bulk_import_members`
- `create_skill_file`, `list_skills`
- `validate_team`, `check_scope_overlaps`, `debug_routing`
- `team_analytics`, `team_health_check`
- `list_providers`, `create_team_quick`, `team_changelog`

#### 2. Win32 Event-Based IPC Signaling
The temp version uses `CreateEventW`/`WaitForSingleObject`/`SetEvent` for zero-poll IPC on Windows instead of v2.0.0's 100μs polling loop. This is more CPU-efficient and lower-latency on Windows.

#### 3. Tool Execution Implementations
The temp version has actual execution code for all 100+ tools in:
- `registry/system_tools.rs` (1637 lines)
- `registry/browser_tools/` (8000+ lines)
- `registry/wa_tools.rs` (1278 lines)
- `registry/team_tools.rs` (1164 lines)

## Upgrade Strategy

### Phase 1: Registry Restructure (Priority: HIGH)
Convert the single-file `registry.rs` into a module directory matching the temp's structure:

```
src/registry/
├── mod.rs              — dispatch + re-exports
├── types.rs            — Tool struct definition
├── dispatch.rs         — call_tool routing
├── parsers.rs          — argument parsing helpers
├── system_tools.rs     — file ops, grep, command, NDA, agent, code, knowledge, etc.
├── browser_tools/
│   ├── mod.rs
│   ├── session.rs      — session management
│   ├── navigation.rs   — web_navigate
│   ├── workflow.rs     — crawl workflows
│   └── native/
│       ├── mod.rs      — native browser engine
│       ├── actions.rs  — click, fill, submit
│       ├── inspect.rs  — DOM inspection
│       ├── render.rs   — visual rendering
│       ├── learn.rs    — page learning
│       ├── assert.rs   — assertions
│       ├── wait.rs     — wait conditions
│       └── tests.rs
├── wa_tools.rs         — Windows automation
├── team_tools.rs       — expert team management
└── tool_definitions/
    ├── mod.rs           — get_tools() aggregation
    ├── system.rs        — system tool schemas
    ├── browser.rs       — browser tool schemas
    ├── wa.rs            — WA tool schemas
    └── team.rs          — team tool schemas
```

**Keep from v2.0.0:**
- NDA tool routing (convert_to_nda_document, convert_to_nda_tool, read_nda, execute_nda)
- C# engine dynamic discovery
- NDA tool registry (convert_and_register_nda_tool)
- TLV encoding/decoding
- Path validation

**Add from temp:**
- All tool definitions (schemas)
- All tool execution implementations
- Dispatch routing for new tools

### Phase 2: Win32 Event IPC (Priority: MEDIUM)
Add Win32 Event signaling to v2.0.0's IPC while keeping its atomic ordering:

- Add `CreateEventW`/`WaitForSingleObject`/`SetEvent`/`CloseHandle` FFI
- Add `wait_for_request()` and `signal_response()` methods
- Keep Acquire/Release atomic ordering on state byte
- Keep SeqCst fences for length fields
- Add `Drop` impl for handle cleanup
- Keep non-Windows fallback (100μs sleep)

### Phase 3: Supporting Infrastructure (Priority: MEDIUM)
Add modules needed by the expanded tools:

- `registry/parsers.rs` — argument parsing helpers used by tool implementations
- Browser session storage (`.velocity/browser-sessions/`)
- WA session storage (`.velocity/wa-sessions/`)
- Team storage (`.velocity/expert-teams/`)

### Phase 4: Integration & Testing (Priority: HIGH)
- Update `json_rpc.rs` and `nmcp_binary.rs` to route through new registry module
- Ensure v2.0.0 rate limiting and audit work with new tools
- Port relevant tests from temp (2000+ lines of registry tests)
- Add integration tests for new tool categories

## Files to Port (from temp → cloned repo)

| Source (temp) | Destination (cloned repo) | Lines |
|---------------|--------------------------|-------|
| `registry/tool_definitions/mod.rs` | `src/registry/tool_definitions/mod.rs` | 14 |
| `registry/tool_definitions/system.rs` | `src/registry/tool_definitions/system.rs` | 306 |
| `registry/tool_definitions/browser.rs` | `src/registry/tool_definitions/browser.rs` | 1529 |
| `registry/tool_definitions/wa.rs` | `src/registry/tool_definitions/wa.rs` | 779 |
| `registry/tool_definitions/team.rs` | `src/registry/tool_definitions/team.rs` | 303 |
| `registry/types.rs` | `src/registry/types.rs` | 10 |
| `registry/dispatch.rs` | `src/registry/dispatch.rs` | 48 |
| `registry/parsers.rs` | `src/registry/parsers.rs` | 342 |
| `registry/system_tools.rs` | `src/registry/system_tools.rs` | 1637 |
| `registry/browser_tools/mod.rs` | `src/registry/browser_tools/mod.rs` | 28 |
| `registry/browser_tools/session.rs` | `src/registry/browser_tools/session.rs` | 859 |
| `registry/browser_tools/navigation.rs` | `src/registry/browser_tools/navigation.rs` | 243 |
| `registry/browser_tools/workflow.rs` | `src/registry/browser_tools/workflow.rs` | 306 |
| `registry/browser_tools/native.rs` | `src/registry/browser_tools/native.rs` | 414 |
| `registry/browser_tools/native/actions.rs` | `src/registry/browser_tools/native/actions.rs` | 322 |
| `registry/browser_tools/native/inspect.rs` | `src/registry/browser_tools/native/inspect.rs` | 666 |
| `registry/browser_tools/native/render.rs` | `src/registry/browser_tools/native/render.rs` | 332 |
| `registry/browser_tools/native/learn.rs` | `src/registry/browser_tools/native/learn.rs` | 395 |
| `registry/browser_tools/native/assert.rs` | `src/registry/browser_tools/native/assert.rs` | 163 |
| `registry/browser_tools/native/wait.rs` | `src/registry/browser_tools/native/wait.rs` | 146 |
| `registry/browser_tools/native/tests.rs` | `src/registry/browser_tools/native/tests.rs` | 2452 |
| `registry/wa_tools.rs` | `src/registry/wa_tools.rs` | 1278 |
| `registry/team_tools.rs` | `src/registry/team_tools.rs` | 1164 |
| `registry/tests/` | `src/registry/tests/` | ~2500 |

**Total: ~15,000 lines of tool code to port**

## Dependencies to Add

```toml
# Already present in v2.0.0:
# serde, serde_json, memmap2, tracing, sha2, base64, ctrlc

# New dependencies needed for expanded tools:
pathdiff = "0.2.3"          # relative path computation
once_cell = "1.18"          # lazy statics (already using OnceLock, but some code uses once_cell)
crossbeam-channel = "0.5"   # concurrent channels for browser sessions
ureq = { version = "2.9", features = ["json"] }  # HTTP client for browser/connectors
notify = "7"                # file watching
image = { version = "0.24", default-features = false, features = ["png", "jpeg", "gif", "bmp", "webp"] }  # image tools
thiserror = "2"             # error derive

[target.'cfg(windows)'.dependencies]
windows = { version = "0.58", features = [
    "Win32_UI_Accessibility",
    "Win32_System_Com",
    "Win32_UI_WindowsAndMessaging",
    "Win32_Foundation",
    "Win32_System_Threading",
    "Win32_System_Variant",
    "Win32_Graphics_Gdi",
    "Win32_System_DataExchange",
    "Win32_System_Memory",
    "Win32_UI_Shell",
    "Win32_System_LibraryLoader",
    "Win32_System_Diagnostics_ToolHelp",
    "Win32_System_ProcessStatus",
    "Win32_UI_Input_KeyboardAndMouse",
] }
```

## What NOT to Port
These are IDE/agentic concerns, not MCP server concerns:
- `agent/` — entire agent system (background agents, planning, reasoning, peer bridge, etc.)
- `automation/` — build runner, task router, instruction registry
- `orchestrator/` — meta-agent control plane
- `compiler/` — JIT, tokenizer, parser loader
- `editor/` — egui IDE (except `editor/nda_document.rs` if it has improvements)
- `connectors/` — OAuth2, sync engine, webhook manager (the `connector_call` tool is enough)
- `security/` — secret store (can add later if needed)
- `metrics/`, `telemetry/`, `health/` — observability (v2.0.0 already has health/check)

## Estimated Effort
- Phase 1 (Registry restructure): 8-12 hours
- Phase 2 (Win32 Event IPC): 2-3 hours
- Phase 3 (Supporting infrastructure): 4-6 hours
- Phase 4 (Integration & testing): 6-8 hours
- **Total: 20-29 hours**
