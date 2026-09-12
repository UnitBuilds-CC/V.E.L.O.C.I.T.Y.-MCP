//! Unit tests for HTTP handler functions in velocity-mcp-edge.
//!
//! Tests cover: process_mcp_request, error_response, error_response_bytes

use hyper::StatusCode;
use http_body_util::BodyExt;
use velocity_mcp_edge::{process_mcp_request, error_response, error_response_bytes};

// ---------------------------------------------------------------------------
// error_response_bytes tests
// ---------------------------------------------------------------------------

#[test]
fn test_error_response_bytes_contains_jsonrpc() {
    let bytes = error_response_bytes("Something went wrong");
    let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(parsed["jsonrpc"], "2.0");
}

#[test]
fn test_error_response_bytes_contains_error_code() {
    let bytes = error_response_bytes("test error");
    let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(parsed["error"]["code"], -32700);
}

#[test]
fn test_error_response_bytes_contains_message() {
    let bytes = error_response_bytes("custom error message");
    let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(parsed["error"]["message"], "custom error message");
}

#[test]
fn test_error_response_bytes_id_is_null() {
    let bytes = error_response_bytes("test");
    let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(parsed["id"].is_null());
}

#[test]
fn test_error_response_bytes_empty_message() {
    let bytes = error_response_bytes("");
    let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(parsed["error"]["message"], "");
}

#[test]
fn test_error_response_bytes_unicode_message() {
    let bytes = error_response_bytes("错误メッセージ 🚀");
    let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(parsed["error"]["message"], "错误メッセージ 🚀");
}

#[test]
fn test_error_response_bytes_valid_json() {
    let bytes = error_response_bytes("test");
    // Must be valid JSON
    assert!(serde_json::from_slice::<serde_json::Value>(&bytes).is_ok());
}

// ---------------------------------------------------------------------------
// error_response tests (HTTP Response object)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_error_response_status_not_found() {
    let resp = error_response(StatusCode::NOT_FOUND, "not found");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_error_response_status_bad_request() {
    let resp = error_response(StatusCode::BAD_REQUEST, "bad request");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_error_response_status_internal_server_error() {
    let resp = error_response(StatusCode::INTERNAL_SERVER_ERROR, "server error");
    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn test_error_response_has_content_type_json() {
    let resp = error_response(StatusCode::BAD_REQUEST, "test");
    let ct = resp.headers().get("content-type").unwrap();
    assert_eq!(ct, "application/json");
}

#[tokio::test]
async fn test_error_response_body_is_valid_json() {
    let resp = error_response(StatusCode::NOT_FOUND, "Endpoint not found");
    let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let parsed: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(parsed["jsonrpc"], "2.0");
    assert_eq!(parsed["error"]["message"], "Endpoint not found");
}

#[tokio::test]
async fn test_error_response_code_matches_status() {
    let resp = error_response(StatusCode::METHOD_NOT_ALLOWED, "method not allowed");
    let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let parsed: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    // 405 for METHOD_NOT_ALLOWED
    assert_eq!(parsed["error"]["code"], 405);
}

// ---------------------------------------------------------------------------
// process_mcp_request edge cases
// ---------------------------------------------------------------------------

#[test]
fn test_process_mcp_request_initialize() {
    let json = br#"{"jsonrpc":"2.0","method":"initialize","params":{"protocolVersion":"2024-11-05"},"id":1}"#;
    let result = process_mcp_request(json);
    let parsed: serde_json::Value = serde_json::from_slice(&result).unwrap();
    assert!(parsed["result"]["protocolVersion"].is_string());
    assert!(parsed["result"]["serverInfo"]["name"].is_string());
}

#[test]
fn test_process_mcp_request_tools_list() {
    let json = br#"{"jsonrpc":"2.0","method":"tools/list","id":2}"#;
    let result = process_mcp_request(json);
    let parsed: serde_json::Value = serde_json::from_slice(&result).unwrap();
    assert!(parsed["result"]["tools"].is_array());
}

#[test]
fn test_process_mcp_request_tools_call_with_arguments() {
    let json = br#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"echo","arguments":{"msg":"hello"}},"id":3}"#;
    let result = process_mcp_request(json);
    let parsed: serde_json::Value = serde_json::from_slice(&result).unwrap();
    assert!(parsed["result"]["content"].is_array());
}

#[test]
fn test_process_mcp_request_unknown_method() {
    let json = br#"{"jsonrpc":"2.0","method":"unknown/xyz","id":4}"#;
    let result = process_mcp_request(json);
    let parsed: serde_json::Value = serde_json::from_slice(&result).unwrap();
    assert_eq!(parsed["error"]["code"], -32601);
}

#[test]
fn test_process_mcp_request_large_payload() {
    // Generate a large JSON payload with a big params field
    let large_value = "x".repeat(10_000);
    let json = format!(
        r#"{{"jsonrpc":"2.0","method":"ping","params":{{"data":"{}"}},"id":5}}"#,
        large_value
    );
    let result = process_mcp_request(json.as_bytes());
    let parsed: serde_json::Value = serde_json::from_slice(&result).unwrap();
    assert_eq!(parsed["result"]["status"], "ok");
}

#[test]
fn test_process_mcp_request_binary_garbage() {
    let bytes: Vec<u8> = (0..255).collect();
    let result = process_mcp_request(&bytes);
    // Should not panic, should return error response
    let parsed: serde_json::Value = serde_json::from_slice(&result).unwrap();
    assert_eq!(parsed["error"]["code"], -32700);
}

#[test]
fn test_process_mcp_request_deeply_nested_json() {
    // Deeply nested but valid JSON
    let mut nested = String::from("{}");
    for _ in 0..20 {
        nested = format!("{{\"a\":{}}}", nested);
    }
    let json = format!(
        r#"{{"jsonrpc":"2.0","method":"ping","params":{},"id":6}}"#,
        nested
    );
    let result = process_mcp_request(json.as_bytes());
    let parsed: serde_json::Value = serde_json::from_slice(&result).unwrap();
    assert_eq!(parsed["jsonrpc"], "2.0");
}

#[test]
fn test_process_mcp_request_string_id() {
    let json = br#"{"jsonrpc":"2.0","method":"ping","id":"string-id-123"}"#;
    let result = process_mcp_request(json);
    let parsed: serde_json::Value = serde_json::from_slice(&result).unwrap();
    assert_eq!(parsed["id"], "string-id-123");
}

#[test]
fn test_process_mcp_request_null_id() {
    let json = br#"{"jsonrpc":"2.0","method":"ping","id":null}"#;
    let result = process_mcp_request(json);
    let parsed: serde_json::Value = serde_json::from_slice(&result).unwrap();
    assert!(parsed["id"].is_null());
}

#[test]
fn test_process_mcp_request_missing_id() {
    let json = br#"{"jsonrpc":"2.0","method":"ping"}"#;
    let result = process_mcp_request(json);
    let parsed: serde_json::Value = serde_json::from_slice(&result).unwrap();
    // Missing id should still produce valid response
    assert_eq!(parsed["jsonrpc"], "2.0");
}

#[test]
fn test_process_mcp_request_all_methods() {
    let methods_with_params = vec![
        ("initialize", Some(r#"{}"#)),
        ("tools/list", None),
        ("tools/call", Some(r#"{"name":"echo","arguments":{"message":"hi"}}"#)),
        ("resources/list", None),
        ("prompts/list", None),
        ("ping", None),
    ];
    for (method, params) in methods_with_params {
        let json = match params {
            Some(p) => format!(r#"{{"jsonrpc":"2.0","method":"{}","params":{},"id":1}}"#, method, p),
            None => format!(r#"{{"jsonrpc":"2.0","method":"{}","id":1}}"#, method),
        };
        let result = process_mcp_request(json.as_bytes());
        let parsed: serde_json::Value = serde_json::from_slice(&result).unwrap();
        assert_eq!(parsed["jsonrpc"], "2.0");
        // Known methods should have result, not error
        assert!(
            parsed.get("result").is_some(),
            "Method {} should return result, got: {:?}",
            method,
            parsed
        );
    }
}
