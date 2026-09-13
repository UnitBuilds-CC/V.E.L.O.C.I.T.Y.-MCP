//! End-to-end tests for the MCP protocol over HTTP.
//!
//! These tests simulate a real MCP client connecting to the server,
//! performing initialization, listing tools, calling tools, and
//! handling error scenarios.

use crate::common;

use reqwest::Client;
use serde_json::Value;
use std::time::Duration;

/// A minimal MCP client for testing.
struct McpClient {
    client: Client,
    url: String,
    next_id: u64,
}

impl McpClient {
    fn new(base_url: &str) -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap(),
            url: format!("{}/mcp", base_url),
            next_id: 1,
        }
    }

    async fn send_request(&mut self, method: &str, params: Option<Value>) -> Value {
        let id = self.next_id;
        self.next_id += 1;

        let mut body = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "id": id,
        });
        if let Some(p) = params {
            body["params"] = p;
        }

        let resp = self
            .client
            .post(&self.url)
            .json(&body)
            .send()
            .await
            .unwrap();

        resp.json().await.unwrap()
    }

    async fn initialize(&mut self) -> Value {
        self.send_request(
            "initialize",
            Some(serde_json::json!({
                "protocolVersion": "2024-11-05",
                "clientInfo": {
                    "name": "test-client",
                    "version": "1.0.0"
                }
            })),
        )
        .await
    }

    async fn ping(&mut self) -> Value {
        self.send_request("ping", None).await
    }

    async fn tools_list(&mut self) -> Value {
        self.send_request("tools/list", None).await
    }

    async fn tools_call(&mut self, name: &str, arguments: Value) -> Value {
        self.send_request(
            "tools/call",
            Some(serde_json::json!({
                "name": name,
                "arguments": arguments
            })),
        )
        .await
    }

    async fn resources_list(&mut self) -> Value {
        self.send_request("resources/list", None).await
    }

    async fn prompts_list(&mut self) -> Value {
        self.send_request("prompts/list", None).await
    }
}

async fn setup() -> McpClient {
    let base_url = common::start_test_server().await;
    McpClient::new(&base_url)
}

// ---------------------------------------------------------------------------
// E2E: Initialization flow
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_e2e_initialize_returns_server_info() {
    let mut client = setup().await;
    let resp = client.initialize().await;
    assert_eq!(resp["jsonrpc"], "2.0");
    assert!(resp["result"]["serverInfo"].is_object());
    assert_eq!(resp["result"]["serverInfo"]["name"], "velocity-mcp-edge");
    assert_eq!(resp["result"]["serverInfo"]["version"], "3.2.0");
}

#[tokio::test]
async fn test_e2e_initialize_returns_protocol_version() {
    let mut client = setup().await;
    let resp = client.initialize().await;
    assert_eq!(resp["result"]["protocolVersion"], "2024-11-05");
}

#[tokio::test]
async fn test_e2e_initialize_returns_capabilities() {
    let mut client = setup().await;
    let resp = client.initialize().await;
    assert!(resp["result"]["capabilities"].is_object());
    assert!(resp["result"]["capabilities"].get("tools").is_some());
}

// ---------------------------------------------------------------------------
// E2E: Ping
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_e2e_ping_returns_ok() {
    let mut client = setup().await;
    let resp = client.ping().await;
    assert_eq!(resp["result"]["status"], "ok");
    assert!(resp.get("error").is_none());
}

// ---------------------------------------------------------------------------
// E2E: Tools list
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_e2e_tools_list_returns_array() {
    let mut client = setup().await;
    let resp = client.tools_list().await;
    assert!(resp["result"]["tools"].is_array());
}

// ---------------------------------------------------------------------------
// E2E: Tools call
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_e2e_tools_call_returns_content() {
    let mut client = setup().await;
    let resp = client
        .tools_call("echo", serde_json::json!({"message": "hello"}))
        .await;
    assert!(resp["result"]["content"].is_array());
    let content = &resp["result"]["content"][0];
    assert_eq!(content["type"], "text");
    // EdgeToolExecutor actually executes the echo tool, returning the message
    assert_eq!(content["text"], "hello");
}

#[tokio::test]
async fn test_e2e_tools_call_unknown_tool() {
    let mut client = setup().await;
    let resp = client
        .tools_call("nonexistent_tool", serde_json::json!({}))
        .await;
    // Currently returns a placeholder message for all tools
    assert!(resp["result"]["content"].is_array());
}

// ---------------------------------------------------------------------------
// E2E: Resources and Prompts
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_e2e_resources_list_returns_array() {
    let mut client = setup().await;
    let resp = client.resources_list().await;
    assert!(resp["result"]["resources"].is_array());
}

#[tokio::test]
async fn test_e2e_prompts_list_returns_array() {
    let mut client = setup().await;
    let resp = client.prompts_list().await;
    assert!(resp["result"]["prompts"].is_array());
}

// ---------------------------------------------------------------------------
// E2E: Error scenarios
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_e2e_unknown_method_returns_method_not_found() {
    let mut client = setup().await;
    let resp = client.send_request("invalid/method", None).await;
    assert_eq!(resp["error"]["code"], -32601);
    assert!(resp["error"]["message"]
        .as_str()
        .unwrap()
        .contains("invalid/method"));
}

#[tokio::test]
async fn test_e2e_full_session_flow() {
    let mut client = setup().await;

    // Step 1: Initialize
    let init_resp = client.initialize().await;
    assert!(init_resp["result"].is_object());

    // Step 2: Ping
    let ping_resp = client.ping().await;
    assert_eq!(ping_resp["result"]["status"], "ok");

    // Step 3: List tools
    let tools_resp = client.tools_list().await;
    assert!(tools_resp["result"]["tools"].is_array());

    // Step 4: Call a tool
    let call_resp = client
        .tools_call("test_tool", serde_json::json!({"key": "value"}))
        .await;
    assert!(call_resp["result"]["content"].is_array());

    // Step 5: List resources
    let res_resp = client.resources_list().await;
    assert!(res_resp["result"]["resources"].is_array());

    // Step 6: List prompts
    let prom_resp = client.prompts_list().await;
    assert!(prom_resp["result"]["prompts"].is_array());
}

// ---------------------------------------------------------------------------
// E2E: Response format compliance
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_e2e_response_ids_match_request() {
    let mut client = setup().await;
    // Send multiple requests and verify IDs are preserved
    for expected_id in 1..=5 {
        let resp = client.ping().await;
        assert_eq!(resp["id"], expected_id);
    }
}

#[tokio::test]
async fn test_e2e_all_responses_have_jsonrpc_version() {
    let mut client = setup().await;
    let methods = vec![
        "initialize",
        "ping",
        "tools/list",
        "resources/list",
        "prompts/list",
    ];
    for method in methods {
        let resp = client.send_request(method, None).await;
        assert_eq!(
            resp["jsonrpc"], "2.0",
            "Response for {} missing jsonrpc version",
            method
        );
    }
}
