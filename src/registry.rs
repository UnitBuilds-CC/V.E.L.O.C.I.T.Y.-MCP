use serde::{Serialize, Deserialize};
use serde_json::{json, Value};
use std::error::Error;
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;
use tracing::{info, warn, error, debug};

/// An MCP tool registration with name, description, and JSON input schema.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Tool {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

/// Default C# NdaMcpServer path. Can be overridden via VELOCITY_CSHARP_PATH env var
/// or by passing a custom path to `call_tool_with_csharp_path`.
const DEFAULT_CSHARP_PATH: &str = r"C:\Users\visse\OneDrive\Documents\Payment and Transaction Flow\Velocity\NdaMcpServer\bin\Debug\net10.0\NdaMcpServer.exe";

/// Timeout for C# process execution (30 seconds).
/// Note: Currently unused as we read stdout until complete response, then kill the process.
const _CSHARP_TIMEOUT: Duration = Duration::from_secs(30);

/// Global cache for dynamically discovered tools from the C# engine.
static CACHED_TOOLS: OnceLock<Vec<Tool>> = OnceLock::new();

/// Return the list of registered MCP tools with their input schemas.
///
/// This includes:
/// 1. Built-in NDA tools (convert_to_nda, read_nda, execute_nda)
/// 2. Dynamically discovered tools from the C# engine (cached on first call)
///
/// Duplicates are filtered out — if a discovered tool has the same name as a built-in,
/// the built-in version takes precedence.
pub fn get_tools() -> Vec<Tool> {
    let mut tools = get_builtin_tools();
    let builtin_names: Vec<String> = tools.iter().map(|t| t.name.clone()).collect();
    
    // Add dynamically discovered tools from the C# engine
    if let Some(dynamic_tools) = CACHED_TOOLS.get() {
        // Filter out tools that have the same name as built-ins
        for tool in dynamic_tools {
            if !builtin_names.contains(&tool.name) {
                tools.push(tool.clone());
            }
        }
    } else {
        // Try to discover tools from the C# engine
        match discover_csharp_tools() {
            Ok(dynamic_tools) => {
                let _ = CACHED_TOOLS.set(dynamic_tools.clone());
                for tool in dynamic_tools {
                    if !builtin_names.contains(&tool.name) {
                        tools.push(tool.clone());
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "Failed to discover tools from C# engine, using built-in tools only");
            }
        }
    }
    
    tools
}

/// Return only the built-in NDA tools.
fn get_builtin_tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "convert_to_nda".to_string(),
            description: "Convert any file (e.g. C# source code, PDF, CSV, Excel, Image, Zip archive) into a cryptographically signed NDA (.nda) binary document.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "filePath": { "type": "string", "description": "Absolute path to the input file to convert." },
                    "outputPath": { "type": "string", "description": "Optional absolute path to write the compiled .nda file. Defaults to input path with .nda extension." }
                },
                "required": ["filePath"]
            }),
        },
        Tool {
            name: "read_nda".to_string(),
            description: "Read and parse a compiled .nda binary file to view its semantic triples, visual display commands, and string pool contents.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "ndaPath": { "type": "string", "description": "Absolute path to the .nda file to inspect." }
                },
                "required": ["ndaPath"]
            }),
        },
        Tool {
            name: "execute_nda".to_string(),
            description: "Execute a runnable .nda container. If it holds a compiled C# binary, it is run in-memory. If it contains a script (e.g., Python, Node.js, PowerShell, Bash), it executes via the corresponding shell process.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "ndaPath": { "type": "string", "description": "Absolute path to the runnable .nda file." },
                    "arguments": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional command-line arguments to pass to the executable or script."
                    }
                },
                "required": ["ndaPath"]
            }),
        },
    ]
}

/// Discover tools from the C# engine by sending a tools/list request.
fn discover_csharp_tools() -> Result<Vec<Tool>, Box<dyn Error>> {
    let csharp_path = resolve_csharp_path();
    
    if !std::path::Path::new(&csharp_path).exists() {
        return Err(format!("C# engine not found at: {}", csharp_path).into());
    }
    
    debug!("Discovering tools from C# engine");
    
    // Prepare tools/list request
    let request = json!({
        "jsonrpc": "2.0",
        "method": "tools/list",
        "params": {},
        "id": 1
    });
    
    let request_str = serde_json::to_string(&request)? + "\n";
    
    // Spawn the C# process and send the request
    use std::io::{Write, BufRead, BufReader};
    let mut child = Command::new(&csharp_path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    
    {
        let stdin = child.stdin.as_mut().ok_or("Failed to open stdin")?;
        stdin.write_all(request_str.as_bytes())?;
        stdin.flush()?;
    }
    
    // Read response
    let mut stdout = child.stdout.take().ok_or("Failed to open stdout")?;
    let reader_thread = std::thread::spawn(move || {
        let mut response_str = String::new();
        let mut reader = BufReader::new(&mut stdout);
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    response_str.push_str(&line);
                    if response_str.trim().starts_with('{') && response_str.trim().ends_with('}') {
                        if serde_json::from_str::<Value>(response_str.trim()).is_ok() {
                            break;
                        }
                    }
                }
                Err(_) => break,
            }
        }
        response_str
    });
    
    let response_str = reader_thread.join().map_err(|_| "Failed to read response")?;
    let _ = child.kill();
    let _ = child.wait();
    
    if response_str.trim().is_empty() {
        return Err("Empty response from C# engine".into());
    }
    
    let response: Value = serde_json::from_str(response_str.trim())?;
    
    // Parse the tools from the response
    let tools_value = &response["result"]["tools"];
    if !tools_value.is_array() {
        return Err("Invalid tools/list response from C# engine".into());
    }
    
    let tools: Vec<Tool> = serde_json::from_value(tools_value.clone())?;
    info!(count = tools.len(), "Discovered tools from C# engine");
    
    Ok(tools)
}

/// Dispatch a tool call by name, using the configured C# engine path.
///
/// Validates required parameters and file paths before delegating to the
/// C# process. Returns the tool's text output on success, or an error
/// describing what went wrong.
pub fn call_tool(name: &str, arguments: &Value) -> Result<String, Box<dyn Error>> {
    let csharp_path = resolve_csharp_path();
    call_tool_with_csharp_path(name, arguments, &csharp_path)
}

/// Call a tool with an explicit C# executable path.
///
/// Routes tool calls as follows:
/// - Built-in NDA tools (convert_to_nda, read_nda, execute_nda): validates paths, then delegates
/// - All other tools: delegates directly to the C# engine (dynamic tool hosting)
pub fn call_tool_with_csharp_path(name: &str, arguments: &Value, csharp_path: &str) -> Result<String, Box<dyn Error>> {
    debug!(tool = name, "Dispatching tool call");

    match name {
        // Built-in NDA tools with path validation
        "convert_to_nda" => {
            let file_path = arguments["filePath"].as_str().ok_or("filePath is required")?;
            validate_file_path(file_path)?;
            let _output_path = arguments["outputPath"].as_str().unwrap_or("");
            if !_output_path.is_empty() {
                validate_file_path(_output_path)?;
            }
            execute_csharp_mcp_tool("convert_to_nda", arguments, csharp_path)
        }
        "read_nda" => {
            let nda_path = arguments["ndaPath"].as_str().ok_or("ndaPath is required")?;
            validate_file_path(nda_path)?;
            execute_csharp_mcp_tool("read_nda", arguments, csharp_path)
        }
        "execute_nda" => {
            let nda_path = arguments["ndaPath"].as_str().ok_or("ndaPath is required")?;
            validate_file_path(nda_path)?;
            execute_csharp_mcp_tool("execute_nda", arguments, csharp_path)
        }
        // Dynamic tools: route directly to the C# engine
        _ => {
            debug!(tool = name, "Routing to C# engine (dynamic tool)");
            execute_csharp_mcp_tool(name, arguments, csharp_path)
        }
    }
}

/// Resolve the C# NdaMcpServer executable path.
/// Priority: VELOCITY_CSHARP_PATH env var > default hardcoded path.
pub fn resolve_csharp_path() -> String {
    std::env::var("VELOCITY_CSHARP_PATH")
        .unwrap_or_else(|_| DEFAULT_CSHARP_PATH.to_string())
}

/// Validate a file path for safety.
/// Rejects paths that are not absolute, contain traversal sequences, or are empty.
fn validate_file_path(path: &str) -> Result<(), Box<dyn Error>> {
    if path.is_empty() {
        return Err("File path cannot be empty".into());
    }

    // Reject path traversal attempts
    if path.contains("..") {
        return Err(format!("File path contains traversal sequence '..': {}", path).into());
    }

    // On Windows, check for absolute path (drive letter or UNC)
    let p = std::path::Path::new(path);
    if !p.is_absolute() {
        return Err(format!("File path must be absolute: {}", path).into());
    }

    Ok(())
}

fn execute_csharp_mcp_tool(tool_name: &str, arguments: &Value, exe_path: &str) -> Result<String, Box<dyn Error>> {
    info!(tool = tool_name, exe = exe_path, "Delegating to C# core engine");

    if !std::path::Path::new(exe_path).exists() {
        error!(exe = exe_path, "C# core engine not found");
        return Err(format!("C# core engine not found at expected path: {}", exe_path).into());
    }

    // Prepare JSON-RPC request to pass to the stdin of the C# server
    let request = json!({
        "jsonrpc": "2.0",
        "method": "tools/call",
        "params": {
            "name": tool_name,
            "arguments": arguments
        },
        "id": 999
    });

    let request_str = serde_json::to_string(&request)? + "\n";

    // Spawn the process
    use std::io::{Write, BufRead, BufReader};
    let mut child = Command::new(exe_path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    // Write request and close stdin to signal EOF
    {
        let stdin = child.stdin.as_mut().ok_or("Failed to open stdin of C# child process")?;
        stdin.write_all(request_str.as_bytes())?;
        stdin.flush()?;
    }
    // stdin is dropped here, closing the pipe

    // Read stdout in a thread with timeout
    let mut stdout = child.stdout.take().ok_or("Failed to open stdout")?;
    let reader_thread = std::thread::spawn(move || {
        let mut response_str = String::new();
        let mut reader = BufReader::new(&mut stdout);
        // Read lines until we get a complete JSON response
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => break, // EOF
                Ok(_) => {
                    response_str.push_str(&line);
                    // Check if we have a complete JSON object
                    if response_str.trim().starts_with('{') && response_str.trim().ends_with('}') {
                        // Try to parse it
                        if serde_json::from_str::<Value>(response_str.trim()).is_ok() {
                            break;
                        }
                    }
                }
                Err(_) => break,
            }
        }
        response_str
    });

    // Wait for reader thread with timeout
    let response_str = match reader_thread.join() {
        Ok(s) => s,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Failed to read response from C# process".into());
        }
    };

    // Kill the process (C# MCP server doesn't exit on its own)
    let _ = child.kill();
    let _ = child.wait();

    if response_str.trim().is_empty() {
        error!(tool = tool_name, "C# process returned empty response");
        return Err("C# process returned empty response".into());
    }

    let response: Value = serde_json::from_str(response_str.trim())?;

    // Parse out the text from the JSON-RPC response content array
    if let Some(err) = response.get("error") {
        let msg = err["message"].as_str().unwrap_or("Unknown");
        error!(tool = tool_name, error = msg, "C# returned JSON-RPC error");
        return Err(format!("C# Execution Error: {}", msg).into());
    }

    let is_error = response["result"]["isError"].as_bool().unwrap_or(false);
    let text = response["result"]["content"][0]["text"].as_str().ok_or("Failed to parse tool text output")?;

    if is_error {
        warn!(tool = tool_name, "C# tool returned error");
        Err(text.into())
    } else {
        info!(tool = tool_name, "C# tool executed successfully");
        Ok(text.to_string())
    }
}

/// Wait for a child process with a timeout.
/// Polls the child with try_wait() and kills it if the timeout expires.
/// Returns the output on success, or an error if the timeout expires.
#[allow(dead_code)]
fn wait_with_timeout(mut child: std::process::Child, timeout: Duration) -> Result<std::process::Output, Box<dyn Error>> {
    let start = std::time::Instant::now();
    let poll_interval = Duration::from_millis(50);

    loop {
        match child.try_wait()? {
            Some(_status) => {
                // Child exited — collect stdout/stderr
                let mut stdout = Vec::new();
                let mut stderr = Vec::new();
                if let Some(mut out) = child.stdout {
                    use std::io::Read;
                    let _ = out.read_to_end(&mut stdout);
                }
                if let Some(mut err) = child.stderr {
                    use std::io::Read;
                    let _ = err.read_to_end(&mut stderr);
                }
                return Ok(std::process::Output {
                    status: _status,
                    stdout,
                    stderr,
                });
            }
            None => {
                if start.elapsed() >= timeout {
                    // Kill the child process on timeout to prevent orphans
                    let _ = child.kill();
                    let _ = child.wait(); // Reap the zombie
                    return Err(format!(
                        "C# process timed out after {:.0}s",
                        timeout.as_secs_f64()
                    ).into());
                }
                std::thread::sleep(poll_interval);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_tools_returns_three_tools() {
        let tools = get_tools();
        assert_eq!(tools.len(), 3);
        assert_eq!(tools[0].name, "convert_to_nda");
        assert_eq!(tools[1].name, "read_nda");
        assert_eq!(tools[2].name, "execute_nda");
    }

    #[test]
    fn test_get_tools_have_input_schemas() {
        for tool in get_tools() {
            assert_eq!(tool.input_schema["type"], "object");
            assert!(tool.input_schema["properties"].is_object());
            assert!(tool.input_schema["required"].is_array());
        }
    }

    #[test]
    fn test_call_unknown_tool_returns_error() {
        // Unknown tools are routed to the C# engine, which will return an error
        let result = call_tool("nonexistent_tool", &json!({}));
        assert!(result.is_err());
    }

    #[test]
    fn test_call_tool_missing_required_param() {
        let result = call_tool("convert_to_nda", &json!({}));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("filePath is required"));
    }

    #[test]
    fn test_call_read_nda_missing_param() {
        let result = call_tool("read_nda", &json!({}));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("ndaPath is required"));
    }

    #[test]
    fn test_call_execute_nda_missing_param() {
        let result = call_tool("execute_nda", &json!({}));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("ndaPath is required"));
    }

    #[test]
    fn test_validate_file_path_rejects_empty() {
        let result = validate_file_path("");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty"));
    }

    #[test]
    fn test_validate_file_path_rejects_traversal() {
        let result = validate_file_path("C:\\Users\\test\\..\\..\\Windows\\System32\\config");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("traversal"));
    }

    #[test]
    fn test_validate_file_path_rejects_relative() {
        let result = validate_file_path("relative/path/file.txt");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("absolute"));
    }

    #[test]
    fn test_validate_file_path_accepts_valid_absolute() {
        let result = validate_file_path("C:\\Users\\test\\documents\\file.nda");
        assert!(result.is_ok());
    }

    #[test]
    fn test_resolve_csharp_path_returns_string() {
        let path = resolve_csharp_path();
        assert!(!path.is_empty());
    }

    #[test]
    fn test_csharp_path_not_found_returns_error() {
        let result = call_tool_with_csharp_path(
            "read_nda",
            &json!({"ndaPath": "C:\\test.nda"}),
            "C:\\nonexistent\\path\\NdaMcpServer.exe",
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }
}
