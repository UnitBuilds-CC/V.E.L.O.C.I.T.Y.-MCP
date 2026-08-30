# NDA Tool Registry and Native Operations

## Classification
- **Category**: Tool Dispatch / Native Operations
- **Files**: src/registry.rs, src/nda_converter.rs, src/nda_document.rs, src/nda_executor.rs
- **Criticality**: High — core tool dispatch and all NDA operations

## Summary

The tool registry defines 8 built-in tools, all implemented natively in Rust. No external process delegation is required for core operations. The server can also discover additional tools from plugins and an optional C# backend engine.

## Registered Tools

### General Tools

| Tool | Parameters | Description |
|------|-----------|-------------|
| `file_read` | `path` (required), `encoding` (optional) | Read file contents with validation |
| `file_write` | `path` (required), `content` (required) | Write files with path validation |
| `shell_exec` | `command` (required), `timeout` (optional) | Execute commands in sandbox |
| `http_request` | `url` (required), `method`, `headers`, `body`, `timeout` | HTTP client with retry/circuit breaker |

### NDA Tools

| Tool | Parameters | Description |
|------|-----------|-------------|
| `convert_to_nda_document` | `filePath` (required), `outputPath` (optional) | Convert any file to signed .nda |
| `convert_to_nda_tool` | `jsonRequest` (required), `outputPath` (optional) | Convert JSON tool call to NDA binary |
| `read_nda` | `ndaPath` (required) | Read/parse .nda with Merkle + Ed25519 verification |
| `execute_nda` | `ndaPath` (required), `arguments` (optional) | Execute .nda in sandbox |

## Native Execution

All tools execute natively in Rust:
- `file_read`/`file_write`: Direct filesystem operations with path validation
- `shell_exec`: Spawns process in capability-based sandbox with OS-level limits
- `http_request`: Native HTTP client with retry logic and circuit breaker
- NDA tools: Native compile/read/execute via `nda_converter`, `nda_document`, `nda_executor`

## Dynamic Tool Hosting

Additional tools can be discovered from:
1. **Plugins**: Installed via marketplace, loaded dynamically
2. **C# engine**: Optional external process via `VELOCITY_CSHARP_PATH` env var

On first `tools/list`, the server queries plugins and C# engine, caches results, and merges with built-in tools (deduplicated by name).

## Critical Constraints

- All file paths validated at boundary (absolute, no traversal)
- `shell_exec` and `execute_nda` run in capability-based sandbox
- 30-second execution timeout with process kill
- stdout limit: 1 MB, stderr limit: 256 KB
- Error messages sanitized before returning to clients
