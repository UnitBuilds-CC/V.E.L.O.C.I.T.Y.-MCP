//! MCP Streaming and Progress Token support.
//!
//! Enables long-running operations to report incremental progress to clients
//! via `notifications/progress` messages. Supports streaming of partial results
//! and integration with SSE for real-time delivery.
//!
//! Features:
//! - Progress tokens for correlating updates with requests
//! - Progress notifications with optional total
//! - Streaming result chunking for large results
//! - Streaming state management
//! - Integration with HTTP SSE transport
//! - Backpressure support via channel capacity

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
#[cfg(feature = "http")]
use tokio::sync::mpsc;

/// Progress notification sent from server to client.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ProgressNotification {
    #[serde(rename = "progressToken")]
    pub progress_token: ProgressToken,
    pub progress: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
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

/// A chunk of streaming result data.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct StreamingChunk {
    pub chunk_id: u64,
    pub data: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_final: Option<bool>,
}

/// Streaming state for a long-running operation.
#[derive(Clone, Debug)]
pub struct StreamingState {
    pub token: ProgressToken,
    pub progress: u64,
    pub total: Option<u64>,
    pub chunks_sent: u64,
    pub is_complete: bool,
}

/// Extract progress token from request metadata.
pub fn extract_progress_token(params: &Value) -> Option<ProgressToken> {
    params.get("_meta")
        .and_then(|m| m.get("progressToken"))
        .map(|t| ProgressToken::from(t.clone()))
}

/// Create a progress notification JSON-RPC message.
pub fn create_progress_notification(
    token: &ProgressToken, 
    progress: u64, 
    total: Option<u64>,
    message: Option<String>,
) -> Value {
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
    if let Some(m) = message {
        msg["params"]["message"] = json!(m);
    }
    msg
}

/// Create a streaming chunk notification.
pub fn create_streaming_chunk_notification(
    token: &ProgressToken,
    chunk: &StreamingChunk,
) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "notifications/streaming",
        "params": {
            "progressToken": token,
            "chunk": chunk
        }
    })
}

/// Progress callback registry for tools that support streaming.
static PROGRESS_CALLBACKS: OnceLock<Mutex<HashMap<String, Box<dyn Fn(u64, Option<u64>) + Send + Sync>>>> = OnceLock::new();

fn get_progress_callbacks() -> &'static Mutex<HashMap<String, Box<dyn Fn(u64, Option<u64>) + Send + Sync>>> {
    PROGRESS_CALLBACKS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Streaming state registry.
static STREAMING_STATES: OnceLock<Mutex<HashMap<String, StreamingState>>> = OnceLock::new();

fn get_streaming_states() -> &'static Mutex<HashMap<String, StreamingState>> {
    STREAMING_STATES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Register a progress callback for a tool. The callback receives (progress, total).
pub fn register_progress_callback<F>(tool_name: &str, callback: F)
where
    F: Fn(u64, Option<u64>) + Send + Sync + 'static,
{
    if let Ok(mut callbacks) = get_progress_callbacks().lock() {
        const MAX_CALLBACKS: usize = 1024;
        if callbacks.len() >= MAX_CALLBACKS && !callbacks.contains_key(tool_name) {
            tracing::warn!(tool = tool_name, "Progress callback registry full ({}), rejecting", MAX_CALLBACKS);
            return;
        }
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

/// Initialize streaming state for a token.
pub fn init_streaming_state(token: &ProgressToken, total: Option<u64>) {
    let token_str = match token {
        ProgressToken::String(s) => s.clone(),
        ProgressToken::Number(n) => n.to_string(),
    };
    
    if let Ok(mut states) = get_streaming_states().lock() {
        const MAX_STATES: usize = 1024;
        if states.len() >= MAX_STATES && !states.contains_key(&token_str) {
            tracing::warn!(token = %token_str, "Streaming state registry full ({}), rejecting", MAX_STATES);
            return;
        }
        states.insert(token_str, StreamingState {
            token: token.clone(),
            progress: 0,
            total,
            chunks_sent: 0,
            is_complete: false,
        });
    }
}

/// Update streaming state with progress.
pub fn update_streaming_progress(token: &ProgressToken, progress: u64, total: Option<u64>) {
    let token_str = match token {
        ProgressToken::String(s) => s.clone(),
        ProgressToken::Number(n) => n.to_string(),
    };
    
    if let Ok(mut states) = get_streaming_states().lock() {
        if let Some(state) = states.get_mut(&token_str) {
            state.progress = progress;
            if let Some(t) = total {
                state.total = Some(t);
            }
        }
    }
}

/// Record a streaming chunk.
pub fn record_streaming_chunk(token: &ProgressToken) {
    let token_str = match token {
        ProgressToken::String(s) => s.clone(),
        ProgressToken::Number(n) => n.to_string(),
    };
    
    if let Ok(mut states) = get_streaming_states().lock() {
        if let Some(state) = states.get_mut(&token_str) {
            state.chunks_sent += 1;
        }
    }
}

/// Mark streaming as complete.
pub fn complete_streaming(token: &ProgressToken) {
    let token_str = match token {
        ProgressToken::String(s) => s.clone(),
        ProgressToken::Number(n) => n.to_string(),
    };
    
    if let Ok(mut states) = get_streaming_states().lock() {
        states.remove(&token_str);
    }
}

/// Get streaming state for a token.
pub fn get_streaming_state(token: &ProgressToken) -> Option<StreamingState> {
    let token_str = match token {
        ProgressToken::String(s) => s.clone(),
        ProgressToken::Number(n) => n.to_string(),
    };
    
    if let Ok(states) = get_streaming_states().lock() {
        states.get(&token_str).cloned()
    } else {
        None
    }
}

/// Handle notifications/progress from client (client reporting progress to server).
pub fn handle_progress_notification(params: &Value) {
    let token = params.get("progressToken").and_then(|t| t.as_str());
    let progress = params.get("progress").and_then(|p| p.as_u64());
    let total = params.get("total").and_then(|t| t.as_u64());
    tracing::debug!(?token, ?progress, ?total, "Progress notification received");
}

/// Check if a tool supports progress reporting.
pub fn tool_supports_progress(tool_name: &str) -> bool {
    if let Ok(callbacks) = get_progress_callbacks().lock() {
        callbacks.contains_key(tool_name)
    } else {
        false
    }
}

/// Split a large result into chunks for streaming.
pub fn chunk_result(data: &Value, chunk_size: usize) -> Vec<StreamingChunk> {
    let mut chunks = Vec::new();
    
    match data {
        Value::String(s) => {
            let chars: Vec<char> = s.chars().collect();
            for (i, chunk) in chars.chunks(chunk_size).enumerate() {
                chunks.push(StreamingChunk {
                    chunk_id: i as u64,
                    data: json!(chunk.iter().collect::<String>()),
                    is_final: Some(i == (chars.len() + chunk_size - 1) / chunk_size - 1),
                });
            }
        }
        Value::Array(arr) => {
            for (i, chunk) in arr.chunks(chunk_size).enumerate() {
                chunks.push(StreamingChunk {
                    chunk_id: i as u64,
                    data: json!(chunk),
                    is_final: Some(i == (arr.len() + chunk_size - 1) / chunk_size - 1),
                });
            }
        }
        _ => {
            // For non-chunkable data, return as single chunk
            chunks.push(StreamingChunk {
                chunk_id: 0,
                data: data.clone(),
                is_final: Some(true),
            });
        }
    }
    
    chunks
}

/// Convert a streaming chunk to an SSE event data string.
#[cfg(feature = "http")]
pub fn chunk_to_sse_event(token: &ProgressToken, chunk: &StreamingChunk) -> String {
    let event = create_streaming_chunk_notification(token, chunk);
    serde_json::to_string(&event).unwrap_or_default()
}

/// Stream chunks via a channel for SSE delivery.
/// Returns a receiver that yields SSE-formatted chunk notifications.
#[cfg(feature = "http")]
pub fn stream_chunks_to_sse(
    token: ProgressToken,
    chunks: Vec<StreamingChunk>,
) -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel(64);
    
    tokio::spawn(async move {
        for chunk in chunks {
            let event_data = chunk_to_sse_event(&token, &chunk);
            if tx.send(event_data).await.is_err() {
                break; // Client disconnected
            }
            record_streaming_chunk(&token);
        }
    });
    
    rx
}

/// Stream chunks with backpressure support.
/// Yields chunks at a controlled rate to avoid overwhelming the client.
#[cfg(feature = "http")]
pub fn stream_chunks_with_backpressure(
    token: ProgressToken,
    chunks: Vec<StreamingChunk>,
    delay_ms: u64,
) -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel(64);
    
    tokio::spawn(async move {
        for chunk in chunks {
            let event_data = chunk_to_sse_event(&token, &chunk);
            if tx.send(event_data).await.is_err() {
                break; // Client disconnected (backpressure)
            }
            record_streaming_chunk(&token);
            
            // Add delay between chunks for backpressure
            if delay_ms > 0 {
                tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
            }
        }
    });
    
    rx
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
        let msg = create_progress_notification(&token, 50, Some(100), None);
        assert_eq!(msg["method"], "notifications/progress");
        assert_eq!(msg["params"]["progressToken"], "tok1");
        assert_eq!(msg["params"]["progress"], 50);
        assert_eq!(msg["params"]["total"], 100);
    }

    #[test]
    fn test_create_progress_notification_with_message() {
        let token = ProgressToken::Number(1);
        let msg = create_progress_notification(&token, 25, None, Some("Processing...".to_string()));
        assert_eq!(msg["params"]["progress"], 25);
        assert_eq!(msg["params"]["message"], "Processing...");
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

    #[test]
    fn test_streaming_state_management() {
        let token = ProgressToken::String("stream1".to_string());
        
        // Initialize state
        init_streaming_state(&token, Some(100));
        
        // Check initial state
        let state = get_streaming_state(&token).unwrap();
        assert_eq!(state.progress, 0);
        assert_eq!(state.total, Some(100));
        assert_eq!(state.chunks_sent, 0);
        assert!(!state.is_complete);
        
        // Update progress
        update_streaming_progress(&token, 50, None);
        let state = get_streaming_state(&token).unwrap();
        assert_eq!(state.progress, 50);
        
        // Record chunks
        record_streaming_chunk(&token);
        record_streaming_chunk(&token);
        let state = get_streaming_state(&token).unwrap();
        assert_eq!(state.chunks_sent, 2);
        
        // Complete streaming (removes entry to prevent memory leak)
        complete_streaming(&token);
        assert!(get_streaming_state(&token).is_none());
    }

    #[test]
    fn test_chunk_result_string() {
        let data = json!("Hello, World! This is a test string.");
        let chunks = chunk_result(&data, 10);
        
        assert_eq!(chunks.len(), 4);
        assert_eq!(chunks[0].chunk_id, 0);
        assert_eq!(chunks[0].data, "Hello, Wor");
        assert_eq!(chunks[3].chunk_id, 3);
        assert_eq!(chunks[3].is_final, Some(true));
    }

    #[test]
    fn test_chunk_result_array() {
        let data = json!([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        let chunks = chunk_result(&data, 3);
        
        assert_eq!(chunks.len(), 4);
        assert_eq!(chunks[0].data, json!([1, 2, 3]));
        assert_eq!(chunks[3].data, json!([10]));
        assert_eq!(chunks[3].is_final, Some(true));
    }

    #[test]
    fn test_chunk_result_non_chunkable() {
        let data = json!({"key": "value"});
        let chunks = chunk_result(&data, 10);
        
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].data, data);
        assert_eq!(chunks[0].is_final, Some(true));
    }

    #[test]
    fn test_create_streaming_chunk_notification() {
        let token = ProgressToken::String("stream1".to_string());
        let chunk = StreamingChunk {
            chunk_id: 0,
            data: json!("chunk data"),
            is_final: Some(false),
        };
        
        let msg = create_streaming_chunk_notification(&token, &chunk);
        assert_eq!(msg["method"], "notifications/streaming");
        assert_eq!(msg["params"]["progressToken"], "stream1");
        assert_eq!(msg["params"]["chunk"]["chunk_id"], 0);
        assert_eq!(msg["params"]["chunk"]["data"], "chunk data");
    }

    #[test]
    #[cfg(feature = "http")]
    fn test_chunk_to_sse_event() {
        let token = ProgressToken::String("sse_test".to_string());
        let chunk = StreamingChunk {
            chunk_id: 0,
            data: json!("test data"),
            is_final: Some(false),
        };
        
        let event_data = chunk_to_sse_event(&token, &chunk);
        assert!(!event_data.is_empty());
        
        // Verify it's valid JSON
        let parsed: Value = serde_json::from_str(&event_data).unwrap();
        assert_eq!(parsed["method"], "notifications/streaming");
    }

    #[tokio::test]
    #[cfg(feature = "http")]
    async fn test_stream_chunks_to_sse() {
        let token = ProgressToken::String("stream_sse".to_string());
        let chunks = vec![
            StreamingChunk {
                chunk_id: 0,
                data: json!("chunk1"),
                is_final: Some(false),
            },
            StreamingChunk {
                chunk_id: 1,
                data: json!("chunk2"),
                is_final: Some(true),
            },
        ];
        
        let mut rx = stream_chunks_to_sse(token, chunks);
        
        // Receive first chunk
        let event1 = rx.recv().await.unwrap();
        assert!(!event1.is_empty());
        
        // Receive second chunk
        let event2 = rx.recv().await.unwrap();
        assert!(!event2.is_empty());
        
        // Channel should be closed
        assert!(rx.recv().await.is_none());
    }
}
