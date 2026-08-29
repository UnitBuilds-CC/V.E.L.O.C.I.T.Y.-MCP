//! MCP Streaming and Progress Token support.
//!
//! Enables long-running operations to report incremental progress to clients
//! via `notifications/progress` messages. Progress tokens are passed in request
//! metadata and used to correlate progress updates with the original request.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Progress notification sent from server to client.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ProgressNotification {
    #[serde(rename = "progressToken")]
    pub progress_token: ProgressToken,
    pub progress: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
}

/// A progress token can be a string or number.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
#[serde(untagged)]
pub enum ProgressToken {
    String(String),
    Number(u64),
}

impl From<Value> for ProgressToken {
    fn from(v: Value) -> Self {
        match v {
            Value::String(s) => ProgressToken::String(s),
            Value::Number(n) => ProgressToken::Number(n.as_u64().unwrap_or(0)),
            _ => ProgressToken::Number(0),
        }
    }
}

/// Extract progress token from request metadata.
pub fn extract_progress_token(params: &Value) -> Option<ProgressToken> {
    params.get("_meta")
        .and_then(|m| m.get("progressToken"))
        .map(|t| ProgressToken::from(t.clone()))
}

/// Create a progress notification JSON-RPC message.
pub fn create_progress_notification(token: &ProgressToken, progress: u64, total: Option<u64>) -> Value {
    let mut msg = json!({
        "jsonrpc": "2.0",
        "method": "notifications/progress",
        "params": {
            "progressToken": token,
            "progress": progress
        }
    });
    if let Some(t) = total {
        msg["params"]["total"] = json!(t);
    }
    msg
}

/// Progress callback registry for tools that support streaming.
static PROGRESS_CALLBACKS: OnceLock<Mutex<HashMap<String, Box<dyn Fn(u64, Option<u64>) + Send + Sync>>>> = OnceLock::new();

fn get_progress_callbacks() -> &'static Mutex<HashMap<String, Box<dyn Fn(u64, Option<u64>) + Send + Sync>>> {
    PROGRESS_CALLBACKS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Register a progress callback for a tool. The callback receives (progress, total).
pub fn register_progress_callback<F>(tool_name: &str, callback: F)
where
    F: Fn(u64, Option<u64>) + Send + Sync + 'static,
{
    if let Ok(mut callbacks) = get_progress_callbacks().lock() {
        callbacks.insert(tool_name.to_string(), Box::new(callback));
    }
}

/// Report progress for a tool. Looks up the registered callback and invokes it.
pub fn report_progress(tool_name: &str, progress: u64, total: Option<u64>) {
    if let Ok(callbacks) = get_progress_callbacks().lock() {
        if let Some(cb) = callbacks.get(tool_name) {
            cb(progress, total);
        }
    }
}

/// Handle notifications/progress from client (client reporting progress to server).
pub fn handle_progress_notification(params: &Value) {
    let _token = params.get("progressToken");
    let _progress = params.get("progress").and_then(|p| p.as_u64());
    let _total = params.get("total").and_then(|t| t.as_u64());
    // For now, just log. In a full implementation, this would update internal state.
}

/// Check if a tool supports progress reporting.
pub fn tool_supports_progress(tool_name: &str) -> bool {
    if let Ok(callbacks) = get_progress_callbacks().lock() {
        callbacks.contains_key(tool_name)
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_progress_token_from_meta() {
        let params = json!({
            "_meta": {"progressToken": "abc123"},
            "name": "test"
        });
        let token = extract_progress_token(&params);
        assert_eq!(token, Some(ProgressToken::String("abc123".to_string())));
    }

    #[test]
    fn test_extract_progress_token_number() {
        let params = json!({
            "_meta": {"progressToken": 42},
            "name": "test"
        });
        let token = extract_progress_token(&params);
        assert_eq!(token, Some(ProgressToken::Number(42)));
    }

    #[test]
    fn test_extract_no_progress_token() {
        let params = json!({"name": "test"});
        let token = extract_progress_token(&params);
        assert!(token.is_none());
    }

    #[test]
    fn test_create_progress_notification() {
        let token = ProgressToken::String("tok1".to_string());
        let msg = create_progress_notification(&token, 50, Some(100));
        assert_eq!(msg["method"], "notifications/progress");
        assert_eq!(msg["params"]["progressToken"], "tok1");
        assert_eq!(msg["params"]["progress"], 50);
        assert_eq!(msg["params"]["total"], 100);
    }

    #[test]
    fn test_create_progress_notification_no_total() {
        let token = ProgressToken::Number(1);
        let msg = create_progress_notification(&token, 25, None);
        assert_eq!(msg["params"]["progress"], 25);
        assert!(msg["params"].get("total").is_none());
    }

    #[test]
    fn test_register_and_report_progress() {
        let called = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let called_clone = called.clone();

        register_progress_callback("test_tool", move |progress, total| {
            assert_eq!(progress, 50);
            assert_eq!(total, Some(100));
            called_clone.store(true, std::sync::atomic::Ordering::SeqCst);
        });

        report_progress("test_tool", 50, Some(100));
        assert!(called.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn test_tool_supports_progress() {
        register_progress_callback("supported_tool", |_, _| {});
        assert!(tool_supports_progress("supported_tool"));
        assert!(!tool_supports_progress("unsupported_tool"));
    }

    #[test]
    fn test_handle_progress_notification() {
        let params = json!({
            "progressToken": "tok1",
            "progress": 75,
            "total": 100
        });
        handle_progress_notification(&params);
        // Just verify it doesn't panic
    }
}
