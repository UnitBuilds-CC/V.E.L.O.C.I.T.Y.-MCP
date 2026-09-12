//! VELOCITY-MCP Edge Library - Testable HTTP handler functions.
//!
//! This module exposes the core HTTP handling logic for testing purposes,
//! and provides the `tools` module with pure-Rust tool implementations
//! suitable for WASM/WASIX edge deployment.

pub mod tools;

use hyper::body::Bytes;
use hyper::{Response, StatusCode};
use http_body_util::Full;
use velocity_mcp_core::{handle_mcp_request, handle_mcp_request_with_executor, parse_request, serialize_response};
use tools::EdgeToolExecutor;

/// Process MCP JSON-RPC request using the edge tool executor for real tool execution.
///
/// This is the preferred entry point. It creates an `EdgeToolExecutor` and routes
/// `tools/list` and `tools/call` through it, returning real tool definitions and
/// actual execution results.
pub fn process_mcp_request(request_body: &[u8]) -> Vec<u8> {
    match parse_request(request_body) {
        Ok(request) => {
            let executor = EdgeToolExecutor::new();
            let response = handle_mcp_request_with_executor(&request, &executor);
            serialize_response(&response)
        }
        Err(e) => error_response_bytes(&format!("Parse error: {}", e)),
    }
}

/// Process MCP JSON-RPC request in stub mode (no tool execution).
///
/// Kept for backwards compatibility and testing the stub path.
pub fn process_mcp_request_stub(request_body: &[u8]) -> Vec<u8> {
    match parse_request(request_body) {
        Ok(request) => {
            let response = handle_mcp_request(&request);
            serialize_response(&response)
        }
        Err(e) => error_response_bytes(&format!("Parse error: {}", e)),
    }
}

/// Create error response as raw bytes
pub fn error_response_bytes(message: &str) -> Vec<u8> {
    let error_json = serde_json::json!({
        "jsonrpc": "2.0",
        "error": {
            "code": -32700,
            "message": message
        },
        "id": null
    });
    serde_json::to_vec(&error_json).unwrap_or_default()
}

/// Create error response as Response object
pub fn error_response(status: StatusCode, message: &str) -> Response<Full<Bytes>> {
    let error_json = serde_json::json!({
        "jsonrpc": "2.0",
        "error": {
            "code": status.as_u16() as i64,
            "message": message
        },
        "id": null
    });
    
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(serde_json::to_vec(&error_json).unwrap_or_default())))
        .unwrap()
}
