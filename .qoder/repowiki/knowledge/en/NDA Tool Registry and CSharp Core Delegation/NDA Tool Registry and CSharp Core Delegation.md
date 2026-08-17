# NDA Tool Registry and CSharp Core Delegation

## Classification
- **Category**: Tool Dispatch / External Integration
- **Files**: src/registry.rs (136 LOC)
- **Criticality**: High — bridges Rust protocol layer to C# NDA engine

## Summary

The tool registry defines 3 NDA tools (`convert_to_nda`, `read_nda`, `execute_nda`) and delegates their execution to an external C# NdaMcpServer process. The Rust server acts as a high-performance protocol front-end while the C# engine handles the complex NDA format operations.

## Registered Tools

| Tool | Parameters | Description |
|------|-----------|-------------|
| `convert_to_nda` | `filePath` (required), `outputPath` (optional) | Convert any file to cryptographically signed .nda |
| `read_nda` | `ndaPath` (required) | Read/parse .nda binary file |
| `execute_nda` | `ndaPath` (required), `arguments` (optional) | Execute runnable .nda container |

## Delegation Flow

1. Construct JSON-RPC request envelope with `id: 999`
2. Spawn C# process with piped stdin/stdout
3. Write request to stdin
4. Read response from stdout via `wait_with_output()`
5. Parse JSON-RPC response → extract `result.content[0].text`
6. Check `result.isError` flag for error propagation

## C# Executable Path

```
C:\Users\visse\OneDrive\Documents\Payment and Transaction Flow\Velocity\NdaMcpServer\bin\Debug\net10.0\NdaMcpServer.exe
```

## Critical Constraints

- Path is hardcoded — must be updated for different environments
- C# process is spawned synchronously per tool call (no pooling)
- Response JSON structure is assumed: `result.content[0].text`
- Requires .NET 10.0 runtime on the host
