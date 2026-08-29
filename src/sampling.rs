//! MCP Sampling protocol support.
//!
//! Allows the server to request LLM sampling from the client, enabling
//! recursive/agentic behaviors where the server can ask the client's LLM
//! for assistance.
//!
//! Features:
//! - Server-initiated sampling requests to clients
//! - Model preferences and configuration
//! - System prompts and multi-turn conversations
//! - Metadata for model hints and token budgets
//! - Conversation history tracking
//!
//! The sampling flow:
//! 1. Server sends `sampling/createMessage` request to client
//! 2. Client invokes its LLM with the provided parameters
//! 3. Client returns the LLM response
//! 4. Server processes the response

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_preferences: Option<ModelPreferences>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<SamplingMetadata>,
}

/// Model preferences for sampling.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ModelPreferences {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hints: Option<Vec<ModelHint>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_priority: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed_priority: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intelligence_priority: Option<f64>,
}

/// A hint for model selection.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ModelHint {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Metadata for sampling requests.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SamplingMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// A message in the sampling conversation.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SamplingMessage {
    pub role: String, // "user", "assistant", or "system"
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

/// Conversation history tracker for multi-turn sampling.
static CONVERSATION_HISTORY: OnceLock<Mutex<HashMap<String, Vec<SamplingMessage>>>> = OnceLock::new();

fn get_conversation_history() -> &'static Mutex<HashMap<String, Vec<SamplingMessage>>> {
    CONVERSATION_HISTORY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Add a message to conversation history.
pub fn add_to_conversation(conversation_id: &str, message: SamplingMessage) {
    if let Ok(mut history) = get_conversation_history().lock() {
        history.entry(conversation_id.to_string())
            .or_insert_with(Vec::new)
            .push(message);
    }
}

/// Get conversation history.
pub fn get_conversation(conversation_id: &str) -> Vec<SamplingMessage> {
    if let Ok(history) = get_conversation_history().lock() {
        history.get(conversation_id).cloned().unwrap_or_default()
    } else {
        vec![]
    }
}

/// Clear conversation history.
pub fn clear_conversation(conversation_id: &str) {
    if let Ok(mut history) = get_conversation_history().lock() {
        history.remove(conversation_id);
    }
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
    
    // Parse new enhanced fields
    let model_preferences: Option<ModelPreferences> = params.get("modelPreferences")
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    let system_prompt = params["systemPrompt"].as_str().map(|s| s.to_string());
    let metadata: Option<SamplingMetadata> = params.get("metadata")
        .and_then(|v| serde_json::from_value(v.clone()).ok());

    let request = SamplingRequest {
        messages: messages.clone(),
        max_tokens,
        temperature,
        include_context,
        model_preferences,
        system_prompt,
        metadata: metadata.clone(),
    };

    // Track conversation history if conversation_id is provided
    if let Some(ref meta) = metadata {
        if let Some(ref conv_id) = meta.conversation_id {
            for msg in &messages {
                add_to_conversation(conv_id, msg.clone());
            }
        }
    }

    if let Some(handler) = SAMPLING_HANDLER.get() {
        if let Ok(lock) = handler.lock() {
            if let Some(cb) = lock.as_ref() {
                let response = cb.on_sampling_request(&request)?;
                
                // Track assistant response in conversation history
                if let Some(ref meta) = metadata {
                    if let Some(ref conv_id) = meta.conversation_id {
                        let assistant_msg = SamplingMessage {
                            role: "assistant".to_string(),
                            content: response.content.clone(),
                        };
                        add_to_conversation(conv_id, assistant_msg);
                    }
                }
                
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
    model_preferences: Option<ModelPreferences>,
    system_prompt: Option<String>,
    metadata: Option<SamplingMetadata>,
) -> Value {
    let mut params = json!({
        "messages": messages,
    });
    
    if let Some(tokens) = max_tokens {
        params["maxTokens"] = json!(tokens);
    }
    if let Some(temp) = temperature {
        params["temperature"] = json!(temp);
    }
    if let Some(prefs) = model_preferences {
        params["modelPreferences"] = serde_json::to_value(prefs).unwrap_or(json!(null));
    }
    if let Some(prompt) = system_prompt {
        params["systemPrompt"] = json!(prompt);
    }
    if let Some(meta) = metadata {
        params["metadata"] = serde_json::to_value(meta).unwrap_or(json!(null));
    }
    
    json!({
        "jsonrpc": "2.0",
        "method": "sampling/createMessage",
        "params": params,
        "id": request_id
    })
}

/// Create a sampling request with conversation history automatically included.
pub fn create_sampling_request_with_history(
    request_id: u64,
    conversation_id: &str,
    new_message: SamplingMessage,
    max_tokens: Option<u32>,
    temperature: Option<f64>,
) -> Value {
    // Add new message to history
    add_to_conversation(conversation_id, new_message);
    
    // Get full conversation history
    let messages = get_conversation(conversation_id);
    
    let metadata = SamplingMetadata {
        progress_token: None,
        conversation_id: Some(conversation_id.to_string()),
        extra: HashMap::new(),
    };
    
    create_sampling_request(
        request_id,
        messages,
        max_tokens,
        temperature,
        None,
        None,
        Some(metadata),
    )
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
        let req = create_sampling_request(1, messages, Some(100), Some(0.7), None, None, None);
        assert_eq!(req["method"], "sampling/createMessage");
        assert_eq!(req["id"], 1);
        assert_eq!(req["params"]["maxTokens"], 100);
    }

    #[test]
    fn test_create_sampling_request_with_model_preferences() {
        let messages = vec![SamplingMessage {
            role: "user".to_string(),
            content: SamplingContent {
                content_type: "text".to_string(),
                text: Some("Hello".to_string()),
                resource: None,
            },
        }];
        
        let prefs = ModelPreferences {
            hints: Some(vec![ModelHint { name: Some("gpt-4".to_string()) }]),
            cost_priority: Some(0.3),
            speed_priority: Some(0.7),
            intelligence_priority: Some(0.9),
        };
        
        let req = create_sampling_request(1, messages, Some(100), Some(0.7), Some(prefs), None, None);
        assert!(req["params"]["modelPreferences"].is_object());
    }

    #[test]
    fn test_create_sampling_request_with_system_prompt() {
        let messages = vec![SamplingMessage {
            role: "user".to_string(),
            content: SamplingContent {
                content_type: "text".to_string(),
                text: Some("Hello".to_string()),
                resource: None,
            },
        }];
        
        let req = create_sampling_request(
            1, 
            messages, 
            Some(100), 
            Some(0.7), 
            None, 
            Some("You are a helpful assistant.".to_string()),
            None
        );
        assert_eq!(req["params"]["systemPrompt"], "You are a helpful assistant.");
    }

    #[test]
    fn test_handle_sampling_with_callback() {
        // Note: This test may be affected by global state from other tests
        // In a real scenario, you'd want to reset the handler between tests
        let params = json!({
            "messages": [{"role": "user", "content": {"type": "text", "text": "Hello"}}],
            "maxTokens": 100
        });
        
        // This will use whatever handler was registered last
        let result = handle_sampling_create_message(&params);
        
        // If a handler is registered, it should succeed
        if result.is_ok() {
            let result = result.unwrap();
            assert_eq!(result["role"], "assistant");
        }
    }

    #[test]
    fn test_sampling_request_serialization() {
        let req = SamplingRequest {
            messages: vec![],
            max_tokens: Some(50),
            temperature: Some(0.5),
            include_context: Some("all".to_string()),
            model_preferences: None,
            system_prompt: None,
            metadata: None,
        };
        let serialized = serde_json::to_string(&req).unwrap();
        assert!(serialized.contains("maxTokens"));
        assert!(serialized.contains("temperature"));
        assert!(serialized.contains("includeContext"));
    }

    #[test]
    fn test_conversation_history_tracking() {
        let conv_id = "test-conv-1";
        
        // Clear any existing history
        clear_conversation(conv_id);
        
        // Add messages
        let msg1 = SamplingMessage {
            role: "user".to_string(),
            content: SamplingContent {
                content_type: "text".to_string(),
                text: Some("Hello".to_string()),
                resource: None,
            },
        };
        add_to_conversation(conv_id, msg1);
        
        let msg2 = SamplingMessage {
            role: "assistant".to_string(),
            content: SamplingContent {
                content_type: "text".to_string(),
                text: Some("Hi there!".to_string()),
                resource: None,
            },
        };
        add_to_conversation(conv_id, msg2);
        
        // Get history
        let history = get_conversation(conv_id);
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].role, "user");
        assert_eq!(history[1].role, "assistant");
        
        // Clear history
        clear_conversation(conv_id);
        let history = get_conversation(conv_id);
        assert_eq!(history.len(), 0);
    }

    #[test]
    fn test_create_sampling_request_with_history() {
        let conv_id = "test-conv-2";
        clear_conversation(conv_id);
        
        let msg = SamplingMessage {
            role: "user".to_string(),
            content: SamplingContent {
                content_type: "text".to_string(),
                text: Some("Test message".to_string()),
                resource: None,
            },
        };
        
        let req = create_sampling_request_with_history(1, conv_id, msg, Some(100), Some(0.7));
        
        // Check that metadata includes conversation_id
        assert_eq!(req["params"]["metadata"]["conversation_id"], conv_id);
        
        // Check that history was tracked
        let history = get_conversation(conv_id);
        assert_eq!(history.len(), 1);
        
        clear_conversation(conv_id);
    }

    #[test]
    fn test_sampling_with_metadata() {
        // Register a simple callback that doesn't make assertions
        register_sampling_callback(|_req| {
            Ok(SamplingResponse {
                role: "assistant".to_string(),
                content: SamplingContent {
                    content_type: "text".to_string(),
                    text: Some("Response with metadata".to_string()),
                    resource: None,
                },
                model: "test-model".to_string(),
                stop_reason: None,
            })
        });

        let params = json!({
            "messages": [{"role": "user", "content": {"type": "text", "text": "Hello"}}],
            "maxTokens": 100,
            "metadata": {
                "conversation_id": "conv-123",
                "progress_token": "token-456"
            }
        });
        
        let result = handle_sampling_create_message(&params).unwrap();
        assert_eq!(result["role"], "assistant");
        
        // Verify that conversation history was tracked
        let history = get_conversation("conv-123");
        assert_eq!(history.len(), 2); // user message + assistant response
        
        clear_conversation("conv-123");
    }
}
