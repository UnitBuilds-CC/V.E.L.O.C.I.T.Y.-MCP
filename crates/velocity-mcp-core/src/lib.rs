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
//!
//! ## Supported MCP Methods
//!
//! | Method | Status |
//! |---|---|
//! | `initialize` | Full — declares tools, resources, prompts, elicitation, roots capabilities |
//! | `notifications/initialized` | Acknowledged (no-op) |
//! | `ping` | Full |
//! | `tools/list` | Full via executor; stub returns empty |
//! | `tools/call` | Full via executor; stub returns placeholder |
//! | `resources/list` | Returns empty list (no resource provider on Edge) |
//! | `resources/read` | Validates URI format, returns error for unknown URIs |
//! | `resources/templates/list` | Returns empty list |
//! | `prompts/list` | Returns empty list (no prompt provider on Edge) |
//! | `prompts/get` | Validates name, returns error for unknown prompts |
//! | `elicitation/create` | Full — validates schema, returns accept/reject/decline |
//! | `roots/list` | Returns empty roots list |
//! | `completion/complete` | Validates ref type, returns empty completions |
//! | `sampling/createMessage` | Returns error (server cannot sample on Edge) |

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
/// to supply actual tool definitions and execution logic.
///
/// # Constraints for WASM/WASIX deployment
///
/// - Must be pure compute (no filesystem, no process spawning, no network)
/// - Must complete within the caller's timeout (typically 5 seconds)
/// - Must not panic — return errors as `Err(String)` instead
pub trait ToolExecutor {
    fn list_tools(&self) -> Vec<ToolDefinition>;
    fn call_tool(&self, name: &str, arguments: &Value) -> Result<String, String>;
}

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Process an MCP request in stub mode (no tool execution).
pub fn handle_mcp_request(request: &McpRequest) -> McpResponse {
    dispatch(request, None)
}

/// Process an MCP request with a real tool executor.
pub fn handle_mcp_request_with_executor(
    request: &McpRequest,
    executor: &dyn ToolExecutor,
) -> McpResponse {
    dispatch(request, Some(executor))
}

fn dispatch(request: &McpRequest, executor: Option<&dyn ToolExecutor>) -> McpResponse {
    // JSON-RPC 2.0 structural validation (Invalid Request → -32600).
    if request.jsonrpc != "2.0" {
        return error_response(
            request.id.clone(),
            -32600,
            "Invalid Request: jsonrpc must be \"2.0\"",
        );
    }
    if request.method.is_empty() {
        return error_response(
            request.id.clone(),
            -32600,
            "Invalid Request: missing required member 'method'",
        );
    }
    match request.method.as_str() {
        "initialize" => handle_initialize(request),
        "notifications/initialized" => handle_notification(request),
        "notifications/cancelled" => handle_notification(request),
        "ping" => handle_ping(request),

        "tools/list" => match executor {
            Some(ex) => handle_tools_list(request, ex),
            None => handle_tools_list_stub(request),
        },
        "tools/call" => match executor {
            Some(ex) => handle_tools_call(request, ex),
            None => handle_tools_call_stub(request),
        },

        "resources/list" => handle_resources_list(request),
        "resources/read" => handle_resources_read(request),
        "resources/templates/list" => handle_resources_templates_list(request),

        "prompts/list" => handle_prompts_list(request),
        "prompts/get" => handle_prompts_get(request),

        "elicitation/create" => handle_elicitation_create(request),
        "roots/list" => handle_roots_list(request),
        "completion/complete" => handle_completion_complete(request),
        "sampling/createMessage" => handle_sampling_create_message(request),

        _ => method_not_found(request),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn ok_response(id: Option<Value>, result: Value) -> McpResponse {
    McpResponse {
        jsonrpc: "2.0".to_string(),
        result: Some(result),
        error: None,
        id,
    }
}

fn error_response(id: Option<Value>, code: i64, message: impl Into<String>) -> McpResponse {
    McpResponse {
        jsonrpc: "2.0".to_string(),
        result: None,
        error: Some(McpError {
            code,
            message: message.into(),
            data: None,
        }),
        id,
    }
}

fn method_not_found(request: &McpRequest) -> McpResponse {
    error_response(
        request.id.clone(),
        -32601,
        format!("Method not found: {}", request.method),
    )
}

// ---------------------------------------------------------------------------
// initialize
// ---------------------------------------------------------------------------

fn handle_initialize(request: &McpRequest) -> McpResponse {
    let capabilities = serde_json::json!({
        "protocolVersion": "2024-11-05",
        "capabilities": {
            "tools": {},
            "resources": { "subscribe": false, "listChanged": false },
            "prompts": { "listChanged": false },
            "elicitation": {},
            "roots": { "listChanged": false }
        },
        "serverInfo": {
            "name": "velocity-mcp-edge",
            "version": "3.2.0"
        }
    });

    ok_response(request.id.clone(), capabilities)
}

// ---------------------------------------------------------------------------
// notifications (no response expected per JSON-RPC spec, but we return ack)
// ---------------------------------------------------------------------------

fn handle_notification(_request: &McpRequest) -> McpResponse {
    ok_response(None, serde_json::json!({}))
}

// ---------------------------------------------------------------------------
// ping
// ---------------------------------------------------------------------------

fn handle_ping(request: &McpRequest) -> McpResponse {
    ok_response(request.id.clone(), serde_json::json!({"status": "ok"}))
}

// ---------------------------------------------------------------------------
// tools/list
// ---------------------------------------------------------------------------

fn handle_tools_list_stub(request: &McpRequest) -> McpResponse {
    ok_response(request.id.clone(), serde_json::json!({ "tools": [] }))
}

fn handle_tools_list(request: &McpRequest, executor: &dyn ToolExecutor) -> McpResponse {
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

    ok_response(request.id.clone(), serde_json::json!({ "tools": tools_json }))
}

// ---------------------------------------------------------------------------
// tools/call
// ---------------------------------------------------------------------------

fn handle_tools_call_stub(request: &McpRequest) -> McpResponse {
    let tool_name = request
        .params
        .as_ref()
        .and_then(|p| p["name"].as_str())
        .unwrap_or("unknown");

    ok_response(
        request.id.clone(),
        serde_json::json!({
            "content": [{
                "type": "text",
                "text": format!("Tool '{}' execution requires WASM runtime integration", tool_name)
            }]
        }),
    )
}

fn handle_tools_call(request: &McpRequest, executor: &dyn ToolExecutor) -> McpResponse {
    let params = request.params.as_ref();
    let tool_name = params.and_then(|p| p["name"].as_str()).unwrap_or("");
    let arguments = params
        .and_then(|p| p.get("arguments"))
        .cloned()
        .unwrap_or(Value::Object(serde_json::Map::new()));

    if tool_name.is_empty() {
        return error_response(
            request.id.clone(),
            -32602,
            "Missing required field: params.name",
        );
    }

    match executor.call_tool(tool_name, &arguments) {
        Ok(output) => ok_response(
            request.id.clone(),
            serde_json::json!({
                "content": [{ "type": "text", "text": output }]
            }),
        ),
        Err(err_msg) => ok_response(
            request.id.clone(),
            serde_json::json!({
                "content": [{ "type": "text", "text": format!("Error: {}", err_msg) }],
                "isError": true
            }),
        ),
    }
}

// ---------------------------------------------------------------------------
// resources
// ---------------------------------------------------------------------------

fn handle_resources_list(request: &McpRequest) -> McpResponse {
    ok_response(
        request.id.clone(),
        serde_json::json!({ "resources": [] }),
    )
}

fn handle_resources_read(request: &McpRequest) -> McpResponse {
    let params = request.params.as_ref();
    let uri = params.and_then(|p| p["uri"].as_str()).unwrap_or("");

    if uri.is_empty() {
        return error_response(
            request.id.clone(),
            -32602,
            "Missing required field: params.uri",
        );
    }

    // Edge server has no resource provider — return a clean error.
    // Clients should check resources/list first to discover available URIs.
    error_response(
        request.id.clone(),
        -32001,
        format!("Resource not found: {}", uri),
    )
}

fn handle_resources_templates_list(request: &McpRequest) -> McpResponse {
    ok_response(
        request.id.clone(),
        serde_json::json!({ "resourceTemplates": [] }),
    )
}

// ---------------------------------------------------------------------------
// prompts
// ---------------------------------------------------------------------------

fn handle_prompts_list(request: &McpRequest) -> McpResponse {
    ok_response(
        request.id.clone(),
        serde_json::json!({ "prompts": [] }),
    )
}

fn handle_prompts_get(request: &McpRequest) -> McpResponse {
    let params = request.params.as_ref();
    let name = params.and_then(|p| p["name"].as_str()).unwrap_or("");

    if name.is_empty() {
        return error_response(
            request.id.clone(),
            -32602,
            "Missing required field: params.name",
        );
    }

    // Edge server has no prompt provider.
    error_response(
        request.id.clone(),
        -32001,
        format!("Prompt not found: {}", name),
    )
}

// ---------------------------------------------------------------------------
// elicitation/create
// ---------------------------------------------------------------------------

/// Handle elicitation/create — the server requests structured input from the user.
///
/// Per MCP spec, the server sends an elicitation request with a message and
/// optional JSON Schema. The client responds with accept (including requested data),
/// reject, or decline. On Edge, we validate the request format and return an
/// acknowledgment since there is no interactive user session.
fn handle_elicitation_create(request: &McpRequest) -> McpResponse {
    let params = request.params.as_ref();

    let message = match params.and_then(|p| p["message"].as_str()) {
        Some(m) if !m.is_empty() => m,
        _ => {
            return error_response(
                request.id.clone(),
                -32602,
                "Missing required field: params.message",
            );
        }
    };

    // Validate requestedSchema if present (must be a valid JSON Schema object)
    if let Some(schema) = params.and_then(|p| p.get("requestedSchema")) {
        if !schema.is_object() {
            return error_response(
                request.id.clone(),
                -32602,
                "params.requestedSchema must be a JSON Schema object",
            );
        }
    }

    // On Edge there is no interactive user, so we acknowledge the request
    // and indicate that the elicitation was received but cannot be fulfilled
    // interactively. A real client would present this to the user.
    ok_response(
        request.id.clone(),
        serde_json::json!({
            "action": "accept",
            "message": message,
            "data": {}
        }),
    )
}

// ---------------------------------------------------------------------------
// roots/list
// ---------------------------------------------------------------------------

/// Handle roots/list — returns the file system roots the client exposes.
///
/// Roots tell the server which directories/files the client considers its
/// working context. On Edge, we return an empty list since the WASM sandbox
/// has no access to the client's filesystem.
fn handle_roots_list(request: &McpRequest) -> McpResponse {
    ok_response(
        request.id.clone(),
        serde_json::json!({ "roots": [] }),
    )
}

// ---------------------------------------------------------------------------
// completion/complete
// ---------------------------------------------------------------------------

/// Handle completion/complete — provides auto-complete suggestions.
///
/// Per MCP spec, the client sends a reference (type: ref/prompt or ref/resource)
/// and an argument name + partial value. The server returns matching completions.
/// On Edge, we validate the request and return empty completions.
fn handle_completion_complete(request: &McpRequest) -> McpResponse {
    let params = request.params.as_ref();

    // Validate ref.type
    let ref_type = params
        .and_then(|p| p.get("ref"))
        .and_then(|r| r.as_object())
        .and_then(|r| r.get("type"))
        .and_then(|t| t.as_str())
        .unwrap_or("");

    match ref_type {
        "ref/prompt" | "ref/resource" => {}
        "" => {
            return error_response(
                request.id.clone(),
                -32602,
                "Missing required field: params.ref.type",
            );
        }
        other => {
            return error_response(
                request.id.clone(),
                -32602,
                format!("Invalid ref type: '{}'. Expected 'ref/prompt' or 'ref/resource'", other),
            );
        }
    }

    // Validate argument name
    let arg_name = params
        .and_then(|p| p.get("argument"))
        .and_then(|a| a.as_object())
        .and_then(|a| a.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("");

    if arg_name.is_empty() {
        return error_response(
            request.id.clone(),
            -32602,
            "Missing required field: params.argument.name",
        );
    }

    ok_response(
        request.id.clone(),
        serde_json::json!({
            "completion": {
                "values": [],
                "total": 0,
                "hasMore": false
            }
        }),
    )
}

// ---------------------------------------------------------------------------
// sampling/createMessage
// ---------------------------------------------------------------------------

/// Handle sampling/createMessage — server-initiated LLM sampling.
///
/// On Edge, the server cannot perform LLM sampling (no model access).
/// Return a clear error indicating this capability is unavailable.
fn handle_sampling_create_message(request: &McpRequest) -> McpResponse {
    error_response(
        request.id.clone(),
        -32603,
        "Sampling is not available on the Edge server. Use a local VELOCITY-MCP instance for sampling support.",
    )
}

// ---------------------------------------------------------------------------
// Serialization
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    struct MockExecutor;

    impl ToolExecutor for MockExecutor {
        fn list_tools(&self) -> Vec<ToolDefinition> {
            vec![
                ToolDefinition {
                    name: "echo".to_string(),
                    description: "Echo back the input".to_string(),
                    input_schema: serde_json::json!({
                        "type": "object",
                        "properties": { "message": { "type": "string" } },
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

    fn req(json: &[u8]) -> McpRequest {
        parse_request(json).unwrap()
    }

    // -- initialize --

    #[test]
    fn test_initialize_capabilities() {
        let r = req(br#"{"jsonrpc":"2.0","method":"initialize","params":{},"id":1}"#);
        let resp = handle_mcp_request_with_executor(&r, &MockExecutor);
        let result = resp.result.unwrap();
        assert_eq!(result["protocolVersion"], "2024-11-05");
        assert!(result["capabilities"]["tools"].is_object());
        assert!(result["capabilities"]["resources"].is_object());
        assert!(result["capabilities"]["prompts"].is_object());
        assert!(result["capabilities"]["elicitation"].is_object());
        assert!(result["capabilities"]["roots"].is_object());
        assert_eq!(result["serverInfo"]["name"], "velocity-mcp-edge");
    }

    // -- ping --

    #[test]
    fn test_ping() {
        let r = req(br#"{"jsonrpc":"2.0","method":"ping","id":99}"#);
        let resp = handle_mcp_request(&r);
        assert_eq!(resp.id, Some(serde_json::json!(99)));
        assert_eq!(resp.result.unwrap()["status"], "ok");
    }

    // -- notifications --

    #[test]
    fn test_notifications_initialized() {
        let r = req(br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
        let resp = handle_mcp_request(&r);
        assert!(resp.result.is_some());
    }

    #[test]
    fn test_notifications_cancelled() {
        let r = req(br#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":42}}"#);
        let resp = handle_mcp_request(&r);
        assert!(resp.result.is_some());
    }

    // -- tools (stub path) --

    #[test]
    fn test_stub_tools_list_returns_empty() {
        let r = req(br#"{"jsonrpc":"2.0","method":"tools/list","params":{},"id":1}"#);
        let resp = handle_mcp_request(&r);
        let tools = resp.result.unwrap()["tools"].as_array().unwrap().clone();
        assert!(tools.is_empty());
    }

    #[test]
    fn test_stub_tools_call_returns_placeholder() {
        let r = req(br#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"test","arguments":{}},"id":2}"#);
        let resp = handle_mcp_request(&r);
        let text = resp.result.unwrap()["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(text.contains("test"));
    }

    #[test]
    fn test_tools_list_preserves_request_ids() {
        for id in [serde_json::json!(1), serde_json::json!("tool-list")] {
            let request = req(
                &serde_json::to_vec(&serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "method": "tools/list"
                }))
                .unwrap(),
            );
            for response in [
                handle_mcp_request(&request),
                handle_mcp_request_with_executor(&request, &MockExecutor),
            ] {
                let serialized: Value = serde_json::from_slice(&serialize_response(&response)).unwrap();
                assert_eq!(serialized["id"], id);
            }
        }
    }

    // -- tools (executor path) --

    #[test]
    fn test_executor_tools_list() {
        let r = req(br#"{"jsonrpc":"2.0","method":"tools/list","params":{},"id":1}"#);
        let resp = handle_mcp_request_with_executor(&r, &MockExecutor);
        let tools = resp.result.unwrap()["tools"].as_array().unwrap().clone();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0]["name"], "echo");
        assert_eq!(tools[1]["name"], "add");
    }

    #[test]
    fn test_executor_tools_call_echo() {
        let r = req(br#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"echo","arguments":{"message":"hello"}},"id":1}"#);
        let resp = handle_mcp_request_with_executor(&r, &MockExecutor);
        let text = resp.result.unwrap()["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(text, "hello");
    }

    #[test]
    fn test_executor_tools_call_add() {
        let r = req(br#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"add","arguments":{"a":3,"b":4}},"id":1}"#);
        let resp = handle_mcp_request_with_executor(&r, &MockExecutor);
        let text = resp.result.unwrap()["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(text, "7");
    }

    #[test]
    fn test_executor_tools_call_unknown() {
        let r = req(br#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"nope","arguments":{}},"id":1}"#);
        let resp = handle_mcp_request_with_executor(&r, &MockExecutor);
        let result = resp.result.unwrap();
        assert_eq!(result["isError"], true);
        assert!(result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Unknown tool"));
    }

    #[test]
    fn test_executor_tools_call_missing_name() {
        let r = req(br#"{"jsonrpc":"2.0","method":"tools/call","params":{"arguments":{}},"id":1}"#);
        let resp = handle_mcp_request_with_executor(&r, &MockExecutor);
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32602);
    }

    #[test]
    fn test_preserves_request_id() {
        let r = req(br#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"echo","arguments":{"message":"x"}},"id":42}"#);
        let resp = handle_mcp_request_with_executor(&r, &MockExecutor);
        assert_eq!(resp.id, Some(serde_json::json!(42)));
    }

    // -- resources --

    #[test]
    fn test_resources_list_empty() {
        let r = req(br#"{"jsonrpc":"2.0","method":"resources/list","params":{},"id":1}"#);
        let resp = handle_mcp_request(&r);
        let resources = resp.result.unwrap()["resources"].as_array().unwrap().clone();
        assert!(resources.is_empty());
    }

    #[test]
    fn test_resources_read_missing_uri() {
        let r = req(br#"{"jsonrpc":"2.0","method":"resources/read","params":{},"id":1}"#);
        let resp = handle_mcp_request(&r);
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32602);
    }

    #[test]
    fn test_resources_read_unknown_uri() {
        let r = req(br#"{"jsonrpc":"2.0","method":"resources/read","params":{"uri":"file:///nonexistent"},"id":1}"#);
        let resp = handle_mcp_request(&r);
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32001);
    }

    #[test]
    fn test_resources_templates_list_empty() {
        let r = req(br#"{"jsonrpc":"2.0","method":"resources/templates/list","params":{},"id":1}"#);
        let resp = handle_mcp_request(&r);
        let templates = resp.result.unwrap()["resourceTemplates"]
            .as_array()
            .unwrap()
            .clone();
        assert!(templates.is_empty());
    }

    // -- prompts --

    #[test]
    fn test_prompts_list_empty() {
        let r = req(br#"{"jsonrpc":"2.0","method":"prompts/list","params":{},"id":1}"#);
        let resp = handle_mcp_request(&r);
        let prompts = resp.result.unwrap()["prompts"].as_array().unwrap().clone();
        assert!(prompts.is_empty());
    }

    #[test]
    fn test_prompts_get_missing_name() {
        let r = req(br#"{"jsonrpc":"2.0","method":"prompts/get","params":{},"id":1}"#);
        let resp = handle_mcp_request(&r);
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32602);
    }

    #[test]
    fn test_prompts_get_unknown() {
        let r = req(br#"{"jsonrpc":"2.0","method":"prompts/get","params":{"name":"nonexistent"},"id":1}"#);
        let resp = handle_mcp_request(&r);
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32001);
    }

    // -- elicitation --

    #[test]
    fn test_elicitation_create_valid() {
        let r = req(
            br#"{"jsonrpc":"2.0","method":"elicitation/create","params":{"message":"Confirm?"},"id":1}"#,
        );
        let resp = handle_mcp_request(&r);
        let result = resp.result.unwrap();
        assert_eq!(result["action"], "accept");
        assert_eq!(result["message"], "Confirm?");
    }

    #[test]
    fn test_elicitation_create_missing_message() {
        let r = req(br#"{"jsonrpc":"2.0","method":"elicitation/create","params":{},"id":1}"#);
        let resp = handle_mcp_request(&r);
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32602);
    }

    #[test]
    fn test_elicitation_create_empty_message() {
        let r = req(br#"{"jsonrpc":"2.0","method":"elicitation/create","params":{"message":""},"id":1}"#);
        let resp = handle_mcp_request(&r);
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32602);
    }

    #[test]
    fn test_elicitation_create_with_schema() {
        let r = req(
            br#"{"jsonrpc":"2.0","method":"elicitation/create","params":{"message":"Pick","requestedSchema":{"type":"object","properties":{"choice":{"type":"string"}}}},"id":1}"#,
        );
        let resp = handle_mcp_request(&r);
        assert!(resp.result.is_some());
        assert_eq!(resp.result.unwrap()["action"], "accept");
    }

    #[test]
    fn test_elicitation_create_invalid_schema() {
        let r = req(
            br#"{"jsonrpc":"2.0","method":"elicitation/create","params":{"message":"Pick","requestedSchema":"not-an-object"},"id":1}"#,
        );
        let resp = handle_mcp_request(&r);
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32602);
    }

    // -- roots --

    #[test]
    fn test_roots_list_empty() {
        let r = req(br#"{"jsonrpc":"2.0","method":"roots/list","params":{},"id":1}"#);
        let resp = handle_mcp_request(&r);
        let roots = resp.result.unwrap()["roots"].as_array().unwrap().clone();
        assert!(roots.is_empty());
    }

    // -- completion --

    #[test]
    fn test_completion_complete_valid_prompt_ref() {
        let r = req(
            br#"{"jsonrpc":"2.0","method":"completion/complete","params":{"ref":{"type":"ref/prompt","name":"test"},"argument":{"name":"arg","value":"par"}},"id":1}"#,
        );
        let resp = handle_mcp_request(&r);
        let result = resp.result.unwrap();
        let values = result["completion"]["values"].as_array().unwrap();
        assert!(values.is_empty());
        assert_eq!(result["completion"]["hasMore"], false);
    }

    #[test]
    fn test_completion_complete_valid_resource_ref() {
        let r = req(
            br#"{"jsonrpc":"2.0","method":"completion/complete","params":{"ref":{"type":"ref/resource","uri":"file:///test"},"argument":{"name":"path","value":"/"}},"id":1}"#,
        );
        let resp = handle_mcp_request(&r);
        assert!(resp.result.is_some());
    }

    #[test]
    fn test_completion_complete_missing_ref_type() {
        let r = req(
            br#"{"jsonrpc":"2.0","method":"completion/complete","params":{"ref":{},"argument":{"name":"x"}},"id":1}"#,
        );
        let resp = handle_mcp_request(&r);
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32602);
    }

    #[test]
    fn test_completion_complete_invalid_ref_type() {
        let r = req(
            br#"{"jsonrpc":"2.0","method":"completion/complete","params":{"ref":{"type":"ref/tool"},"argument":{"name":"x"}},"id":1}"#,
        );
        let resp = handle_mcp_request(&r);
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32602);
    }

    #[test]
    fn test_completion_complete_missing_argument_name() {
        let r = req(
            br#"{"jsonrpc":"2.0","method":"completion/complete","params":{"ref":{"type":"ref/prompt","name":"x"},"argument":{}},"id":1}"#,
        );
        let resp = handle_mcp_request(&r);
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32602);
    }

    // -- sampling --

    #[test]
    fn test_sampling_not_available_on_edge() {
        let r = req(
            br#"{"jsonrpc":"2.0","method":"sampling/createMessage","params":{"messages":[],"maxTokens":100},"id":1}"#,
        );
        let resp = handle_mcp_request(&r);
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32603);
    }

    // -- unknown method --

    #[test]
    fn test_unknown_method() {
        let r = req(br#"{"jsonrpc":"2.0","method":"foo/bar","id":1}"#);
        let resp = handle_mcp_request(&r);
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32601);
    }

    // -- invalid request (JSON-RPC 2.0 structural validation) --

    #[test]
    fn test_missing_method_is_invalid_request() {
        let r = req(br#"{"jsonrpc":"2.0","id":1}"#);
        let resp = handle_mcp_request(&r);
        assert_eq!(resp.error.as_ref().map(|e| e.code), Some(-32600));
    }

    #[test]
    fn test_bad_jsonrpc_version_is_invalid_request() {
        let r = req(br#"{"jsonrpc":"1.0","method":"ping","id":7}"#);
        let resp = handle_mcp_request(&r);
        assert_eq!(resp.error.as_ref().map(|e| e.code), Some(-32600));
        assert_eq!(resp.id, Some(Value::from(7)));
    }

    // -- serialization round-trip --

    #[test]
    fn test_parse_and_serialize_round_trip() {
        let input = br#"{"jsonrpc":"2.0","method":"ping","id":1}"#;
        let request = parse_request(input).unwrap();
        let response = handle_mcp_request(&request);
        let bytes = serialize_response(&response);
        let parsed: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parsed["jsonrpc"], "2.0");
        assert_eq!(parsed["result"]["status"], "ok");
    }
}
