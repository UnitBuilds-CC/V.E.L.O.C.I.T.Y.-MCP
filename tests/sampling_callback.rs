//! Sampling callback-path tests.
//!
//! These live in their own test binary on purpose. `register_sampling_callback` writes a
//! process-global slot, and the lib test binary asserts that the slot is *empty*
//! (`protocol::json_rpc::tests::test_sampling_create_message_no_handler_returns_error` and
//! `protocol::nmcp_binary::tests::test_dispatch_sampling_create_no_handler`). Holding the
//! callback path in a separate process means no lib test can ever observe a registered
//! callback, so those two assertions are structural rather than ordering-dependent.

use serde_json::json;
use std::sync::Mutex;
use velocity_mcp::sampling::{
    clear_conversation, get_conversation, handle_sampling_create_message,
    register_sampling_callback, SamplingContent, SamplingRequest, SamplingResponse,
};

/// Serialises the tests that install a callback into the process-global slot.
static HANDLER_LOCK: Mutex<()> = Mutex::new(());

/// The request object the most recent callback was handed, so a test can assert on the
/// parsed fields rather than on re-serialised JSON.
static CAPTURED: Mutex<Option<SamplingRequest>> = Mutex::new(None);

fn text_response(text: &str) -> SamplingResponse {
    SamplingResponse {
        role: "assistant".to_string(),
        content: SamplingContent {
            content_type: "text".to_string(),
            text: Some(text.to_string()),
            resource: None,
        },
        model: "test-model".to_string(),
        stop_reason: Some("end_turn".to_string()),
    }
}

fn minimal_params(extra: serde_json::Value) -> serde_json::Value {
    let mut params = json!({
        "messages": [{"role": "user", "content": {"type": "text", "text": "Hello"}}],
        "maxTokens": 100
    });
    if let Some(map) = extra.as_object() {
        for (k, v) in map {
            params[k] = v.clone();
        }
    }
    params
}

#[test]
fn callback_dispatch_tracks_conversation_history() {
    let _guard = HANDLER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    register_sampling_callback(|_| Ok(text_response("Response with metadata")));

    let params = minimal_params(json!({"metadata": {"conversation_id": "conv-541"}}));
    let result = handle_sampling_create_message(&params).unwrap();
    assert_eq!(result["role"], "assistant");
    assert_eq!(result["content"]["text"], "Response with metadata");
    assert_eq!(result["stopReason"], "end_turn");

    // The user message and the assistant reply are both recorded for the conversation.
    let history = get_conversation("conv-541");
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].role, "user");
    assert_eq!(history[1].role, "assistant");

    clear_conversation("conv-541");
}

/// Re-registering must install the new callback: the slot used to be filled with
/// `OnceLock::get_or_init`, so every registration after the first was silently ignored.
#[test]
fn re_registration_replaces_the_previous_callback() {
    let _guard = HANDLER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    register_sampling_callback(|_| Ok(text_response("first")));
    register_sampling_callback(|_| Ok(text_response("second")));

    let result = handle_sampling_create_message(&minimal_params(json!({}))).unwrap();
    assert_eq!(
        result["content"]["text"], "second",
        "the second registration was silently dropped"
    );
}

/// The spec spells these fields `costPriority` / `speedPriority` / `intelligencePriority`
/// and `progressToken` -- the same camelCase rename `src/streaming.rs` applies to
/// `progressToken`.
#[test]
fn spec_spelled_model_preferences_and_metadata_reach_the_callback() {
    let _guard = HANDLER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    register_sampling_callback(|req| {
        *CAPTURED.lock().unwrap_or_else(|e| e.into_inner()) = Some(req.clone());
        Ok(text_response("captured"))
    });

    let params = minimal_params(json!({
        "maxTokens": 250,
        "systemPrompt": "Be brief.",
        "modelPreferences": {
            "costPriority": 0.25,
            "speedPriority": 0.75,
            "intelligencePriority": 0.5,
            "hints": [{"name": "claude"}]
        },
        "metadata": {"progressToken": "pt-541", "conversation_id": "conv-pref-541"}
    }));
    handle_sampling_create_message(&params).unwrap();

    let seen = CAPTURED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .expect("callback did not run");

    assert_eq!(seen.max_tokens, Some(250));
    assert_eq!(seen.system_prompt.as_deref(), Some("Be brief."));

    let prefs = seen
        .model_preferences
        .expect("modelPreferences parsed as None");
    assert_eq!(
        prefs.cost_priority,
        Some(0.25),
        "costPriority dropped by the parser"
    );
    assert_eq!(prefs.speed_priority, Some(0.75));
    assert_eq!(prefs.intelligence_priority, Some(0.5));
    assert_eq!(
        prefs
            .hints
            .as_ref()
            .and_then(|h| h.first())
            .and_then(|h| h.name.as_deref()),
        Some("claude")
    );

    let meta = seen.metadata.expect("metadata parsed as None");
    assert_eq!(
        meta.progress_token.as_ref().and_then(|v| v.as_str()),
        Some("pt-541"),
        "progressToken dropped by the parser"
    );
    assert_eq!(meta.conversation_id.as_deref(), Some("conv-pref-541"));

    clear_conversation("conv-pref-541");
}
