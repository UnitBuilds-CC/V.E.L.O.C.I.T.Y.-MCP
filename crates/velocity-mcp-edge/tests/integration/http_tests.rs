//! Integration tests for HTTP endpoints.
//!
//! Tests verify: /health, POST /mcp, POST /, method not found, CORS, etc.

use crate::common;

use reqwest::Client;
use serde_json::Value;

async fn setup() -> (Client, String) {
    let base_url = common::start_test_server().await;
    let client = Client::new();
    (client, base_url)
}

// ---------------------------------------------------------------------------
// Health endpoint tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_health_endpoint_returns_200() {
    let (client, base) = setup().await;
    let resp = client
        .get(&format!("{}/health", base))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn test_healthz_endpoint_returns_200() {
    let (client, base) = setup().await;
    let resp = client
        .get(&format!("{}/healthz", base))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn test_health_returns_json_content_type() {
    let (client, base) = setup().await;
    let resp = client
        .get(&format!("{}/health", base))
        .send()
        .await
        .unwrap();
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.contains("application/json"));
}

#[tokio::test]
async fn test_health_body_contains_healthy_status() {
    let (client, base) = setup().await;
    let resp = client
        .get(&format!("{}/health", base))
        .send()
        .await
        .unwrap();
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "healthy");
}

#[tokio::test]
async fn test_health_body_contains_version() {
    let (client, base) = setup().await;
    let resp = client
        .get(&format!("{}/health", base))
        .send()
        .await
        .unwrap();
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["version"], "3.2.0");
}

// ---------------------------------------------------------------------------
// MCP endpoint tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_post_mcp_ping() {
    let (client, base) = setup().await;
    let body = serde_json::json!({"jsonrpc": "2.0", "method": "ping", "id": 1});
    let resp = client
        .post(&format!("{}/mcp", base))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let result: Value = resp.json().await.unwrap();
    assert_eq!(result["jsonrpc"], "2.0");
    assert_eq!(result["result"]["status"], "ok");
}

#[tokio::test]
async fn test_post_root_mcp() {
    let (client, base) = setup().await;
    let body = serde_json::json!({"jsonrpc": "2.0", "method": "ping", "id": 1});
    let resp = client
        .post(&format!("{}/", base))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let result: Value = resp.json().await.unwrap();
    assert_eq!(result["result"]["status"], "ok");
}

#[tokio::test]
async fn test_post_mcp_initialize() {
    let (client, base) = setup().await;
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "initialize",
        "params": {"protocolVersion": "2024-11-05"},
        "id": 1
    });
    let resp = client
        .post(&format!("{}/mcp", base))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let result: Value = resp.json().await.unwrap();
    assert_eq!(result["result"]["protocolVersion"], "2024-11-05");
    assert_eq!(result["result"]["serverInfo"]["name"], "velocity-mcp-edge");
}

#[tokio::test]
async fn test_post_mcp_tools_list() {
    let (client, base) = setup().await;
    let body = serde_json::json!({"jsonrpc": "2.0", "method": "tools/list", "id": 2});
    let resp = client
        .post(&format!("{}/mcp", base))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let result: Value = resp.json().await.unwrap();
    assert!(result["result"]["tools"].is_array());
}

#[tokio::test]
async fn test_post_mcp_invalid_json() {
    let (client, base) = setup().await;
    let resp = client
        .post(&format!("{}/mcp", base))
        .header("content-type", "application/json")
        .body("not valid json!!!")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let result: Value = resp.json().await.unwrap();
    assert_eq!(result["error"]["code"], -32700);
}

// ---------------------------------------------------------------------------
// Method / path not found tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_get_mcp_returns_404() {
    let (client, base) = setup().await;
    let resp = client.get(&format!("{}/mcp", base)).send().await.unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_unknown_path_returns_404() {
    let (client, base) = setup().await;
    let resp = client
        .get(&format!("{}/unknown", base))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_unknown_path_body_has_error() {
    let (client, base) = setup().await;
    let resp = client.get(&format!("{}/nope", base)).send().await.unwrap();
    let body: Value = resp.json().await.unwrap();
    assert!(body.get("error").is_some());
}

#[tokio::test]
async fn test_put_mcp_returns_404() {
    let (client, base) = setup().await;
    let resp = client
        .put(&format!("{}/mcp", base))
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_delete_mcp_returns_404() {
    let (client, base) = setup().await;
    let resp = client
        .delete(&format!("{}/mcp", base))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

// ---------------------------------------------------------------------------
// Response format tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mcp_response_has_json_content_type() {
    let (client, base) = setup().await;
    let body = serde_json::json!({"jsonrpc": "2.0", "method": "ping", "id": 1});
    let resp = client
        .post(&format!("{}/mcp", base))
        .json(&body)
        .send()
        .await
        .unwrap();
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.contains("application/json"));
}

#[tokio::test]
async fn test_mcp_response_has_jsonrpc_version() {
    let (client, base) = setup().await;
    let body = serde_json::json!({"jsonrpc": "2.0", "method": "ping", "id": 1});
    let resp = client
        .post(&format!("{}/mcp", base))
        .json(&body)
        .send()
        .await
        .unwrap();
    let result: Value = resp.json().await.unwrap();
    assert_eq!(result["jsonrpc"], "2.0");
}

#[tokio::test]
async fn test_mcp_response_preserves_id() {
    let (client, base) = setup().await;
    let body = serde_json::json!({"jsonrpc": "2.0", "method": "ping", "id": 42});
    let resp = client
        .post(&format!("{}/mcp", base))
        .json(&body)
        .send()
        .await
        .unwrap();
    let result: Value = resp.json().await.unwrap();
    assert_eq!(result["id"], 42);
}
