//! VELOCITY-MCP Edge Library - Testable HTTP handler functions.
//!
//! This module exposes the core HTTP handling logic for testing purposes,
//! and provides the `tools` module with pure-Rust tool implementations
//! suitable for WASM/WASIX edge deployment.

pub mod tools;

#[cfg(not(target_arch = "wasm32"))]
use http_body_util::Full;
#[cfg(not(target_arch = "wasm32"))]
use hyper::body::Bytes;
#[cfg(not(target_arch = "wasm32"))]
use hyper::{Response, StatusCode};
use tools::EdgeToolExecutor;
use velocity_mcp_core::{
    handle_mcp_request, handle_mcp_request_with_executor, parse_request, serialize_response,
};

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
        Err(_) => error_response_bytes("Parse error"), // Sanitized: don't leak internal details
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
        Err(_) => error_response_bytes("Parse error"), // Sanitized: don't leak internal details
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

/// Create error response as Response object.
///
/// Maps HTTP status codes to JSON-RPC 2.0 error codes per spec:
/// - 400 -> -32600 (Invalid Request)
/// - 401/403 -> -32600 (Invalid Request - auth failure)
/// - 404 -> -32601 (Method Not Found)
/// - 413 -> -32602 (Invalid Params - payload too large)
/// - 429 -> -32600 (Invalid Request - rate limited)
/// - 500 -> -32603 (Internal Error)
#[cfg(not(target_arch = "wasm32"))]
pub fn error_response(status: StatusCode, message: &str) -> Response<Full<Bytes>> {
    // Map HTTP status to JSON-RPC error code
    let jsonrpc_code = match status.as_u16() {
        400 | 401 | 403 | 429 => -32600, // Invalid Request
        404 | 405 => -32601,             // Method Not Found
        413 => -32602,                   // Invalid Params
        _ => -32603,                     // Internal Error (default for 5xx)
    };

    let error_json = serde_json::json!({
        "jsonrpc": "2.0",
        "error": {
            "code": jsonrpc_code,
            "message": message
        },
        "id": null
    });

    let body_bytes = serde_json::to_vec(&error_json).unwrap_or_default();

    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body_bytes)))
        .unwrap_or_else(|_| {
            // Fallback: if builder fails, return a minimal 500 response
            const FALLBACK_JSON: &[u8] = b"{\"jsonrpc\":\"2.0\",\"error\":{\"code\":-32603,\"message\":\"Internal server error\"},\"id\":null}";
            Response::builder()
                .status(500)
                .header("content-type", "application/json")
                .body(Full::new(Bytes::from_static(FALLBACK_JSON)))
                .expect("fallback 500 response should never fail")
        })
}
