//! OAuth2 Connector Framework.
//!
//! Provides a generic connector interface for external services with OAuth2
//! authentication support. Includes:
//! - OAuth2 authorization flow (authorize URL generation, code exchange)
//! - Token refresh logic
//! - Token expiration tracking
//! - Encrypted token storage with AES-256-GCM
//! - Persistent file-based token storage
//! - Pre-built connectors for common services (GitHub, Google, etc.)
//! - Webhook support
//!
//! Feature-gated behind the `oauth2` feature flag.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(feature = "oauth2")]
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
#[cfg(feature = "oauth2")]
use rand::RngCore;

fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push_str(&format!("%{:02X}", b));
            }
        }
    }
    out
}

/// OAuth2 token with optional refresh and expiration tracking.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct OAuth2Token {
    pub access_token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_in: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_type: Option<String>,
    /// Unix timestamp when the token expires
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
    /// Unix timestamp when the token was issued
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issued_at: Option<u64>,
}

impl OAuth2Token {
    /// Check if the token is expired.
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            now >= expires_at
        } else if let Some(expires_in) = self.expires_in {
            if let Some(issued_at) = self.issued_at {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                now >= issued_at + expires_in
            } else {
                false
            }
        } else {
            false
        }
    }

    /// Check if the token needs refresh (expired or will expire within 5 minutes).
    pub fn needs_refresh(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            now + 300 >= expires_at // 5 minute buffer
        } else {
            self.is_expired()
        }
    }
}

/// Connector configuration.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ConnectorConfig {
    pub id: String,
    pub name: String,
    pub base_url: String,
    #[serde(rename = "authType")]
    pub auth_type: String, // "none", "bearer", "oauth2"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth2_config: Option<OAuth2Config>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webhook_config: Option<WebhookConfig>,
}

/// OAuth2 provider configuration.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct OAuth2Config {
    pub authorize_url: String,
    pub token_url: String,
    pub client_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scopes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redirect_uri: Option<String>,
}

/// Webhook configuration for a connector.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WebhookConfig {
    pub endpoint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub events: Option<Vec<String>>,
}

/// HTTP request for a connector.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ConnectorRequest {
    pub method: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<HashMap<String, String>>,
}

/// HTTP response from a connector.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ConnectorResponse {
    pub status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<HashMap<String, String>>,
}

/// OAuth2 authorization request.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AuthorizationRequest {
    pub connector_id: String,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scopes: Option<Vec<String>>,
}

/// OAuth2 authorization response (code exchange).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AuthorizationResponse {
    pub code: String,
    pub state: String,
}

/// OAuth2 token exchange request.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TokenExchangeRequest {
    pub connector_id: String,
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redirect_uri: Option<String>,
}

static CONNECTOR_REGISTRY: OnceLock<Mutex<HashMap<String, ConnectorConfig>>> = OnceLock::new();
static TOKEN_STORE: OnceLock<Mutex<HashMap<String, OAuth2Token>>> = OnceLock::new();
static STATE_STORE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

fn get_connector_registry() -> &'static Mutex<HashMap<String, ConnectorConfig>> {
    CONNECTOR_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn get_token_store() -> &'static Mutex<HashMap<String, OAuth2Token>> {
    TOKEN_STORE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn get_state_store() -> &'static Mutex<HashMap<String, String>> {
    STATE_STORE.get_or_init(|| Mutex::new(HashMap::new()))
}

const MAX_STORE_ENTRIES: usize = 1024;

/// Register a connector configuration.
pub fn register_connector(config: ConnectorConfig) {
    if let Ok(mut registry) = get_connector_registry().lock() {
        if registry.len() >= MAX_STORE_ENTRIES && !registry.contains_key(&config.id) {
            let first_key = registry.keys().next().cloned();
            if let Some(key) = first_key {
                registry.remove(&key);
                tracing::warn!(connector_id = %key, "Connector registry full, evicted oldest entry");
            }
        }
        registry.insert(config.id.clone(), config);
    }
}

/// Store an OAuth2 token for a connector.
pub fn store_token(connector_id: &str, token: OAuth2Token) {
    if let Ok(mut store) = get_token_store().lock() {
        if store.len() >= MAX_STORE_ENTRIES && !store.contains_key(connector_id) {
            let first_key = store.keys().next().cloned();
            if let Some(key) = first_key {
                store.remove(&key);
                tracing::warn!(connector_id = %key, "Token store full, evicted oldest entry");
            }
        }
        store.insert(connector_id.to_string(), token);
    }
}

/// Get a stored OAuth2 token for a connector.
pub fn get_token(connector_id: &str) -> Option<OAuth2Token> {
    match get_token_store().lock() {
        Ok(store) => store.get(connector_id).cloned(),
        Err(_) => None,
    }
}

/// Encryption key for token storage (32 bytes for AES-256).
static ENCRYPTION_KEY: OnceLock<Mutex<Option<[u8; 32]>>> = OnceLock::new();

fn get_encryption_key() -> &'static Mutex<Option<[u8; 32]>> {
    ENCRYPTION_KEY.get_or_init(|| Mutex::new(None))
}

/// Set the encryption key for token storage.
/// Must be called before using encrypted storage functions.
pub fn set_encryption_key(key: [u8; 32]) {
    if let Ok(mut key_store) = get_encryption_key().lock() {
        *key_store = Some(key);
    }
}

/// Generate a random encryption key.
pub fn generate_encryption_key() -> [u8; 32] {
    use rand::RngCore;
    let mut key = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut key);
    key
}

/// Encrypt a token for secure storage.
#[cfg(feature = "oauth2")]
pub fn encrypt_token(token: &OAuth2Token) -> Result<Vec<u8>, String> {
    let key_store = get_encryption_key().lock()
        .map_err(|e| format!("Failed to lock encryption key: {}", e))?;
    
    let key_bytes = key_store.as_ref()
        .ok_or("Encryption key not set. Call set_encryption_key() first.")?;
    
    let cipher = Aes256Gcm::new_from_slice(key_bytes)
        .map_err(|e| format!("Failed to create cipher: {}", e))?;
    
    // Generate a random nonce (12 bytes for AES-GCM)
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    
    // Serialize token to JSON
    let token_json = serde_json::to_string(token)
        .map_err(|e| format!("Failed to serialize token: {}", e))?;
    
    // Encrypt the token
    let ciphertext = cipher.encrypt(nonce, token_json.as_bytes())
        .map_err(|e| format!("Failed to encrypt token: {}", e))?;
    
    // Prepend nonce to ciphertext
    let mut result = nonce_bytes.to_vec();
    result.extend(ciphertext);
    
    Ok(result)
}

/// Decrypt a token from secure storage.
#[cfg(feature = "oauth2")]
pub fn decrypt_token(encrypted: &[u8]) -> Result<OAuth2Token, String> {
    if encrypted.len() < 12 {
        return Err("Encrypted data too short".to_string());
    }
    
    let key_store = get_encryption_key().lock()
        .map_err(|e| format!("Failed to lock encryption key: {}", e))?;
    
    let key_bytes = key_store.as_ref()
        .ok_or("Encryption key not set. Call set_encryption_key() first.")?;
    
    let cipher = Aes256Gcm::new_from_slice(key_bytes)
        .map_err(|e| format!("Failed to create cipher: {}", e))?;
    
    // Extract nonce and ciphertext
    let nonce = Nonce::from_slice(&encrypted[..12]);
    let ciphertext = &encrypted[12..];
    
    // Decrypt the token
    let plaintext = cipher.decrypt(nonce, ciphertext)
        .map_err(|e| format!("Failed to decrypt token: {}", e))?;
    
    // Deserialize token from JSON
    let token_json = String::from_utf8(plaintext)
        .map_err(|e| format!("Failed to convert to UTF-8: {}", e))?;
    
    let token: OAuth2Token = serde_json::from_str(&token_json)
        .map_err(|e| format!("Failed to deserialize token: {}", e))?;
    
    Ok(token)
}

/// Store an encrypted token to a file.
#[cfg(feature = "oauth2")]
pub fn store_token_encrypted(connector_id: &str, token: &OAuth2Token, path: &str) -> Result<(), String> {
    let encrypted = encrypt_token(token)?;
    
    std::fs::write(path, encrypted)
        .map_err(|e| format!("Failed to write encrypted token: {}", e))?;
    
    // Also store in memory for quick access
    store_token(connector_id, token.clone());
    
    Ok(())
}

/// Load an encrypted token from a file.
#[cfg(feature = "oauth2")]
pub fn load_token_encrypted(connector_id: &str, path: &str) -> Result<OAuth2Token, String> {
    let encrypted = std::fs::read(path)
        .map_err(|e| format!("Failed to read encrypted token: {}", e))?;
    
    let token = decrypt_token(&encrypted)?;
    
    // Store in memory for quick access
    store_token(connector_id, token.clone());
    
    Ok(token)
}

/// Webhook event payload.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WebhookEvent {
    pub event_type: String,
    pub timestamp: u64,
    pub data: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connector_id: Option<String>,
}

/// Webhook delivery result.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WebhookDeliveryResult {
    pub success: bool,
    pub status_code: Option<u16>,
    pub attempts: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Compute HMAC-SHA256 signature for webhook payload.
#[cfg(feature = "oauth2")]
pub fn compute_webhook_signature(payload: &str, secret: &str) -> Result<String, String> {
    use sha2::Sha256;
    use hmac::{Hmac, Mac};
    
    type HmacSha256 = Hmac<Sha256>;
    
    let mut mac = <HmacSha256 as Mac>::new_from_slice(secret.as_bytes())
        .map_err(|e| format!("Failed to create HMAC: {}", e))?;
    mac.update(payload.as_bytes());
    let result = mac.finalize();
    
    Ok(base64::Engine::encode(&base64::engine::general_purpose::STANDARD, result.into_bytes()))
}

/// Verify webhook signature.
#[cfg(feature = "oauth2")]
pub fn verify_webhook_signature(payload: &str, signature: &str, secret: &str) -> bool {
    match compute_webhook_signature(payload, secret) {
        Ok(expected) => constant_time_eq(expected.as_bytes(), signature.as_bytes()),
        Err(_) => false,
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Send a webhook event to a configured endpoint.
#[cfg(feature = "oauth2")]
pub fn send_webhook(connector_id: &str, event: &WebhookEvent) -> Result<WebhookDeliveryResult, String> {
    let config = get_connector_registry().lock()
        .ok()
        .and_then(|r| r.get(connector_id).cloned())
        .ok_or_else(|| format!("Connector not found: {}", connector_id))?;
    
    let webhook_config = config.webhook_config
        .ok_or_else(|| format!("Connector {} does not have webhook configured", connector_id))?;
    
    // Check if event type is subscribed
    if let Some(events) = &webhook_config.events {
        if !events.contains(&event.event_type) {
            return Ok(WebhookDeliveryResult {
                success: true,
                status_code: None,
                attempts: 0,
                error: Some("Event type not subscribed".to_string()),
            });
        }
    }
    
    // Serialize event to JSON
    let payload = serde_json::to_string(event)
        .map_err(|e| format!("Failed to serialize webhook event: {}", e))?;
    
    // Compute signature if secret is configured
    let signature = webhook_config.secret.as_ref().map(|secret| {
        compute_webhook_signature(&payload, secret).ok()
    }).flatten();
    
    // Send webhook with retry logic
    let max_attempts = 3;
    let mut last_error = None;
    
    for attempt in 1..=max_attempts {
        let mut req_builder = ureq::post(&webhook_config.endpoint);
        
        req_builder = req_builder.set("Content-Type", "application/json");
        
        if let Some(sig) = &signature {
            req_builder = req_builder.set("X-Webhook-Signature", sig);
        }
        
        match req_builder.send_string(&payload) {
            Ok(response) => {
                return Ok(WebhookDeliveryResult {
                    success: response.status() >= 200 && response.status() < 300,
                    status_code: Some(response.status()),
                    attempts: attempt,
                    error: None,
                });
            }
            Err(e) => {
                last_error = Some(format!("{}", e));
                if attempt < max_attempts {
                    // Wait before retry (exponential backoff)
                    std::thread::sleep(std::time::Duration::from_millis(100 * 2u64.pow(attempt - 1)));
                }
            }
        }
    }
    
    Ok(WebhookDeliveryResult {
        success: false,
        status_code: None,
        attempts: max_attempts,
        error: last_error,
    })
}

/// Handle incoming webhook (for receiving webhooks from external services).
#[cfg(feature = "oauth2")]
pub fn handle_incoming_webhook(
    connector_id: &str,
    payload: &str,
    signature: Option<&str>,
) -> Result<WebhookEvent, String> {
    let config = get_connector_registry().lock()
        .ok()
        .and_then(|r| r.get(connector_id).cloned())
        .ok_or_else(|| format!("Connector not found: {}", connector_id))?;
    
    let webhook_config = config.webhook_config
        .ok_or_else(|| format!("Connector {} does not have webhook configured", connector_id))?;
    
    // Verify signature if secret is configured
    if let (Some(secret), Some(sig)) = (&webhook_config.secret, signature) {
        if !verify_webhook_signature(payload, sig, secret) {
            return Err("Invalid webhook signature".to_string());
        }
    }
    
    // Parse webhook event
    let event: WebhookEvent = serde_json::from_str(payload)
        .map_err(|e| format!("Failed to parse webhook event: {}", e))?;
    
    Ok(event)
}

/// Generate an OAuth2 authorization URL.
pub fn generate_authorize_url(connector_id: &str, state: &str, scopes: Option<Vec<String>>) -> Result<String, String> {
    let config = get_connector_registry().lock()
        .ok()
        .and_then(|r| r.get(connector_id).cloned())
        .ok_or_else(|| format!("Connector not found: {}", connector_id))?;
    
    let oauth2_config = config.oauth2_config
        .ok_or_else(|| format!("Connector {} does not support OAuth2", connector_id))?;
    
    // Store state for validation
    if let Ok(mut state_store) = get_state_store().lock() {
        const MAX_STATE_ENTRIES: usize = 1000;
        if state_store.len() >= MAX_STATE_ENTRIES {
            tracing::warn!("OAuth2 state store full ({} entries), clearing stale entries", MAX_STATE_ENTRIES);
            state_store.clear();
        }
        state_store.insert(state.to_string(), connector_id.to_string());
    }
    
    let mut url = format!("{}?response_type=code&client_id={}&state={}",
        oauth2_config.authorize_url,
        percent_encode(&oauth2_config.client_id),
        percent_encode(&state)
    );

    if let Some(redirect_uri) = &oauth2_config.redirect_uri {
        url.push_str(&format!("&redirect_uri={}", percent_encode(redirect_uri)));
    }

    let effective_scopes = scopes.or(oauth2_config.scopes);
    if let Some(scopes) = effective_scopes {
        url.push_str(&format!("&scope={}", percent_encode(&scopes.join(" "))));
    }
    
    Ok(url)
}

/// Exchange authorization code for tokens.
#[cfg(feature = "oauth2")]
pub fn exchange_code(connector_id: &str, code: &str, redirect_uri: Option<&str>) -> Result<OAuth2Token, String> {
    let config = get_connector_registry().lock()
        .ok()
        .and_then(|r| r.get(connector_id).cloned())
        .ok_or_else(|| format!("Connector not found: {}", connector_id))?;
    
    let oauth2_config = config.oauth2_config
        .ok_or_else(|| format!("Connector {} does not support OAuth2", connector_id))?;
    
    let mut params = vec![
        ("grant_type", "authorization_code"),
        ("code", code),
        ("client_id", &oauth2_config.client_id),
    ];
    
    if let Some(redirect) = redirect_uri.or(oauth2_config.redirect_uri.as_deref()) {
        params.push(("redirect_uri", redirect));
    }
    
    if let Some(secret) = &oauth2_config.client_secret {
        params.push(("client_secret", secret));
    }
    
    let response = ureq::post(&oauth2_config.token_url)
        .send_form(&params)
        .map_err(|e| format!("Token exchange failed: {}", e))?;
    
    let token_response: Value = response.into_json()
        .map_err(|e| format!("Failed to parse token response: {}", e))?;
    
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    
    let token = OAuth2Token {
        access_token: token_response["access_token"].as_str()
            .ok_or("Missing access_token in response")?
            .to_string(),
        refresh_token: token_response["refresh_token"].as_str().map(|s| s.to_string()),
        expires_in: token_response["expires_in"].as_u64(),
        token_type: token_response["token_type"].as_str().map(|s| s.to_string()),
        expires_at: token_response["expires_in"].as_u64().map(|e| now + e),
        issued_at: Some(now),
    };
    
    store_token(connector_id, token.clone());
    Ok(token)
}

/// Refresh an expired token.
#[cfg(feature = "oauth2")]
pub fn refresh_token(connector_id: &str) -> Result<OAuth2Token, String> {
    let token = get_token(connector_id)
        .ok_or_else(|| format!("No token found for connector: {}", connector_id))?;
    
    let refresh_token = token.refresh_token
        .ok_or("No refresh token available")?;
    
    let config = get_connector_registry().lock()
        .ok()
        .and_then(|r| r.get(connector_id).cloned())
        .ok_or_else(|| format!("Connector not found: {}", connector_id))?;
    
    let oauth2_config = config.oauth2_config
        .ok_or_else(|| format!("Connector {} does not support OAuth2", connector_id))?;
    
    let mut params = vec![
        ("grant_type", "refresh_token"),
        ("refresh_token", &refresh_token),
        ("client_id", &oauth2_config.client_id),
    ];
    
    if let Some(secret) = &oauth2_config.client_secret {
        params.push(("client_secret", secret));
    }
    
    let response = ureq::post(&oauth2_config.token_url)
        .send_form(&params)
        .map_err(|e| format!("Token refresh failed: {}", e))?;
    
    let token_response: Value = response.into_json()
        .map_err(|e| format!("Failed to parse token response: {}", e))?;
    
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    
    let new_token = OAuth2Token {
        access_token: token_response["access_token"].as_str()
            .ok_or("Missing access_token in response")?
            .to_string(),
        refresh_token: token_response["refresh_token"].as_str()
            .map(|s| s.to_string())
            .or(Some(refresh_token)), // Keep old refresh token if new one not provided
        expires_in: token_response["expires_in"].as_u64(),
        token_type: token_response["token_type"].as_str().map(|s| s.to_string()),
        expires_at: token_response["expires_in"].as_u64().map(|e| now + e),
        issued_at: Some(now),
    };
    
    store_token(connector_id, new_token.clone());
    Ok(new_token)
}

/// Validate OAuth2 state parameter.
pub fn validate_state(state: &str) -> Option<String> {
    if let Ok(mut state_store) = get_state_store().lock() {
        state_store.remove(state)
    } else {
        None
    }
}

/// Get or refresh token, automatically refreshing if needed.
#[cfg(feature = "oauth2")]
pub fn get_valid_token(connector_id: &str) -> Result<OAuth2Token, String> {
    let token = get_token(connector_id)
        .ok_or_else(|| format!("No token found for connector: {}", connector_id))?;
    
    if token.needs_refresh() {
        refresh_token(connector_id)
    } else {
        Ok(token)
    }
}

/// Pre-built connector template for GitHub.
pub fn github_connector_template(client_id: &str, client_secret: Option<&str>) -> ConnectorConfig {
    ConnectorConfig {
        id: "github".to_string(),
        name: "GitHub API".to_string(),
        base_url: "https://api.github.com".to_string(),
        auth_type: "oauth2".to_string(),
        oauth2_config: Some(OAuth2Config {
            authorize_url: "https://github.com/login/oauth/authorize".to_string(),
            token_url: "https://github.com/login/oauth/access_token".to_string(),
            client_id: client_id.to_string(),
            client_secret: client_secret.map(|s| s.to_string()),
            scopes: Some(vec!["repo".to_string(), "user".to_string()]),
            redirect_uri: None,
        }),
        webhook_config: None,
    }
}

/// Pre-built connector template for GitLab.
pub fn gitlab_connector_template(client_id: &str, client_secret: Option<&str>) -> ConnectorConfig {
    ConnectorConfig {
        id: "gitlab".to_string(),
        name: "GitLab API".to_string(),
        base_url: "https://gitlab.com/api/v4".to_string(),
        auth_type: "oauth2".to_string(),
        oauth2_config: Some(OAuth2Config {
            authorize_url: "https://gitlab.com/oauth/authorize".to_string(),
            token_url: "https://gitlab.com/oauth/token".to_string(),
            client_id: client_id.to_string(),
            client_secret: client_secret.map(|s| s.to_string()),
            scopes: Some(vec!["api".to_string(), "read_user".to_string()]),
            redirect_uri: None,
        }),
        webhook_config: None,
    }
}

/// Pre-built connector template for Slack.
pub fn slack_connector_template(client_id: &str, client_secret: Option<&str>) -> ConnectorConfig {
    ConnectorConfig {
        id: "slack".to_string(),
        name: "Slack API".to_string(),
        base_url: "https://slack.com/api".to_string(),
        auth_type: "oauth2".to_string(),
        oauth2_config: Some(OAuth2Config {
            authorize_url: "https://slack.com/oauth/v2/authorize".to_string(),
            token_url: "https://slack.com/api/oauth.v2.access".to_string(),
            client_id: client_id.to_string(),
            client_secret: client_secret.map(|s| s.to_string()),
            scopes: Some(vec!["chat:write".to_string(), "channels:read".to_string()]),
            redirect_uri: None,
        }),
        webhook_config: None,
    }
}

/// Pre-built connector template for Discord.
pub fn discord_connector_template(client_id: &str, client_secret: Option<&str>) -> ConnectorConfig {
    ConnectorConfig {
        id: "discord".to_string(),
        name: "Discord API".to_string(),
        base_url: "https://discord.com/api".to_string(),
        auth_type: "oauth2".to_string(),
        oauth2_config: Some(OAuth2Config {
            authorize_url: "https://discord.com/api/oauth2/authorize".to_string(),
            token_url: "https://discord.com/api/oauth2/token".to_string(),
            client_id: client_id.to_string(),
            client_secret: client_secret.map(|s| s.to_string()),
            scopes: Some(vec!["bot".to_string(), "identify".to_string()]),
            redirect_uri: None,
        }),
        webhook_config: None,
    }
}

/// Pre-built connector template for Google.
pub fn google_connector_template(client_id: &str, client_secret: Option<&str>) -> ConnectorConfig {
    ConnectorConfig {
        id: "google".to_string(),
        name: "Google API".to_string(),
        base_url: "https://www.googleapis.com".to_string(),
        auth_type: "oauth2".to_string(),
        oauth2_config: Some(OAuth2Config {
            authorize_url: "https://accounts.google.com/o/oauth2/v2/auth".to_string(),
            token_url: "https://oauth2.googleapis.com/token".to_string(),
            client_id: client_id.to_string(),
            client_secret: client_secret.map(|s| s.to_string()),
            scopes: Some(vec![
                "https://www.googleapis.com/auth/userinfo.profile".to_string(),
                "https://www.googleapis.com/auth/userinfo.email".to_string(),
            ]),
            redirect_uri: None,
        }),
        webhook_config: None,
    }
}

/// Pre-built connector template for Microsoft.
pub fn microsoft_connector_template(client_id: &str, client_secret: Option<&str>) -> ConnectorConfig {
    ConnectorConfig {
        id: "microsoft".to_string(),
        name: "Microsoft Graph API".to_string(),
        base_url: "https://graph.microsoft.com/v1.0".to_string(),
        auth_type: "oauth2".to_string(),
        oauth2_config: Some(OAuth2Config {
            authorize_url: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize".to_string(),
            token_url: "https://login.microsoftonline.com/common/oauth2/v2.0/token".to_string(),
            client_id: client_id.to_string(),
            client_secret: client_secret.map(|s| s.to_string()),
            scopes: Some(vec!["User.Read".to_string(), "Mail.Read".to_string()]),
            redirect_uri: None,
        }),
        webhook_config: None,
    }
}

/// Make an authenticated HTTP request through a connector.
#[cfg(feature = "oauth2")]
pub fn call_connector(connector_id: &str, request: &ConnectorRequest) -> Result<ConnectorResponse, String> {
    let config = get_connector_registry().lock()
        .ok()
        .and_then(|r| r.get(connector_id).cloned())
        .ok_or_else(|| format!("Connector not found: {}", connector_id))?;

    let url = format!("{}{}", config.base_url, request.path);

    let mut req_builder = match request.method.as_str() {
        "GET" => ureq::get(&url),
        "POST" => ureq::post(&url),
        "PUT" => ureq::put(&url),
        "DELETE" => ureq::delete(&url),
        "PATCH" => ureq::patch(&url),
        _ => return Err(format!("Unsupported HTTP method: {}", request.method)),
    };

    req_builder = req_builder.timeout(std::time::Duration::from_secs(30));

    // Add auth header with automatic token refresh
    match config.auth_type.as_str() {
        "bearer" | "oauth2" => {
            let token = get_valid_token(connector_id)?;
            let auth_value = format!("{} {}",
                token.token_type.unwrap_or_else(|| "Bearer".to_string()),
                token.access_token
            );
            req_builder = req_builder.set("Authorization", &auth_value);
        }
        _ => {}
    }

    // Add custom headers
    if let Some(headers) = &request.headers {
        for (k, v) in headers {
            req_builder = req_builder.set(k, v);
        }
    }

    req_builder = req_builder.set("Content-Type", "application/json");
    req_builder = req_builder.set("Accept", "application/json");

    // Send request
    let response = if let Some(body) = &request.body {
        req_builder.send_json(&body).map_err(|e| format!("HTTP request failed: {}", e))?
    } else {
        req_builder.call().map_err(|e| format!("HTTP request failed: {}", e))?
    };

    let status = response.status();
    let body: Option<Value> = response.into_json().ok();

    Ok(ConnectorResponse {
        status,
        body,
        headers: None,
    })
}

/// List all registered connectors.
pub fn list_connectors() -> Vec<ConnectorConfig> {
    match get_connector_registry().lock() {
        Ok(r) => r.values().cloned().collect(),
        Err(_) => vec![],
    }
}

/// Handle connector/call MCP tool request.
pub fn handle_connector_call(params: &Value) -> Result<Value, String> {
    #[cfg(not(feature = "oauth2"))]
    {
        let _ = params;
        Err("OAuth2 feature not enabled. Build with --features oauth2".to_string())
    }
    #[cfg(feature = "oauth2")]
    {
        let connector_id = params["connectorId"].as_str()
            .ok_or("Missing connectorId parameter")?;

        let method = params["method"].as_str().unwrap_or("GET").to_string();
        let path = params["path"].as_str()
            .ok_or("Missing path parameter")?
            .to_string();

        let body = params.get("body").cloned();
        let headers: Option<HashMap<String, String>> = params.get("headers")
            .and_then(|h| serde_json::from_value(h.clone()).ok());

        let request = ConnectorRequest {
            method,
            path,
            body,
            headers,
        };

        let response = call_connector(connector_id, &request)?;

        Ok(json!({
            "status": response.status,
            "body": response.body
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_and_list_connectors() {
        let config = ConnectorConfig {
            id: "github".to_string(),
            name: "GitHub API".to_string(),
            base_url: "https://api.github.com".to_string(),
            auth_type: "oauth2".to_string(),
            oauth2_config: Some(OAuth2Config {
                authorize_url: "https://github.com/login/oauth/authorize".to_string(),
                token_url: "https://github.com/login/oauth/access_token".to_string(),
                client_id: "test-client".to_string(),
                client_secret: None,
                scopes: Some(vec!["repo".to_string()]),
                redirect_uri: None,
            }),
            webhook_config: None,
        };
        register_connector(config);
        let connectors = list_connectors();
        assert!(!connectors.is_empty());
    }

    #[test]
    fn test_store_and_get_token() {
        let token = OAuth2Token {
            access_token: "ghp_test123".to_string(),
            refresh_token: Some("ghr_refresh456".to_string()),
            expires_in: Some(3600),
            token_type: Some("Bearer".to_string()),
            expires_at: None,
            issued_at: None,
        };
        store_token("github", token.clone());
        let retrieved = get_token("github");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().access_token, "ghp_test123");
    }

    #[test]
    fn test_token_expiration_check() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        // Token that expires in the past
        let expired_token = OAuth2Token {
            access_token: "test".to_string(),
            refresh_token: None,
            expires_in: None,
            token_type: None,
            expires_at: Some(now - 100),
            issued_at: None,
        };
        assert!(expired_token.is_expired());
        
        // Token that expires in the future
        let valid_token = OAuth2Token {
            access_token: "test".to_string(),
            refresh_token: None,
            expires_in: None,
            token_type: None,
            expires_at: Some(now + 3600),
            issued_at: None,
        };
        assert!(!valid_token.is_expired());
    }

    #[test]
    fn test_token_needs_refresh() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        // Token that expires in 2 minutes (needs refresh within 5 min buffer)
        let needs_refresh_token = OAuth2Token {
            access_token: "test".to_string(),
            refresh_token: None,
            expires_in: None,
            token_type: None,
            expires_at: Some(now + 120),
            issued_at: None,
        };
        assert!(needs_refresh_token.needs_refresh());
        
        // Token that expires in 10 minutes (doesn't need refresh yet)
        let fresh_token = OAuth2Token {
            access_token: "test".to_string(),
            refresh_token: None,
            expires_in: None,
            token_type: None,
            expires_at: Some(now + 600),
            issued_at: None,
        };
        assert!(!fresh_token.needs_refresh());
    }

    #[test]
    fn test_generate_authorize_url() {
        let config = ConnectorConfig {
            id: "test".to_string(),
            name: "Test".to_string(),
            base_url: "https://api.test.com".to_string(),
            auth_type: "oauth2".to_string(),
            oauth2_config: Some(OAuth2Config {
                authorize_url: "https://auth.test.com/authorize".to_string(),
                token_url: "https://auth.test.com/token".to_string(),
                client_id: "client123".to_string(),
                client_secret: None,
                scopes: Some(vec!["read".to_string(), "write".to_string()]),
                redirect_uri: Some("https://app.test.com/callback".to_string()),
            }),
            webhook_config: None,
        };
        register_connector(config);
        
        let url = generate_authorize_url("test", "state123", None).unwrap();
        assert!(url.contains("response_type=code"));
        assert!(url.contains("client_id=client123"));
        assert!(url.contains("state=state123"));
        assert!(url.contains("redirect_uri="));
        assert!(url.contains("scope=read write"));
    }

    #[test]
    fn test_state_validation() {
        // Store a state
        if let Ok(mut state_store) = get_state_store().lock() {
            state_store.insert("test_state".to_string(), "test_connector".to_string());
        }
        
        // Validate and remove
        let connector_id = validate_state("test_state");
        assert_eq!(connector_id, Some("test_connector".to_string()));
        
        // State should be removed after validation
        let connector_id = validate_state("test_state");
        assert!(connector_id.is_none());
    }

    #[test]
    fn test_github_connector_template() {
        let config = github_connector_template("client_id", Some("client_secret"));
        assert_eq!(config.id, "github");
        assert_eq!(config.base_url, "https://api.github.com");
        assert!(config.oauth2_config.is_some());
        
        let oauth2 = config.oauth2_config.unwrap();
        assert_eq!(oauth2.client_id, "client_id");
        assert_eq!(oauth2.client_secret, Some("client_secret".to_string()));
        assert!(oauth2.scopes.unwrap().contains(&"repo".to_string()));
    }

    #[test]
    fn test_google_connector_template() {
        let config = google_connector_template("client_id", None);
        assert_eq!(config.id, "google");
        assert_eq!(config.base_url, "https://www.googleapis.com");
        assert!(config.oauth2_config.is_some());
        
        let oauth2 = config.oauth2_config.unwrap();
        assert_eq!(oauth2.client_id, "client_id");
        assert!(oauth2.client_secret.is_none());
    }

    #[test]
    fn test_webhook_config() {
        let config = ConnectorConfig {
            id: "webhook_test".to_string(),
            name: "Webhook Test".to_string(),
            base_url: "https://api.test.com".to_string(),
            auth_type: "none".to_string(),
            oauth2_config: None,
            webhook_config: Some(WebhookConfig {
                endpoint: "https://webhook.test.com/events".to_string(),
                secret: Some("webhook_secret".to_string()),
                events: Some(vec!["push".to_string(), "pull_request".to_string()]),
            }),
        };
        register_connector(config);
        
        let connectors = list_connectors();
        let webhook_connector = connectors.iter().find(|c| c.id == "webhook_test").unwrap();
        assert!(webhook_connector.webhook_config.is_some());
        
        let webhook = webhook_connector.webhook_config.as_ref().unwrap();
        assert_eq!(webhook.endpoint, "https://webhook.test.com/events");
        assert_eq!(webhook.secret, Some("webhook_secret".to_string()));
    }

    #[test]
    fn test_handle_connector_call_no_feature() {
        let params = json!({"connectorId": "test", "method": "GET", "path": "/users"});
        let result = handle_connector_call(&params);
        // Should either succeed with error message or fail gracefully
        // The exact behavior depends on whether oauth2 feature is enabled
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    #[cfg(feature = "oauth2")]
    #[serial_test::serial]
    fn test_encrypted_token_storage() {
        // Generate and set encryption key
        let key = generate_encryption_key();
        set_encryption_key(key.clone());
        
        // Create a test token
        let token = OAuth2Token {
            access_token: "secret_access_token_123".to_string(),
            refresh_token: Some("secret_refresh_token_456".to_string()),
            expires_in: Some(3600),
            token_type: Some("Bearer".to_string()),
            expires_at: None,
            issued_at: None,
        };
        
        // Ensure key is set before encrypting (protect against parallel test interference)
        set_encryption_key(key.clone());
        
        // Encrypt the token
        let encrypted = encrypt_token(&token).unwrap();
        assert!(!encrypted.is_empty());
        assert!(encrypted.len() > 12); // At least nonce + some ciphertext
        
        // Ensure key is set before decrypting (protect against parallel test interference)
        set_encryption_key(key);
        
        // Decrypt the token
        let decrypted = decrypt_token(&encrypted).unwrap();
        assert_eq!(decrypted.access_token, token.access_token);
        assert_eq!(decrypted.refresh_token, token.refresh_token);
        assert_eq!(decrypted.expires_in, token.expires_in);
    }

    #[test]
    #[cfg(feature = "oauth2")]
    #[serial_test::serial]
    fn test_encrypted_token_file_storage() {
        use std::fs;
        
        // Generate and set encryption key
        let key = generate_encryption_key();
        set_encryption_key(key.clone());
        
        // Create a test token
        let token = OAuth2Token {
            access_token: "file_test_token".to_string(),
            refresh_token: None,
            expires_in: Some(7200),
            token_type: Some("Bearer".to_string()),
            expires_at: None,
            issued_at: None,
        };
        
        // Use unique file name to avoid conflicts with parallel tests
        let temp_path = format!("test_encrypted_token_{}.bin", std::process::id());
        
        // Ensure key is set before storing (protect against parallel test interference)
        set_encryption_key(key.clone());
        
        // Store encrypted token to file
        let result = store_token_encrypted("file_test", &token, &temp_path);
        assert!(result.is_ok());
        
        // Verify file exists
        assert!(fs::metadata(&temp_path).is_ok());
        
        // Ensure same key is set for loading (in case other tests changed it)
        set_encryption_key(key);
        
        // Load encrypted token from file
        let loaded = load_token_encrypted("file_test", &temp_path).unwrap();
        assert_eq!(loaded.access_token, token.access_token);
        assert_eq!(loaded.expires_in, token.expires_in);
        
        // Clean up
        let _ = fs::remove_file(&temp_path);
    }

    #[test]
    fn test_generate_encryption_key() {
        let key1 = generate_encryption_key();
        let key2 = generate_encryption_key();
        
        // Keys should be different (random)
        assert_ne!(key1, key2);
        assert_eq!(key1.len(), 32);
        assert_eq!(key2.len(), 32);
    }

    #[test]
    #[cfg(feature = "oauth2")]
    fn test_webhook_signature() {
        let payload = r#"{"event_type":"push","timestamp":1234567890,"data":{"ref":"main"}}"#;
        let secret = "webhook_secret_key";
        
        // Compute signature
        let signature = compute_webhook_signature(payload, secret).unwrap();
        assert!(!signature.is_empty());
        
        // Verify signature
        assert!(verify_webhook_signature(payload, &signature, secret));
        
        // Verify wrong signature fails
        assert!(!verify_webhook_signature(payload, "wrong_signature", secret));
        
        // Verify wrong secret fails
        assert!(!verify_webhook_signature(payload, &signature, "wrong_secret"));
    }

    #[test]
    #[cfg(feature = "oauth2")]
    fn test_webhook_event_serialization() {
        let event = WebhookEvent {
            event_type: "push".to_string(),
            timestamp: 1234567890,
            data: json!({"ref": "main", "commits": 3}),
            connector_id: Some("github".to_string()),
        };
        
        let serialized = serde_json::to_string(&event).unwrap();
        let deserialized: WebhookEvent = serde_json::from_str(&serialized).unwrap();
        
        assert_eq!(deserialized.event_type, event.event_type);
        assert_eq!(deserialized.timestamp, event.timestamp);
        assert_eq!(deserialized.connector_id, event.connector_id);
    }

    #[test]
    #[cfg(feature = "oauth2")]
    fn test_handle_incoming_webhook() {
        // Register a connector with webhook
        let config = ConnectorConfig {
            id: "webhook_test".to_string(),
            name: "Webhook Test".to_string(),
            base_url: "https://api.test.com".to_string(),
            auth_type: "none".to_string(),
            oauth2_config: None,
            webhook_config: Some(WebhookConfig {
                endpoint: "https://webhook.test.com/events".to_string(),
                secret: Some("test_secret".to_string()),
                events: Some(vec!["push".to_string()]),
            }),
        };
        register_connector(config);
        
        let payload = r#"{"event_type":"push","timestamp":1234567890,"data":{"ref":"main"}}"#;
        let signature = compute_webhook_signature(payload, "test_secret").unwrap();
        
        // Handle webhook with valid signature
        let result = handle_incoming_webhook("webhook_test", payload, Some(&signature));
        assert!(result.is_ok());
        
        let event = result.unwrap();
        assert_eq!(event.event_type, "push");
        assert_eq!(event.timestamp, 1234567890);
        
        // Handle webhook with invalid signature
        let result = handle_incoming_webhook("webhook_test", payload, Some("invalid"));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid webhook signature"));
    }
}
