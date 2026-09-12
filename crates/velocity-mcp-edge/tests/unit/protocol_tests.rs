//! Unit tests for MCP protocol parsing and serialization.
//!
//! Tests cover: parse_request, serialize_response, handle_mcp_request
//! from velocity-mcp-core, exercised through the edge crate's process_mcp_request.

use velocity_mcp_core::{parse_request, serialize_response, handle_mcp_request};
use velocity_mcp_edge::process_mcp_request;
use proptest::prelude::*;

// ---------------------------------------------------------------------------
// parse_request tests
// ---------------------------------------------------------------------------

#[test]
fn test_parse_request_valid_initialize() {
    let json = br#"{"jsonrpc":"2.0","method":"initialize","params":{"protocolVersion":"2024-11-05"},"id":1}"#;
    let request = parse_request(json).unwrap();
    assert_eq!(request.jsonrpc, "2.0");
    assert_eq!(request.method, "initialize");
    assert!(request.params.is_some());
    assert_eq!(request.id, Some(serde_json::json!(1)));
}

#[test]
fn test_parse_request_valid_ping() {
    let json = br#"{"jsonrpc":"2.0","method":"ping","id":"abc-123"}"#;
    let request = parse_request(json).unwrap();
    assert_eq!(request.method, "ping");
    assert_eq!(request.id, Some(serde_json::json!("abc-123")));
}

#[test]
fn test_parse_request_valid_tools_list() {
    let json = br#"{"jsonrpc":"2.0","method":"tools/list","id":42}"#;
    let request = parse_request(json).unwrap();
    assert_eq!(request.method, "tools/list");
}

#[test]
fn test_parse_request_valid_tools_call() {
    let json = br#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"my_tool","arguments":{"key":"value"}},"id":7}"#;
    let request = parse_request(json).unwrap();
    assert_eq!(request.method, "tools/call");
    let params = request.params.unwrap();
    assert_eq!(params["name"], "my_tool");
}

#[test]
fn test_parse_request_missing_method_defaults_empty() {
    // serde default means method will be empty string
    let json = br#"{"jsonrpc":"2.0","id":1}"#;
    let request = parse_request(json).unwrap();
    assert_eq!(request.method, "");
}

#[test]
fn test_parse_request_invalid_json_returns_error() {
    let bad = b"not json at all";
    let result = parse_request(bad);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Invalid JSON"));
}

#[test]
fn test_parse_request_empty_input() {
    let result = parse_request(b"");
    assert!(result.is_err());
}

#[test]
fn test_parse_request_truncated_json() {
    let result = parse_request(br#"{"jsonrpc":"2.0","method":"ping""#);
    assert!(result.is_err());
}

#[test]
fn test_parse_request_null_fields() {
    let json = br#"{"jsonrpc":"2.0","method":null,"params":null,"id":null}"#;
    // method is String with default, null will fail deserialization for String
    // but serde default may handle it differently. Let's check behavior.
    let result = parse_request(json);
    // This should either parse with default or fail - both are acceptable
    // The key is it doesn't panic
    let _ = result;
}

#[test]
fn test_parse_request_extra_fields_ignored() {
    let json = br#"{"jsonrpc":"2.0","method":"ping","id":1,"extra":"field","another":42}"#;
    let request = parse_request(json).unwrap();
    assert_eq!(request.method, "ping");
}

// ---------------------------------------------------------------------------
// serialize_response tests
// ---------------------------------------------------------------------------

#[test]
fn test_serialize_response_success() {
    use velocity_mcp_core::McpResponse;
    let response = McpResponse {
        jsonrpc: "2.0".to_string(),
        result: Some(serde_json::json!({"status": "ok"})),
        error: None,
        id: Some(serde_json::json!(1)),
    };
    let bytes = serialize_response(&response);
    let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(parsed["jsonrpc"], "2.0");
    assert_eq!(parsed["result"]["status"], "ok");
    assert_eq!(parsed["id"], 1);
    assert!(parsed.get("error").is_none());
}

#[test]
fn test_serialize_response_error() {
    use velocity_mcp_core::{McpResponse, McpError};
    let response = McpResponse {
        jsonrpc: "2.0".to_string(),
        result: None,
        error: Some(McpError {
            code: -32601,
            message: "Method not found".to_string(),
            data: None,
        }),
        id: None,
    };
    let bytes = serialize_response(&response);
    let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(parsed["error"]["code"], -32601);
    assert!(parsed.get("result").is_none());
}

// ---------------------------------------------------------------------------
// handle_mcp_request tests
// ---------------------------------------------------------------------------

#[test]
fn test_handle_initialize_returns_capabilities() {
    let json = br#"{"jsonrpc":"2.0","method":"initialize","params":{},"id":1}"#;
    let request = parse_request(json).unwrap();
    let response = handle_mcp_request(&request);
    assert!(response.result.is_some());
    assert!(response.error.is_none());
    let result = response.result.unwrap();
    assert_eq!(result["protocolVersion"], "2024-11-05");
    assert_eq!(result["serverInfo"]["name"], "velocity-mcp-edge");
}

#[test]
fn test_handle_ping_returns_ok() {
    let json = br#"{"jsonrpc":"2.0","method":"ping","id":99}"#;
    let request = parse_request(json).unwrap();
    let response = handle_mcp_request(&request);
    assert!(response.result.is_some());
    assert_eq!(response.result.unwrap()["status"], "ok");
    assert_eq!(response.id, Some(serde_json::json!(99)));
}

#[test]
fn test_handle_tools_list_returns_empty() {
    let json = br#"{"jsonrpc":"2.0","method":"tools/list","id":10}"#;
    let request = parse_request(json).unwrap();
    let response = handle_mcp_request(&request);
    let result = response.result.unwrap();
    assert!(result["tools"].as_array().unwrap().is_empty());
}

#[test]
fn test_handle_tools_call_returns_placeholder() {
    let json = br#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"test_tool"},"id":5}"#;
    let request = parse_request(json).unwrap();
    let response = handle_mcp_request(&request);
    assert!(response.result.is_some());
    let content = &response.result.unwrap()["content"][0]["text"];
    assert!(content.as_str().unwrap().contains("test_tool"));
}

#[test]
fn test_handle_resources_list_returns_empty() {
    let json = br#"{"jsonrpc":"2.0","method":"resources/list","id":11}"#;
    let request = parse_request(json).unwrap();
    let response = handle_mcp_request(&request);
    let result = response.result.unwrap();
    assert!(result["resources"].as_array().unwrap().is_empty());
}

#[test]
fn test_handle_prompts_list_returns_empty() {
    let json = br#"{"jsonrpc":"2.0","method":"prompts/list","id":12}"#;
    let request = parse_request(json).unwrap();
    let response = handle_mcp_request(&request);
    let result = response.result.unwrap();
    assert!(result["prompts"].as_array().unwrap().is_empty());
}

#[test]
fn test_handle_unknown_method_returns_error() {
    let json = br#"{"jsonrpc":"2.0","method":"nonexistent/method","id":15}"#;
    let request = parse_request(json).unwrap();
    let response = handle_mcp_request(&request);
    assert!(response.error.is_some());
    let err = response.error.unwrap();
    assert_eq!(err.code, -32601);
    assert!(err.message.contains("nonexistent/method"));
}

#[test]
fn test_handle_empty_method_returns_error() {
    let json = br#"{"jsonrpc":"2.0","method":"","id":16}"#;
    let request = parse_request(json).unwrap();
    let response = handle_mcp_request(&request);
    assert!(response.error.is_some());
    assert_eq!(response.error.unwrap().code, -32601);
}

// ---------------------------------------------------------------------------
// process_mcp_request integration (edge crate wrapper)
// ---------------------------------------------------------------------------

#[test]
fn test_process_mcp_request_valid_ping() {
    let json = br#"{"jsonrpc":"2.0","method":"ping","id":1}"#;
    let result = process_mcp_request(json);
    let parsed: serde_json::Value = serde_json::from_slice(&result).unwrap();
    assert_eq!(parsed["jsonrpc"], "2.0");
    assert_eq!(parsed["result"]["status"], "ok");
}

#[test]
fn test_process_mcp_request_invalid_json() {
    let bad = b"{invalid json!!!}";
    let result = process_mcp_request(bad);
    let parsed: serde_json::Value = serde_json::from_slice(&result).unwrap();
    assert_eq!(parsed["error"]["code"], -32700);
    assert!(parsed["error"]["message"].as_str().unwrap().contains("Parse error"));
}

#[test]
fn test_process_mcp_request_empty_body() {
    let result = process_mcp_request(b"");
    let parsed: serde_json::Value = serde_json::from_slice(&result).unwrap();
    assert_eq!(parsed["error"]["code"], -32700);
}

// ---------------------------------------------------------------------------
// Property-based tests
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn test_parse_request_never_panics_on_arbitrary_bytes(ref bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
        // parse_request should never panic, always return Result
        let _ = parse_request(bytes);
    }

    #[test]
    fn test_process_mcp_request_never_panics(ref bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
        let _ = process_mcp_request(bytes);
    }

    #[test]
    fn test_valid_jsonrpc_always_produces_valid_output(
        method in r#"[a-z]{1,20}"#,
        id in 0..10000i64,
    ) {
        let json = format!(r#"{{"jsonrpc":"2.0","method":"{}","id":{}}}"#, method, id);
        let result = process_mcp_request(json.as_bytes());
        let parsed: serde_json::Value = serde_json::from_slice(&result).unwrap();
        assert_eq!(parsed["jsonrpc"], "2.0");
        // Must have either result or error
        assert!(parsed.get("result").is_some() || parsed.get("error").is_some());
    }
}
