//! VELOCITY-MCP Core - WASM-compatible protocol logic.
//!
//! This crate contains pure MCP protocol handling without OS-specific dependencies.
//! It can be compiled to both native and wasm32-wasip1 targets.
//!
//! ## Tool Execution
//!
//! The core crate defines a [`ToolExecutor`] trait that callers implement to provide
//! actual tool execution. The [`handle_mcp_request_with_executor`] function routes
//! `tools/list` and `tools/call` to the provided executor, enabling real tool execution
//! in any deployment target (native, edge/WASM, etc.).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// MCP JSON-RPC request
#[derive(Debug, Deserialize)]
pub struct McpRequest {
    pub jsonrpc: String,
    #[serde(default)]
    pub method: String,
    #[serde(default)]
    pub params: Option<Value>,
    pub id: Option<Value>,
}

/// MCP JSON-RPC response
#[derive(Debug, Serialize)]
pub struct McpResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<McpError>,
    pub id: Option<Value>,
}

/// MCP error structure
#[derive(Debug, Serialize)]
pub struct McpError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// Definition of an MCP tool (name, description, JSON Schema for input).
///
/// This is a pure-data struct with no OS-specific dependencies, suitable for
/// compilation to WASM/WASIX targets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

/// Trait for providing tool listing and execution to the core protocol handler.
///
/// Implement this trait in the deployment-specific crate (e.g., velocity-mcp-edge)
/// to supply actual tool definitions and execution logic. The core crate handles
/// all MCP protocol framing; the executor handles the tool semantics.
///
/// # Constraints for WASM/WASIX deployment
///
/// - Must be pure compute (no filesystem, no process spawning, no network)
/// - Must complete within the caller's timeout (typically 5 seconds)
/// - Must not panic — return errors as `Err(String)` instead
pub trait ToolExecutor {
    /// Return the list of available tools with their schemas.
    fn list_tools(&self) -> Vec<ToolDefinition>;

    /// Execute a tool by name with JSON arguments. Returns the tool's text output.
    ///
    /// Implementations should enforce their own timeout and memory limits.
    /// Errors are returned as `Err(message)` and surfaced to the MCP client.
    fn call_tool(&self, name: &str, arguments: &Value) -> Result<String, String>;
}

/// Process an MCP request and return a response (stub mode — no tool execution).
///
/// This is the backwards-compatible entry point. Tools/list returns empty and
/// tools/call returns a placeholder message. Use [`handle_mcp_request_with_executor`]
/// for real tool execution.
pub fn handle_mcp_request(request: &McpRequest) -> McpResponse {
    match request.method.as_str() {
        "initialize" => handle_initialize(request),
        "tools/list" => handle_tools_list_stub(request),
        "tools/call" => handle_tools_call_stub(request),
        "resources/list" => handle_resources_list(request),
        "prompts/list" => handle_prompts_list(request),
        "ping" => handle_ping(request),
        _ => McpResponse {
            jsonrpc: "2.0".to_string(),
            result: None,
            error: Some(McpError {
                code: -32601,
                message: format!("Method not found: {}", request.method),
                data: None,
            }),
            id: request.id.clone(),
        },
    }
}

/// Process an MCP request with a real tool executor for actual tool execution.
///
/// This is the preferred entry point for deployments that have real tools.
/// The executor provides tool definitions and execution logic.
pub fn handle_mcp_request_with_executor(
    request: &McpRequest,
    executor: &dyn ToolExecutor,
) -> McpResponse {
    match request.method.as_str() {
        "initialize" => handle_initialize(request),
        "tools/list" => handle_tools_list_with_executor(request, executor),
        "tools/call" => handle_tools_call_with_executor(request, executor),
        "resources/list" => handle_resources_list(request),
        "prompts/list" => handle_prompts_list(request),
        "ping" => handle_ping(request),
        _ => McpResponse {
            jsonrpc: "2.0".to_string(),
            result: None,
            error: Some(McpError {
                code: -32601,
                message: format!("Method not found: {}", request.method),
                data: None,
            }),
            id: request.id.clone(),
        },
    }
}

fn handle_initialize(request: &McpRequest) -> McpResponse {
    let capabilities = serde_json::json!({
        "protocolVersion": "2024-11-05",
        "capabilities": {
            "tools": {}
        },
        "serverInfo": {
            "name": "velocity-mcp-edge",
            "version": "3.2.0"
        }
    });

    McpResponse {
        jsonrpc: "2.0".to_string(),
        result: Some(capabilities),
        error: None,
        id: request.id.clone(),
    }
}

/// Stub tools/list — returns empty tool list (backwards compatible).
fn handle_tools_list_stub(_request: &McpRequest) -> McpResponse {
    McpResponse {
        jsonrpc: "2.0".to_string(),
        result: Some(serde_json::json!({
            "tools": []
        })),
        error: None,
        id: None,
    }
}

/// Real tools/list — returns tool definitions from the executor.
fn handle_tools_list_with_executor(
    _request: &McpRequest,
    executor: &dyn ToolExecutor,
) -> McpResponse {
    let tools = executor.list_tools();
    let tools_json: Vec<Value> = tools
        .iter()
        .map(|t| {
            serde_json::json!({
                "name": t.name,
                "description": t.description,
                "inputSchema": t.input_schema,
            })
        })
        .collect();

    McpResponse {
        jsonrpc: "2.0".to_string(),
        result: Some(serde_json::json!({
            "tools": tools_json
        })),
        error: None,
        id: None,
    }
}

/// Stub tools/call — returns placeholder message (backwards compatible).
fn handle_tools_call_stub(request: &McpRequest) -> McpResponse {
    let tool_name = request
        .params
        .as_ref()
        .and_then(|p| p["name"].as_str())
        .unwrap_or("unknown");

    McpResponse {
        jsonrpc: "2.0".to_string(),
        result: Some(serde_json::json!({
            "content": [{
                "type": "text",
                "text": format!("Tool '{}' execution requires WASM runtime integration", tool_name)
            }]
        })),
        error: None,
        id: request.id.clone(),
    }
}

/// Real tools/call — dispatches to the executor and formats the MCP response.
fn handle_tools_call_with_executor(
    request: &McpRequest,
    executor: &dyn ToolExecutor,
) -> McpResponse {
    let params = request.params.as_ref();
    let tool_name = params.and_then(|p| p["name"].as_str()).unwrap_or("");
    let arguments = params
        .and_then(|p| p.get("arguments"))
        .cloned()
        .unwrap_or(Value::Object(serde_json::Map::new()));

    if tool_name.is_empty() {
        return McpResponse {
            jsonrpc: "2.0".to_string(),
            result: None,
            error: Some(McpError {
                code: -32602,
                message: "Missing required field: params.name".to_string(),
                data: None,
            }),
            id: request.id.clone(),
        };
    }

    match executor.call_tool(tool_name, &arguments) {
        Ok(output) => McpResponse {
            jsonrpc: "2.0".to_string(),
            result: Some(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": output
                }]
            })),
            error: None,
            id: request.id.clone(),
        },
        Err(err_msg) => McpResponse {
            jsonrpc: "2.0".to_string(),
            result: Some(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": format!("Error: {}", err_msg)
                }],
                "isError": true
            })),
            error: None,
            id: request.id.clone(),
        },
    }
}

fn handle_resources_list(_request: &McpRequest) -> McpResponse {
    McpResponse {
        jsonrpc: "2.0".to_string(),
        result: Some(serde_json::json!({
            "resources": []
        })),
        error: None,
        id: None,
    }
}

fn handle_prompts_list(_request: &McpRequest) -> McpResponse {
    McpResponse {
        jsonrpc: "2.0".to_string(),
        result: Some(serde_json::json!({
            "prompts": []
        })),
        error: None,
        id: None,
    }
}

fn handle_ping(request: &McpRequest) -> McpResponse {
    McpResponse {
        jsonrpc: "2.0".to_string(),
        result: Some(serde_json::json!({"status": "ok"})),
        error: None,
        id: request.id.clone(),
    }
}

/// Parse raw JSON bytes into an McpRequest
pub fn parse_request(bytes: &[u8]) -> Result<McpRequest, String> {
    serde_json::from_slice(bytes).map_err(|e| format!("Invalid JSON: {}", e))
}

/// Serialize an McpResponse to JSON bytes
pub fn serialize_response(response: &McpResponse) -> Vec<u8> {
    serde_json::to_vec(response).unwrap_or_else(|_| {
        b"{\"jsonrpc\":\"2.0\",\"error\":{\"code\":-32000,\"message\":\"Serialization failed\"},\"id\":null}".to_vec()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_and_handle_initialize() {
        let json = br#"{"jsonrpc":"2.0","method":"initialize","params":{},"id":1}"#;
        let request = parse_request(json).unwrap();
        assert_eq!(request.method, "initialize");

        let response = handle_mcp_request(&request);
        assert!(response.result.is_some());
    }

    #[test]
    fn test_handle_ping() {
        let json = br#"{"jsonrpc":"2.0","method":"ping","id":99}"#;
        let request = parse_request(json).unwrap();
        let response = handle_mcp_request(&request);
        assert_eq!(response.id, Some(serde_json::json!(99)));
    }

    #[test]
    fn test_stub_tools_list_returns_empty() {
        let json = br#"{"jsonrpc":"2.0","method":"tools/list","params":{},"id":1}"#;
        let request = parse_request(json).unwrap();
        let response = handle_mcp_request(&request);
        let result = response.result.unwrap();
        let tools = result["tools"].as_array().unwrap();
        assert!(tools.is_empty());
    }

    #[test]
    fn test_stub_tools_call_returns_placeholder() {
        let json = br#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"test","arguments":{}},"id":2}"#;
        let request = parse_request(json).unwrap();
        let response = handle_mcp_request(&request);
        let result = response.result.unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("test"));
    }

    /// Mock executor for testing the executor path.
    struct MockExecutor;

    impl ToolExecutor for MockExecutor {
        fn list_tools(&self) -> Vec<ToolDefinition> {
            vec![
                ToolDefinition {
                    name: "echo".to_string(),
                    description: "Echo back the input".to_string(),
                    input_schema: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "message": { "type": "string" }
                        },
                        "required": ["message"]
                    }),
                },
                ToolDefinition {
                    name: "add".to_string(),
                    description: "Add two numbers".to_string(),
                    input_schema: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "a": { "type": "number" },
                            "b": { "type": "number" }
                        },
                        "required": ["a", "b"]
                    }),
                },
            ]
        }

        fn call_tool(&self, name: &str, arguments: &Value) -> Result<String, String> {
            match name {
                "echo" => {
                    let msg = arguments["message"].as_str().ok_or("message is required")?;
                    Ok(msg.to_string())
                }
                "add" => {
                    let a = arguments["a"].as_f64().ok_or("a is required")?;
                    let b = arguments["b"].as_f64().ok_or("b is required")?;
                    Ok(format!("{}", a + b))
                }
                _ => Err(format!("Unknown tool: {}", name)),
            }
        }
    }

    #[test]
    fn test_executor_tools_list_returns_real_tools() {
        let executor = MockExecutor;
        let json = br#"{"jsonrpc":"2.0","method":"tools/list","params":{},"id":1}"#;
        let request = parse_request(json).unwrap();
        let response = handle_mcp_request_with_executor(&request, &executor);

        let result = response.result.unwrap();
        let tools = result["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0]["name"].as_str().unwrap(), "echo");
        assert_eq!(tools[1]["name"].as_str().unwrap(), "add");
    }

    #[test]
    fn test_executor_tools_call_echo() {
        let executor = MockExecutor;
        let json = br#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"echo","arguments":{"message":"hello"}},"id":1}"#;
        let request = parse_request(json).unwrap();
        let response = handle_mcp_request_with_executor(&request, &executor);

        let result = response.result.unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        assert_eq!(text, "hello");
    }

    #[test]
    fn test_executor_tools_call_add() {
        let executor = MockExecutor;
        let json = br#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"add","arguments":{"a":3,"b":4}},"id":1}"#;
        let request = parse_request(json).unwrap();
        let response = handle_mcp_request_with_executor(&request, &executor);

        let result = response.result.unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        assert_eq!(text, "7");
    }

    #[test]
    fn test_executor_tools_call_unknown_tool() {
        let executor = MockExecutor;
        let json = br#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"nonexistent","arguments":{}},"id":1}"#;
        let request = parse_request(json).unwrap();
        let response = handle_mcp_request_with_executor(&request, &executor);

        let result = response.result.unwrap();
        let is_error = result["isError"].as_bool().unwrap();
        assert!(is_error);
        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("Unknown tool"));
    }

    #[test]
    fn test_executor_tools_call_missing_name() {
        let executor = MockExecutor;
        let json = br#"{"jsonrpc":"2.0","method":"tools/call","params":{"arguments":{}},"id":1}"#;
        let request = parse_request(json).unwrap();
        let response = handle_mcp_request_with_executor(&request, &executor);

        assert!(response.error.is_some());
        assert_eq!(response.error.unwrap().code, -32602);
    }

    #[test]
    fn test_executor_preserves_request_id() {
        let executor = MockExecutor;
        let json = br#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"echo","arguments":{"message":"test"}},"id":42}"#;
        let request = parse_request(json).unwrap();
        let response = handle_mcp_request_with_executor(&request, &executor);
        assert_eq!(response.id, Some(serde_json::json!(42)));
    }
}
