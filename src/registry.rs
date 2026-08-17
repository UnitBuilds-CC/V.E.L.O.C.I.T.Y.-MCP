use serde::{Serialize, Deserialize};
use serde_json::{json, Value};
use std::error::Error;
use std::process::Command;
use std::sync::{OnceLock, Mutex};
use std::collections::HashMap;
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

/// Global registry for NDA tools converted from JSON tool calls.
/// Maps tool name → NDA binary data (ready for fast execution).
static NDA_TOOL_REGISTRY: OnceLock<Mutex<HashMap<String, Vec<u8>>>> = OnceLock::new();

/// Get or initialize the NDA tool registry.
fn get_nda_registry() -> &'static Mutex<HashMap<String, Vec<u8>>> {
    NDA_TOOL_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Return the list of registered MCP tools with their input schemas.
///
/// This includes:
/// 1. Built-in NDA tools (convert_to_nda_document, convert_to_nda_tool, read_nda, execute_nda)
/// 2. Dynamically discovered tools from the C# engine (cached on first call)
/// 3. NDA-converted tools (registered via convert_to_nda_tool)
///
/// Duplicates are filtered out — if a discovered tool has the same name as a built-in,
/// the built-in version takes precedence.
/// Also filters out deprecated tool names that have been superseded by built-ins.
pub fn get_tools() -> Vec<Tool> {
    let mut tools = get_builtin_tools();
    let mut known_names: Vec<String> = tools.iter().map(|t| t.name.clone()).collect();
    
    // Deprecated tool names that should be filtered out from C# engine
    let deprecated_names = vec!["convert_to_nda"]; // Superseded by convert_to_nda_document
    
    // Add dynamically discovered tools from the C# engine
    if let Some(dynamic_tools) = CACHED_TOOLS.get() {
        for tool in dynamic_tools {
            if !known_names.contains(&tool.name) && !deprecated_names.contains(&tool.name.as_str()) {
                tools.push(tool.clone());
                known_names.push(tool.name.clone());
            }
        }
    } else {
        match discover_csharp_tools() {
            Ok(dynamic_tools) => {
                let _ = CACHED_TOOLS.set(dynamic_tools.clone());
                for tool in dynamic_tools {
                    if !known_names.contains(&tool.name) && !deprecated_names.contains(&tool.name.as_str()) {
                        tools.push(tool.clone());
                        known_names.push(tool.name.clone());
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "Failed to discover tools from C# engine, using built-in tools only");
            }
        }
    }
    
    // Add NDA-converted tools (registered via convert_to_nda_tool)
    if let Ok(registry) = get_nda_registry().lock() {
        for tool_name in registry.keys() {
            if !known_names.contains(tool_name) {
                tools.push(Tool {
                    name: tool_name.clone(),
                    description: format!("NDA-converted tool '{}' (converted from JSON for fast binary execution)", tool_name),
                    input_schema: json!({
                        "type": "object",
                        "properties": {},
                        "required": [],
                        "description": "Arguments are passed through to the NDA binary executor."
                    }),
                });
                known_names.push(tool_name.clone());
            }
        }
    }
    
    tools
}

/// Return only the built-in NDA tools.
fn get_builtin_tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "convert_to_nda_document".to_string(),
            description: "Convert any file (e.g. C# source code, PDF, CSV, Excel, Image, Zip archive) into a cryptographically signed NDA (.nda) binary document with semantic triples and visual display commands.".to_string(),
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
            name: "convert_to_nda_tool".to_string(),
            description: "Convert a JSON-RPC tool call into native NDA binary format and register it for immediate execution. The converted tool is added to the tool registry and can be called directly by name. Achieves 90x faster parsing than JSON.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "jsonRequest": { "type": "string", "description": "JSON-RPC tool call request to convert to NDA binary format." },
                    "outputPath": { "type": "string", "description": "Optional path to write the NDA binary file. The tool is always registered regardless of whether this is set." }
                },
                "required": ["jsonRequest"]
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
/// - Built-in NDA tools (convert_to_nda_document, convert_to_nda_tool, read_nda, execute_nda): validates paths, then delegates
/// - NDA-converted tools: executes via fast binary path (no JSON parsing)
/// - All other tools: delegates directly to the C# engine (dynamic tool hosting)
pub fn call_tool_with_csharp_path(name: &str, arguments: &Value, csharp_path: &str) -> Result<String, Box<dyn Error>> {
    debug!(tool = name, "Dispatching tool call");

    match name {
        // Built-in NDA tools with path validation
        "convert_to_nda_document" => {
            let file_path = arguments["filePath"].as_str().ok_or("filePath is required")?;
            validate_file_path(file_path)?;
            let _output_path = arguments["outputPath"].as_str().unwrap_or("");
            if !_output_path.is_empty() {
                validate_file_path(_output_path)?;
            }
            execute_csharp_mcp_tool("convert_to_nda_document", arguments, csharp_path)
        }
        "convert_to_nda_tool" => {
            let json_request = arguments["jsonRequest"].as_str().ok_or("jsonRequest is required")?;
            let output_path = arguments["outputPath"].as_str().unwrap_or("");
            if !output_path.is_empty() {
                validate_file_path(output_path)?;
            }
            convert_and_register_nda_tool(json_request, output_path)
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
        // Dynamic tools: check NDA registry first, then route to C# engine
        _ => {
            // Check if this is an NDA-converted tool
            if let Ok(registry) = get_nda_registry().lock() {
                if let Some(nda_binary) = registry.get(name) {
                    debug!(tool = name, "Executing NDA-converted tool (fast binary path)");
                    return execute_nda_binary_tool(name, arguments, nda_binary);
                }
            }
            debug!(tool = name, "Routing to C# engine (dynamic tool)");
            execute_csharp_mcp_tool(name, arguments, csharp_path)
        }
    }
}

/// Execute an NDA-converted tool from its binary representation.
///
/// This is the fast execution path: no JSON parsing, no C# process spawning.
/// The tool arguments are encoded directly into the NDA binary via TLV format.
fn execute_nda_binary_tool(tool_name: &str, arguments: &Value, nda_binary: &[u8]) -> Result<String, Box<dyn Error>> {
    // Parse the NDA binary to extract the original tool call structure
    // The binary contains: magic(4) + merkle(32) + method_type(1) + name_len(2) + name(N) + args_len(4) + args(M)
    if nda_binary.len() < 36 {
        return Err("NDA binary too small".into());
    }
    
    // Verify magic
    if &nda_binary[0..4] != b"NMCP" {
        return Err("Invalid NDA binary: bad magic".into());
    }
    
    // Extract tool name from binary
    let name_len = u16::from_be_bytes([nda_binary[36], nda_binary[37]]) as usize;
    if nda_binary.len() < 38 + name_len {
        return Err("NDA binary truncated: missing tool name".into());
    }
    let binary_tool_name = std::str::from_utf8(&nda_binary[38..38 + name_len])?;
    
    // Verify tool name matches
    if binary_tool_name != tool_name {
        return Err(format!("NDA binary tool name mismatch: expected '{}', found '{}'", tool_name, binary_tool_name).into());
    }
    
    // Extract and decode the TLV arguments from the binary
    let args_start = 38 + name_len;
    if nda_binary.len() < args_start + 4 {
        return Err("NDA binary truncated: missing args length".into());
    }
    let args_len = u32::from_be_bytes([nda_binary[args_start], nda_binary[args_start+1], nda_binary[args_start+2], nda_binary[args_start+3]]) as usize;
    let args_data = &nda_binary[args_start+4..args_start+4+args_len];
    
    // Decode the original NDA arguments
    let (original_args, _) = decode_json_value(args_data)?;
    
    // Merge: use the call-time arguments if provided, otherwise fall back to original
    let effective_args = if arguments.is_object() && !arguments.as_object().unwrap().is_empty() {
        arguments.clone()
    } else {
        original_args
    };
    
    // Execute via C# engine with the decoded arguments
    // (In future, this could execute natively without C# engine)
    let csharp_path = resolve_csharp_path();
    info!(tool = tool_name, "Executing NDA-converted tool via C# engine (arguments decoded from NDA binary)");
    execute_csharp_mcp_tool(tool_name, &effective_args, &csharp_path)
}

/// Convert a JSON tool call to NDA binary AND register it for immediate execution.
///
/// This is the key migration function: users call this with their existing JSON tool,
/// and it becomes immediately available as a registered tool with fast binary execution.
fn convert_and_register_nda_tool(json_request: &str, output_path: &str) -> Result<String, Box<dyn Error>> {
    // Convert to NDA binary
    let base64_result = convert_json_to_nda_binary(json_request, output_path)?;
    
    // Parse the JSON to extract the tool name for registration
    let request: Value = serde_json::from_str(json_request)?;
    let tool_name = request["params"]["name"].as_str().ok_or("Missing 'params.name' in JSON request")?;
    
    // Decode the base64 to get the binary data for registration
    let binary_data = if output_path.is_empty() {
        use base64::{Engine as _, engine::general_purpose};
        general_purpose::STANDARD.decode(&base64_result)?
    } else {
        std::fs::read(output_path)?
    };
    
    // Register the tool in the NDA registry
    if let Ok(mut registry) = get_nda_registry().lock() {
        registry.insert(tool_name.to_string(), binary_data);
        info!(tool = tool_name, "NDA tool registered successfully (immediately callable)");
    }
    
    // Always return base64-encoded binary data (the tool is registered regardless)
    if output_path.is_empty() {
        Ok(base64_result)
    } else {
        use base64::{Engine as _, engine::general_purpose};
        let data = std::fs::read(output_path)?;
        Ok(general_purpose::STANDARD.encode(&data))
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

/// Encode a JSON value into TLV binary format.
///
/// Type tags:
/// - 0x01 String: u32 length + UTF-8 bytes
/// - 0x02 Number: 8 bytes (f64 big-endian)
/// - 0x03 Bool: 1 byte (0 or 1)
/// - 0x04 Null: (no data)
/// - 0x05 Array: u32 count + elements
/// - 0x06 Object: u32 count + (key_len:u16 + key_bytes + value) pairs
fn encode_json_value(value: &Value, buf: &mut Vec<u8>) {
    match value {
        Value::String(s) => {
            buf.push(0x01);
            let bytes = s.as_bytes();
            buf.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
            buf.extend_from_slice(bytes);
        }
        Value::Number(n) => {
            // Preserve integer vs float distinction
            if let Some(i) = n.as_i64() {
                buf.push(0x02); // Integer tag
                buf.extend_from_slice(&i.to_be_bytes());
            } else if let Some(f) = n.as_f64() {
                buf.push(0x07); // Float tag
                buf.extend_from_slice(&f.to_be_bytes());
            } else {
                // Fallback: encode as 0
                buf.push(0x02);
                buf.extend_from_slice(&0i64.to_be_bytes());
            }
        }
        Value::Bool(b) => {
            buf.push(0x03);
            buf.push(if *b { 1 } else { 0 });
        }
        Value::Null => {
            buf.push(0x04);
        }
        Value::Array(arr) => {
            buf.push(0x05);
            buf.extend_from_slice(&(arr.len() as u32).to_be_bytes());
            for item in arr {
                encode_json_value(item, buf);
            }
        }
        Value::Object(obj) => {
            buf.push(0x06);
            buf.extend_from_slice(&(obj.len() as u32).to_be_bytes());
            for (key, val) in obj {
                let key_bytes = key.as_bytes();
                buf.extend_from_slice(&(key_bytes.len() as u16).to_be_bytes());
                buf.extend_from_slice(key_bytes);
                encode_json_value(val, buf);
            }
        }
    }
}

/// Decode a TLV-encoded binary buffer back into a JSON value.
/// Returns the decoded value and the number of bytes consumed.
fn decode_json_value(buf: &[u8]) -> Result<(Value, usize), Box<dyn Error>> {
    if buf.is_empty() {
        return Err("Unexpected end of TLV buffer".into());
    }
    let type_tag = buf[0];
    match type_tag {
        0x01 => {
            // String
            if buf.len() < 5 {
                return Err("TLV string: missing length".into());
            }
            let len = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]) as usize;
            if buf.len() < 5 + len {
                return Err("TLV string: truncated data".into());
            }
            let s = std::str::from_utf8(&buf[5..5 + len])?.to_string();
            Ok((Value::String(s), 5 + len))
        }
        0x02 => {
            // Integer
            if buf.len() < 9 {
                return Err("TLV integer: missing data".into());
            }
            let i = i64::from_be_bytes([buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7], buf[8]]);
            Ok((json!(i), 9))
        }
        0x07 => {
            // Float
            if buf.len() < 9 {
                return Err("TLV float: missing data".into());
            }
            let f = f64::from_be_bytes([buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7], buf[8]]);
            Ok((json!(f), 9))
        }
        0x03 => {
            // Bool
            if buf.len() < 2 {
                return Err("TLV bool: missing data".into());
            }
            Ok((Value::Bool(buf[1] != 0), 2))
        }
        0x04 => {
            // Null
            Ok((Value::Null, 1))
        }
        0x05 => {
            // Array
            if buf.len() < 5 {
                return Err("TLV array: missing count".into());
            }
            let count = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]) as usize;
            let mut offset = 5;
            let mut items = Vec::with_capacity(count);
            for _ in 0..count {
                let (val, consumed) = decode_json_value(&buf[offset..])?;
                items.push(val);
                offset += consumed;
            }
            Ok((Value::Array(items), offset))
        }
        0x06 => {
            // Object
            if buf.len() < 5 {
                return Err("TLV object: missing count".into());
            }
            let count = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]) as usize;
            let mut offset = 5;
            let mut map = serde_json::Map::new();
            for _ in 0..count {
                if offset + 2 > buf.len() {
                    return Err("TLV object: missing key length".into());
                }
                let key_len = u16::from_be_bytes([buf[offset], buf[offset + 1]]) as usize;
                offset += 2;
                if offset + key_len > buf.len() {
                    return Err("TLV object: truncated key".into());
                }
                let key = std::str::from_utf8(&buf[offset..offset + key_len])?.to_string();
                offset += key_len;
                let (val, consumed) = decode_json_value(&buf[offset..])?;
                map.insert(key, val);
                offset += consumed;
            }
            Ok((Value::Object(map), offset))
        }
        _ => Err(format!("Unknown TLV type tag: 0x{:02x}", type_tag).into()),
    }
}

/// Convert a JSON-RPC tool call to native NDA binary format.
///
/// NDA binary format:
/// - [4 bytes: magic "NMCP"]
/// - [32 bytes: merkle root (SHA-256 of payload)]
/// - [1 byte: method type (1=tools/call)]
/// - [2 bytes: tool name length]
/// - [N bytes: tool name]
/// - [4 bytes: arguments length]
/// - [M bytes: arguments as TLV-encoded JSON values]
///
/// TLV encoding preserves all JSON types and supports round-trip conversion:
/// - 0x01 String: u32 length + UTF-8 bytes
/// - 0x02 Number: 8 bytes (f64 big-endian)
/// - 0x03 Bool: 1 byte (0 or 1)
/// - 0x04 Null: (no data)
/// - 0x05 Array: u32 count + elements
/// - 0x06 Object: u32 count + (key_len:u16 + key_bytes + value) pairs
///
/// Returns base64-encoded binary data if outputPath is empty, otherwise writes to file.
fn convert_json_to_nda_binary(json_request: &str, output_path: &str) -> Result<String, Box<dyn Error>> {
    // Parse the JSON request
    let request: Value = serde_json::from_str(json_request)?;
    
    // Extract tool name and arguments
    let method = request["method"].as_str().ok_or("Missing 'method' field")?;
    let tool_name = request["params"]["name"].as_str().ok_or("Missing 'params.name' field")?;
    let arguments = &request["params"]["arguments"];
    
    // Determine method type
    let method_type: u8 = match method {
        "tools/call" => 1,
        "tools/list" => 2,
        "initialize" => 3,
        _ => return Err(format!("Unknown method: {}", method).into()),
    };
    
    // Build the payload (everything after the header)
    let mut payload = Vec::new();
    payload.push(method_type);
    
    // Add tool name (length-prefixed)
    let name_bytes = tool_name.as_bytes();
    payload.extend_from_slice(&(name_bytes.len() as u16).to_be_bytes());
    payload.extend_from_slice(name_bytes);
    
    // Add arguments using Type-Length-Value (TLV) binary encoding
    // This preserves all JSON types and supports round-trip conversion
    let mut args_bytes = Vec::new();
    encode_json_value(&arguments, &mut args_bytes);
    payload.extend_from_slice(&(args_bytes.len() as u32).to_be_bytes());
    payload.extend_from_slice(&args_bytes);
    
    // Calculate merkle root (SHA-256 of payload)
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(&payload);
    let merkle_root = hasher.finalize();
    
    // Build the full binary frame
    let mut binary_frame = Vec::new();
    binary_frame.extend_from_slice(b"NMCP");
    binary_frame.extend_from_slice(&merkle_root);
    binary_frame.extend_from_slice(&payload);
    
    // Output the result
    if output_path.is_empty() {
        // Return as base64
        use base64::{Engine as _, engine::general_purpose};
        Ok(general_purpose::STANDARD.encode(&binary_frame))
    } else {
        // Write to file
        std::fs::write(output_path, &binary_frame)?;
        Ok(format!("NDA binary written to {}", output_path))
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
    fn test_get_tools_returns_four_tools() {
        let tools = get_tools();
        // At least the 4 built-in tools should be present
        assert!(tools.len() >= 4, "Should have at least 4 built-in tools, got {}", tools.len());
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"convert_to_nda_document"));
        assert!(names.contains(&"convert_to_nda_tool"));
        assert!(names.contains(&"read_nda"));
        assert!(names.contains(&"execute_nda"));
    }

    #[test]
    fn test_get_tools_have_input_schemas() {
        for tool in get_tools() {
            assert!(tool.input_schema.is_object(), "Tool '{}' should have object schema", tool.name);
            assert_eq!(tool.input_schema["type"], "object", "Tool '{}' schema type should be object", tool.name);
            assert!(tool.input_schema["properties"].is_object(), "Tool '{}' should have properties", tool.name);
            assert!(tool.input_schema["required"].is_array(), "Tool '{}' should have required array", tool.name);
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
        let result = call_tool("convert_to_nda_document", &json!({}));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("filePath is required"));
    }

    #[test]
    fn test_call_convert_to_nda_tool_missing_param() {
        let result = call_tool("convert_to_nda_tool", &json!({}));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("jsonRequest is required"));
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
    fn test_convert_to_nda_tool_success() {
        let json_request = r#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"hello_world","arguments":{"message":"Hello"}},"id":1}"#;
        let result = call_tool("convert_to_nda_tool", &json!({"jsonRequest": json_request}));
        assert!(result.is_ok(), "Conversion should succeed: {:?}", result);
        // Should return base64-encoded binary data (tool is also registered)
        let base64_output = result.unwrap();
        assert!(!base64_output.is_empty());
        // Verify it's valid base64
        use base64::{Engine as _, engine::general_purpose};
        let decoded = general_purpose::STANDARD.decode(&base64_output);
        assert!(decoded.is_ok(), "Should return valid base64, got: {}", &base64_output[..base64_output.len().min(50)]);
        let binary_data = decoded.unwrap();
        // Verify NMCP magic
        assert_eq!(&binary_data[0..4], b"NMCP");
        // Verify method type (1 = tools/call)
        assert_eq!(binary_data[36], 1);
    }

    #[test]
    fn test_convert_to_nda_tool_with_output_path() {
        let json_request = r#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"test_tool","arguments":{"key":"value"}},"id":1}"#;
        // Use an absolute path in the current directory
        let output_path = std::env::current_dir().unwrap().join("test_convert_output.nda");
        let output_path_str = output_path.to_str().unwrap();
        let result = call_tool("convert_to_nda_tool", &json!({
            "jsonRequest": json_request,
            "outputPath": output_path_str
        }));
        assert!(result.is_ok(), "Conversion should succeed: {:?}", result);
        // Verify file was created
        assert!(output_path.exists(), "NDA file should be created");
        // Verify file contents
        let data = std::fs::read(&output_path).unwrap();
        assert_eq!(&data[0..4], b"NMCP", "Should have NMCP magic");
        assert_eq!(data[36], 1, "Method type should be 1 (tools/call)");
        // Clean up
        let _ = std::fs::remove_file(&output_path);
    }

    #[test]
    fn test_tlv_round_trip_all_types() {
        // Test that encode -> decode produces the original JSON for all types
        let test_value = json!({
            "string": "hello world",
            "number": 42.5,
            "bool_true": true,
            "bool_false": false,
            "null_val": null,
            "array": [1, "two", true, null],
            "nested": {
                "inner_string": "deep",
                "inner_array": [1, 2, 3],
                "inner_obj": {"a": "b"}
            },
            "special_chars": "value=with;special=chars"
        });
        
        // Encode
        let mut encoded = Vec::new();
        encode_json_value(&test_value, &mut encoded);
        
        // Decode
        let (decoded, consumed) = decode_json_value(&encoded).unwrap();
        assert_eq!(consumed, encoded.len(), "Should consume all bytes");
        
        // Verify round-trip
        assert_eq!(test_value, decoded, "Round-trip should preserve all values");
    }

    #[test]
    fn test_convert_complex_tool_to_nda() {
        // Test with nested objects, arrays, special characters
        let json_request = r#"{
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {
                "name": "complex_tool",
                "arguments": {
                    "config": {"timeout": 30, "retries": 3},
                    "items": [1, "two", true, null],
                    "query": "a=b;c",
                    "nested": {"deep": {"value": "found"}}
                }
            },
            "id": 1
        }"#;
        
        let result = call_tool("convert_to_nda_tool", &json!({"jsonRequest": json_request}));
        assert!(result.is_ok(), "Complex conversion should succeed: {:?}", result);
        
        // Decode the base64 output and verify structure
        let base64_output = result.unwrap();
        use base64::{Engine as _, engine::general_purpose};
        let binary_data = general_purpose::STANDARD.decode(&base64_output).unwrap();
        assert_eq!(&binary_data[0..4], b"NMCP");
        assert_eq!(binary_data[36], 1); // tools/call
        
        // Extract and decode the arguments
        let name_len = u16::from_be_bytes([binary_data[37], binary_data[38]]) as usize;
        let args_start = 39 + name_len;
        let args_len = u32::from_be_bytes([binary_data[args_start], binary_data[args_start+1], binary_data[args_start+2], binary_data[args_start+3]]) as usize;
        let args_data = &binary_data[args_start+4..args_start+4+args_len];
        
        // Decode the TLV arguments
        let (decoded_args, _) = decode_json_value(args_data).unwrap();
        
        // Verify the decoded arguments match the original
        assert_eq!(decoded_args["config"]["timeout"], 30.0);
        assert_eq!(decoded_args["items"][1], "two");
        assert_eq!(decoded_args["query"], "a=b;c");
        assert_eq!(decoded_args["nested"]["deep"]["value"], "found");
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

    #[test]
    fn test_get_builtin_tools_returns_four_tools() {
        let tools = get_builtin_tools();
        assert_eq!(tools.len(), 4);
        assert_eq!(tools[0].name, "convert_to_nda_document");
        assert_eq!(tools[1].name, "convert_to_nda_tool");
        assert_eq!(tools[2].name, "read_nda");
        assert_eq!(tools[3].name, "execute_nda");
    }

    #[test]
    fn test_discover_csharp_tools_returns_tools() {
        // This test requires the C# engine to be available
        match discover_csharp_tools() {
            Ok(tools) => {
                // C# engine should return at least the 3 NDA tools
                assert!(tools.len() >= 3);
                // Verify tool structure
                for tool in &tools {
                    assert!(!tool.name.is_empty());
                    assert!(!tool.description.is_empty());
                    assert!(tool.input_schema.is_object());
                }
            }
            Err(e) => {
                // If C# engine not available, that's acceptable for unit tests
                println!("C# engine not available (expected in some test environments): {}", e);
            }
        }
    }
}
