//! MCP Sampling protocol support.
//!
//! Allows the server to request LLM sampling from the client, enabling
//! recursive/agentic behaviors where the server can ask the client's LLM
//! for assistance.
//!
//! The sampling flow:
//! 1. Server sends `sampling/createMessage` request to client
//! 2. Client invokes its LLM with the provided parameters
//! 3. Client returns the LLM response
//! 4. Server processes the response

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::{Mutex, OnceLock};

/// Sampling request sent from server to client.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SamplingRequest {
    pub messages: Vec<SamplingMessage>,
    #[serde(rename = "maxTokens", skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(rename = "includeContext", skip_serializing_if = "Option::is_none")]
    pub include_context: Option<String>,
}

/// A message in the sampling conversation.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SamplingMessage {
    pub role: String, // "user" or "assistant"
    pub content: SamplingContent,
}

/// Content of a sampling message.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SamplingContent {
    #[serde(rename = "type")]
    pub content_type: String, // "text" or "resource"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<ResourceReference>,
}

/// Reference to a resource in sampling content.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ResourceReference {
    pub uri: String,
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

/// Sampling response from client.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SamplingResponse {
    pub role: String, // "assistant"
    pub content: SamplingContent,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
}

static SAMPLING_HANDLER: OnceLock<Mutex<Option<Box<dyn SamplingCallback + Send>>>> = OnceLock::new();

/// Callback trait for handling sampling requests.
pub trait SamplingCallback {
    fn on_sampling_request(&self, request: &SamplingRequest) -> Result<SamplingResponse, String>;
}

/// Register a sampling callback. When the server receives a sampling request
/// from a client, this callback will be invoked.
pub fn register_sampling_callback<F>(callback: F)
where
    F: Fn(&SamplingRequest) -> Result<SamplingResponse, String> + Send + 'static,
{
    struct CallbackWrapper<F> {
        f: F,
    }
    impl<F> SamplingCallback for CallbackWrapper<F>
    where
        F: Fn(&SamplingRequest) -> Result<SamplingResponse, String> + Send,
    {
        fn on_sampling_request(&self, request: &SamplingRequest) -> Result<SamplingResponse, String> {
            (self.f)(request)
        }
    }

    SAMPLING_HANDLER.get_or_init(|| {
        Mutex::new(Some(Box::new(CallbackWrapper { f: callback })))
    });
}

/// Handle a sampling/createMessage request from a client.
/// This is called when the CLIENT wants the SERVER to perform sampling.
/// In our architecture, the server delegates to the registered callback.
pub fn handle_sampling_create_message(params: &Value) -> Result<Value, String> {
    let messages: Vec<SamplingMessage> = serde_json::from_value(params["messages"].clone())
        .map_err(|e| format!("Invalid messages: {}", e))?;

    let max_tokens = params["maxTokens"].as_u64().map(|v| v as u32);
    let temperature = params["temperature"].as_f64();
    let include_context = params["includeContext"].as_str().map(|s| s.to_string());

    let request = SamplingRequest {
        messages,
        max_tokens,
        temperature,
        include_context,
    };

    if let Some(handler) = SAMPLING_HANDLER.get() {
        if let Ok(lock) = handler.lock() {
            if let Some(cb) = lock.as_ref() {
                let response = cb.on_sampling_request(&request)?;
                return Ok(json!({
                    "role": response.role,
                    "content": response.content,
                    "model": response.model,
                    "stopReason": response.stop_reason
                }));
            }
        }
    }

    Err("No sampling callback registered".to_string())
}

/// Create a sampling request to send TO the client (server-initiated sampling).
/// Returns the JSON-RPC request that should be sent to the client.
pub fn create_sampling_request(
    request_id: u64,
    messages: Vec<SamplingMessage>,
    max_tokens: Option<u32>,
    temperature: Option<f64>,
) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "sampling/createMessage",
        "params": {
            "messages": messages,
            "maxTokens": max_tokens,
            "temperature": temperature
        },
        "id": request_id
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_sampling_request() {
        let messages = vec![SamplingMessage {
            role: "user".to_string(),
            content: SamplingContent {
                content_type: "text".to_string(),
                text: Some("Hello".to_string()),
                resource: None,
            },
        }];
        let req = create_sampling_request(1, messages, Some(100), Some(0.7));
        assert_eq!(req["method"], "sampling/createMessage");
        assert_eq!(req["id"], 1);
        assert_eq!(req["params"]["maxTokens"], 100);
    }

    #[test]
    fn test_handle_sampling_with_callback() {
        register_sampling_callback(|req| {
            Ok(SamplingResponse {
                role: "assistant".to_string(),
                content: SamplingContent {
                    content_type: "text".to_string(),
                    text: Some("Response".to_string()),
                    resource: None,
                },
                model: "test-model".to_string(),
                stop_reason: None,
            })
        });

        let params = json!({
            "messages": [{"role": "user", "content": {"type": "text", "text": "Hello"}}],
            "maxTokens": 100
        });
        let result = handle_sampling_create_message(&params).unwrap();
        assert_eq!(result["role"], "assistant");
        assert_eq!(result["model"], "test-model");
    }

    #[test]
    fn test_sampling_request_serialization() {
        let req = SamplingRequest {
            messages: vec![],
            max_tokens: Some(50),
            temperature: Some(0.5),
            include_context: Some("all".to_string()),
        };
        let serialized = serde_json::to_string(&req).unwrap();
        assert!(serialized.contains("maxTokens"));
        assert!(serialized.contains("temperature"));
        assert!(serialized.contains("includeContext"));
    }
}
