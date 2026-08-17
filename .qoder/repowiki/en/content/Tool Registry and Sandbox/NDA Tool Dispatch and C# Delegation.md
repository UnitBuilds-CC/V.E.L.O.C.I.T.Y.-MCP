# NDA Tool Dispatch and C# Delegation

<cite>
**Referenced Files in This Document**
- [src/registry.rs](file://src/registry.rs)
</cite>

## Overview

The tool registry (`src/registry.rs`) defines the MCP tools available on this server and handles their execution. The server currently provides three NDA (Non-Deterministic Automata) tools that are delegated to an external C# NdaMcpServer process for execution.

## Registered Tools

### `convert_to_nda`

Converts any file (C# source, PDF, CSV, Excel, Image, Zip) into a cryptographically signed `.nda` binary document.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `filePath` | string | Yes | Absolute path to input file |
| `outputPath` | string | No | Output path (defaults to input with `.nda` extension) |

### `read_nda`

Reads and parses a compiled `.nda` binary file to view its semantic triples, visual display commands, and string pool contents.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `ndaPath` | string | Yes | Absolute path to the `.nda` file |

### `execute_nda`

Executes a runnable `.nda` container. If it holds a compiled C# binary, it runs in-memory. If it contains a script (Python, Node.js, PowerShell, Bash), it executes via the corresponding shell process.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `ndaPath` | string | Yes | Absolute path to the runnable `.nda` file |
| `arguments` | string[] | No | Command-line arguments for the executable/script |

## Tool Definition Structure

Each tool is defined as a `Tool` struct with serde serialization:

```rust
pub struct Tool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,  // JSON Schema for parameters
}
```

The `input_schema` uses standard JSON Schema format with `type`, `properties`, and `required` fields. This is returned to MCP clients via `tools/list`.

## Execution Flow

### Dispatch

```rust
pub fn call_tool(name: &str, arguments: &Value) -> Result<String, Box<dyn Error>>
```

The `call_tool` function matches the tool name and delegates to `execute_csharp_mcp_tool()`.

### C# Delegation

The `execute_csharp_mcp_tool()` function:

1. **Locates the C# executable** at the hardcoded path:
   ```
   C:\Users\visse\OneDrive\Documents\Payment and Transaction Flow\Velocity\NdaMcpServer\bin\Debug\net10.0\NdaMcpServer.exe
   ```

2. **Constructs a JSON-RPC request** envelope:
   ```json
   {
     "jsonrpc": "2.0",
     "method": "tools/call",
     "params": { "name": "<tool_name>", "arguments": <args> },
     "id": 999
   }
   ```

3. **Spawns the C# process** with piped stdin/stdout:
   ```rust
   Command::new(exe_path)
       .stdin(Stdio::piped())
       .stdout(Stdio::piped())
       .spawn()?
   ```

4. **Writes the request** to the C# process's stdin

5. **Reads the response** from stdout via `wait_with_output()`

6. **Parses the JSON-RPC response** and extracts the text content:
   - Checks for `error` field → returns error
   - Checks `result.isError` → returns error if true
   - Extracts `result.content[0].text` → returns as success

## Error Handling

| Error | Cause |
|-------|-------|
| "filePath is required" | Missing required parameter |
| "C# core engine not found" | NdaMcpServer.exe doesn't exist at expected path |
| "Failed to open stdin" | Process spawn succeeded but stdin pipe failed |
| "C# process exited with status" | Non-zero exit code from C# process |
| "Failed to parse tool text output" | C# response doesn't match expected JSON structure |
| "C# Execution Error: ..." | C# process returned a JSON-RPC error |

## Key Design Decisions

1. **C# delegation**: The NDA format operations are complex and already implemented in C#. Rather than reimplementing in Rust, the server delegates to the proven C# engine via a subprocess.
2. **JSON-RPC over stdin**: The C# process speaks the same JSON-RPC protocol, making the delegation clean and testable.
3. **Hardcoded path**: The C# executable path is hardcoded for the current development setup. For production, this should be configurable via CLI flag or environment variable.
4. **Fixed request ID**: The delegation uses `id: 999` since it's an internal implementation detail — the original client request ID is not forwarded.
5. **Synchronous execution**: `wait_with_output()` blocks until the C# process completes. This is acceptable for tool calls but would need to be async for high-throughput scenarios.

**Section sources**
- [src/registry.rs](file://src/registry.rs)
