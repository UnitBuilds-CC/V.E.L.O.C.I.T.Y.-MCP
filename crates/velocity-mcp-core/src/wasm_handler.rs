//! WASM HTTP handler for Wasmer Edge.
//!
//! This module provides the entry point when compiled to wasm32-wasip1.

use crate::{handle_mcp_request, parse_request, serialize_response};

/// Process raw HTTP request body and return response bytes.
/// This is the main entry point for Wasmer Edge deployments.
#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn handle_http_request(input_ptr: *const u8, input_len: usize) -> *mut u8 {
    use std::slice;

    // SAFETY: Called from WASM host with valid pointer/length
    let input_bytes = unsafe { slice::from_raw_parts(input_ptr, input_len) };

    // Parse MCP request
    let response_bytes = match parse_request(input_bytes) {
        Ok(request) => {
            let response = handle_mcp_request(&request);
            serialize_response(&response)
        }
        Err(_) => {
            // Sanitize: never leak internal parse errors to clients
            let error_response = serde_json::json!({
                "jsonrpc": "2.0",
                "error": {
                    "code": -32700,
                    "message": "Parse error"
                },
                "id": null
            });
            serde_json::to_vec(&error_response).unwrap_or_else(|_| {
                // This should never fail for a simple static JSON object
                b"{\"jsonrpc\":\"2.0\",\"error\":{\"code\":-32603,\"message\":\"Internal error\"},\"id\":null}".to_vec()
            })
        }
    };

    // Return pointer to response (caller must free via wasmer_free)
    let boxed = response_bytes.into_boxed_slice();
    Box::into_raw(boxed) as *mut u8
}

/// Free memory allocated by handle_http_request
#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub unsafe extern "C" fn wasmer_free(ptr: *mut u8) {
    if !ptr.is_null() {
        drop(Box::from_raw(ptr));
    }
}
