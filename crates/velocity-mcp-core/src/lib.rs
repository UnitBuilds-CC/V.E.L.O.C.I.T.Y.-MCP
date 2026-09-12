//! VELOCITY-MCP Core - WASM-compatible protocol logic.
//!
//! This crate contains pure MCP protocol handling without OS-specific dependencies.
//! It can be compiled to both native and wasm32-wasip1 targets.

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

/// Process an MCP request and return a response
pub fn handle_mcp_request(request: &McpRequest) -> McpResponse {
    match request.method.as_str() {
        "initialize" => handle_initialize(request),
        "tools/list" => handle_tools_list(request),
        "tools/call" => handle_tools_call(request),
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

fn handle_tools_list(_request: &McpRequest) -> McpResponse {
    // Return empty tools list for now - actual tools come from WASM runtimes
    McpResponse {
        jsonrpc: "2.0".to_string(),
        result: Some(serde_json::json!({
            "tools": []
        })),
        error: None,
        id: None,
    }
}

fn handle_tools_call(request: &McpRequest) -> McpResponse {
    let tool_name = request.params.as_ref()
        .and_then(|p| p["name"].as_str())
        .unwrap_or("unknown");
    
    // For Edge deployment, tools are executed via WASM runtimes
    // This is a placeholder that would integrate with the wasm_runtime module
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
}
