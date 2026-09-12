//! Integration tests for security and robustness features.
//!
//! Tests verify: malformed inputs, oversized payloads, concurrent access,
//! content-type handling, and graceful error responses.

use crate::common;

use reqwest::Client;
use serde_json::Value;
use std::time::Duration;

async fn setup() -> (Client, String) {
    let base_url = common::start_test_server().await;
    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    (client, base_url)
}

// ---------------------------------------------------------------------------
// Malformed input tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_security_empty_body() {
    let (client, base) = setup().await;
    let resp = client
        .post(&format!("{}/mcp", base))
        .header("content-type", "application/json")
        .body("")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], -32700);
}

#[tokio::test]
async fn test_security_binary_payload() {
    let (client, base) = setup().await;
    let binary: Vec<u8> = (0..=255).collect();
    let resp = client
        .post(&format!("{}/mcp", base))
        .header("content-type", "application/octet-stream")
        .body(binary)
        .send()
        .await
        .unwrap();
    // Server should handle gracefully, not crash
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert!(body.get("error").is_some());
}

#[tokio::test]
async fn test_security_html_injection_attempt() {
    let (client, base) = setup().await;
    let body = r#"<script>alert("xss")</script>"#;
    let resp = client
        .post(&format!("{}/mcp", base))
        .header("content-type", "application/json")
        .body(body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let result: Value = resp.json().await.unwrap();
    // Should be a parse error, not reflected HTML
    assert_eq!(result["error"]["code"], -32700);
    // Ensure no HTML tags in error message
    let msg = result["error"]["message"].as_str().unwrap();
    assert!(!msg.contains("<script>"));
}

#[tokio::test]
async fn test_security_sql_injection_attempt() {
    let (client, base) = setup().await;
    let body = r#"{"jsonrpc":"2.0","method":"'; DROP TABLE users;--","id":1}"#;
    let resp = client
        .post(&format!("{}/mcp", base))
        .header("content-type", "application/json")
        .body(body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let result: Value = resp.json().await.unwrap();
    // Should be a "method not found" error, no SQL execution
    assert_eq!(result["error"]["code"], -32601);
}

#[tokio::test]
async fn test_security_oversized_method_name() {
    let (client, base) = setup().await;
    let long_method = "x".repeat(10_000);
    let body = format!(r#"{{"jsonrpc":"2.0","method":"{}","id":1}}"#, long_method);
    let resp = client
        .post(&format!("{}/mcp", base))
        .header("content-type", "application/json")
        .body(body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let result: Value = resp.json().await.unwrap();
    // Should be handled gracefully (method not found)
    assert!(result.get("error").is_some() || result.get("result").is_some());
}

// ---------------------------------------------------------------------------
// Large payload tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_security_large_valid_payload() {
    let (client, base) = setup().await;
    let large_data = "A".repeat(100_000);
    let body = format!(
        r#"{{"jsonrpc":"2.0","method":"ping","params":{{"data":"{}"}},"id":1}}"#,
        large_data
    );
    let resp = client
        .post(&format!("{}/mcp", base))
        .header("content-type", "application/json")
        .body(body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let result: Value = resp.json().await.unwrap();
    assert_eq!(result["result"]["status"], "ok");
}

#[tokio::test]
async fn test_security_very_large_payload_1mb() {
    let (client, base) = setup().await;
    let large_data = "B".repeat(1_000_000);
    let body = format!(
        r#"{{"jsonrpc":"2.0","method":"ping","params":{{"data":"{}"}},"id":1}}"#,
        large_data
    );
    let resp = client
        .post(&format!("{}/mcp", base))
        .header("content-type", "application/json")
        .body(body)
        .send()
        .await
        .unwrap();
    // Should either succeed or return a proper error, not crash
    assert!(resp.status() == 200 || resp.status() == 400 || resp.status() == 413);
}

// ---------------------------------------------------------------------------
// Concurrent access tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_security_concurrent_requests() {
    let (client, base) = setup().await;
    let url = format!("{}/mcp", base);

    let mut handles = vec![];
    for i in 0..50 {
        let c = client.clone();
        let u = url.clone();
        handles.push(tokio::spawn(async move {
            let body = serde_json::json!({"jsonrpc": "2.0", "method": "ping", "id": i});
            let resp = c.post(&u).json(&body).send().await.unwrap();
            assert_eq!(resp.status(), 200);
            let result: Value = resp.json().await.unwrap();
            assert_eq!(result["result"]["status"], "ok");
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }
}

#[tokio::test]
async fn test_security_mixed_concurrent_endpoints() {
    let (client, base) = setup().await;
    let mut handles = vec![];

    // Mix of health checks and MCP requests
    for i in 0..20 {
        let c = client.clone();
        let b = base.clone();
        handles.push(tokio::spawn(async move {
            if i % 2 == 0 {
                let resp = c.get(&format!("{}/health", b)).send().await.unwrap();
                assert_eq!(resp.status(), 200);
            } else {
                let body = serde_json::json!({"jsonrpc": "2.0", "method": "ping", "id": i});
                let resp = c.post(&format!("{}/mcp", b)).json(&body).send().await.unwrap();
                assert_eq!(resp.status(), 200);
            }
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }
}

// ---------------------------------------------------------------------------
// Error handling robustness
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_security_unknown_method_returns_proper_error() {
    let (client, base) = setup().await;
    let body = serde_json::json!({"jsonrpc": "2.0", "method": "system/reboot", "id": 99});
    let resp = client
        .post(&format!("{}/mcp", base))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let result: Value = resp.json().await.unwrap();
    assert_eq!(result["error"]["code"], -32601);
    // Server should stay healthy after unknown method
    let health = client.get(&format!("{}/health", base)).send().await.unwrap();
    assert_eq!(health.status(), 200);
}

#[tokio::test]
async fn test_security_rapid_sequential_requests() {
    let (client, base) = setup().await;
    let url = format!("{}/mcp", base);
    let body = serde_json::json!({"jsonrpc": "2.0", "method": "ping", "id": 1});

    // 100 rapid sequential requests
    for _ in 0..100 {
        let resp = client.post(&url).json(&body).send().await.unwrap();
        assert_eq!(resp.status(), 200);
    }
}

#[tokio::test]
async fn test_security_no_content_type_header() {
    let (client, base) = setup().await;
    let resp = client
        .post(&format!("{}/mcp", base))
        .body(r#"{"jsonrpc":"2.0","method":"ping","id":1}"#)
        .send()
        .await
        .unwrap();
    // Server should still process the request
    assert_eq!(resp.status(), 200);
    let result: Value = resp.json().await.unwrap();
    assert_eq!(result["result"]["status"], "ok");
}
