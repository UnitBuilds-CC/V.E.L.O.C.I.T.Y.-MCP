# NDA Tool Dispatch and Native Operations

<cite>
**Referenced Files in This Document**
- [src/registry.rs](file://src/registry.rs)
- [src/nda_converter.rs](file://src/nda_converter.rs)
- [src/nda_document.rs](file://src/nda_document.rs)
- [src/nda_executor.rs](file://src/nda_executor.rs)
- [src/sandbox.rs](file://src/sandbox.rs)
</cite>

## Overview

The tool registry (`src/registry.rs`) defines the MCP tools available on this server and handles their execution. In v3.0, the server provides **8 built-in tools** — all implemented natively in Rust. No external process delegation is required for core operations.

## Registered Tools

### General Tools

#### `file_read`
Read file contents with path validation and size limits.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `path` | string | Yes | Absolute path to the file |
| `encoding` | string | No | Text encoding (default: utf-8) |

#### `file_write`
Write content to a file with path validation.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `path` | string | Yes | Absolute path for the output file |
| `content` | string | Yes | Content to write |
| `encoding` | string | No | Text encoding (default: utf-8) |

#### `shell_exec`
Execute shell commands in a sandboxed environment with timeout and output limits.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `command` | string | Yes | Shell command to execute |
| `timeout` | integer | No | Timeout in seconds (default: 30, max: 30) |

Commands run in a capability-based sandbox with no network access, filesystem isolated to a temp directory, and OS-level memory limits.

#### `http_request`
Make HTTP requests with automatic retry logic and circuit breaker protection.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `url` | string | Yes | Target URL |
| `method` | string | No | HTTP method (default: GET) |
| `headers` | object | No | Request headers |
| `body` | string | No | Request body |
| `timeout` | integer | No | Timeout in seconds (default: 30) |

### NDA Tools

#### `convert_to_nda_document`
Converts any file (C# source, PDF, CSV, Excel, DOCX, Image, Zip) into a cryptographically signed `.nda` binary document with semantic triples and visual display commands.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `filePath` | string | Yes | Absolute path to input file |
| `outputPath` | string | No | Output path (defaults to input with `.nda` extension) |

Implemented natively in `src/nda_converter.rs`. Supports Ed25519 signing via `compile_signed()`.

#### `convert_to_nda_tool`
Converts a JSON-RPC tool call into native NDA binary format.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `jsonRequest` | string | Yes | JSON-RPC tool call request to convert |
| `outputPath` | string | No | Path to write the NDA binary file |

#### `read_nda`
Reads and parses a compiled `.nda` binary file. Returns semantic triples, visual display commands, string pool contents, Merkle verification status, and Ed25519 signature status.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `ndaPath` | string | Yes | Absolute path to the `.nda` file |

Implemented natively in `src/nda_document.rs`. Includes `verify_merkle()` and `verify_signature()`.

#### `execute_nda`
Executes a runnable `.nda` container in a capability-based sandbox.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `ndaPath` | string | Yes | Absolute path to the runnable `.nda` file |
| `arguments` | string[] | No | Command-line arguments |

Implemented natively in `src/nda_executor.rs`. Runs in the sandbox with process isolation.

## Tool Definition Structure

Each tool is defined as a `Tool` struct with serde serialization:

```rust
pub struct Tool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,  // JSON Schema for parameters
}
```

Tools can also be registered via proc macros:

```rust
#[mcp_tool(
    name = "my_tool",
    description = "Does something useful",
    param_constraints = { "path": { "min_length": 1 } }
)]
fn my_tool(path: String) -> Result<String, String> { ... }
```

## Execution Flow

### Native Dispatch

All 8 built-in tools execute natively in Rust:

```rust
pub fn call_tool(name: &str, arguments: &Value) -> Result<String, String> {
    match name {
        "file_read" => { /* native implementation */ }
        "file_write" => { /* native implementation */ }
        "shell_exec" => { /* sandbox + native */ }
        "http_request" => { /* native with retry/circuit breaker */ }
        "convert_to_nda_document" => { /* nda_converter */ }
        "convert_to_nda_tool" => { /* nda_converter */ }
        "read_nda" => { /* nda_document */ }
        "execute_nda" => { /* nda_executor + sandbox */ }
        _ => Err(format!("Unknown tool: {}", name)),
    }
}
```

### Dynamic Tool Hosting

The server can also discover additional tools from plugins or a C# backend engine:

1. On first `tools/list`, query plugins and C# engine (if present)
2. Cache results for subsequent requests
3. Merge built-in and discovered tools (deduplicated by name)

### Sandbox Enforcement

`shell_exec` and `execute_nda` run through the capability-based sandbox:

- **ProcessCapabilities** defines allowed operations
- **Violation tracking** records all sandbox violations
- **OS-level enforcement**: Windows Job Objects (memory caps), Linux seccomp (syscall filtering)
- **Isolated temp directory** per execution, cleaned up after completion
- **30-second timeout** with process kill

## Error Handling

| Error | Cause |
|-------|-------|
| "filePath is required" | Missing required parameter |
| "File path must be absolute" | Relative path provided |
| "File path contains traversal sequence" | Path traversal attempt |
| "Sandbox violation: ..." | Operation not allowed by sandbox |
| "Execution timed out after 30s" | Tool exceeded timeout |
| "Output size limit exceeded" | stdout > 1 MB or stderr > 256 KB |

## Key Design Decisions

1. **All native Rust**: Since v3.0, all core tools run natively — no external process delegation required. The C# engine is optional for dynamic tool hosting.
2. **Sandbox by default**: All execution tools (`shell_exec`, `execute_nda`) run in the capability-based sandbox with OS-level enforcement.
3. **Retry and circuit breaker**: `http_request` includes automatic retry logic and circuit breaker protection for resilience.
4. **Path validation at boundary**: All file paths are validated before any tool receives them — empty, relative, and traversal paths are rejected.
5. **Error sanitization**: Error messages are sanitized before returning to clients (paths stripped, truncated at 500 chars).

**Section sources**
- [src/registry.rs](file://src/registry.rs)
- [src/nda_converter.rs](file://src/nda_converter.rs)
- [src/nda_document.rs](file://src/nda_document.rs)
- [src/nda_executor.rs](file://src/nda_executor.rs)
- [src/sandbox.rs](file://src/sandbox.rs)
