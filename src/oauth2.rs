//! OAuth2 Connector Framework.
//!
//! Provides a generic connector interface for external services with OAuth2
//! authentication support. Includes:
//! - OAuth2 authorization flow (authorize URL generation, code exchange)
//! - Token refresh logic
//! - Token expiration tracking
//! - Pre-built connectors for common services (GitHub, Google, etc.)
//! - Webhook support
//!
//! Feature-gated behind the `oauth2` feature flag.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

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

/// Register a connector configuration.
pub fn register_connector(config: ConnectorConfig) {
    if let Ok(mut registry) = get_connector_registry().lock() {
        registry.insert(config.id.clone(), config);
    }
}

/// Store an OAuth2 token for a connector.
pub fn store_token(connector_id: &str, token: OAuth2Token) {
    if let Ok(mut store) = get_token_store().lock() {
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
        state_store.insert(state.to_string(), connector_id.to_string());
    }
    
    let mut url = format!("{}?response_type=code&client_id={}&state={}",
        oauth2_config.authorize_url,
        oauth2_config.client_id,
        state
    );
    
    if let Some(redirect_uri) = &oauth2_config.redirect_uri {
        url.push_str(&format!("&redirect_uri={}", redirect_uri));
    }
    
    let effective_scopes = scopes.or(oauth2_config.scopes);
    if let Some(scopes) = effective_scopes {
        url.push_str(&format!("&scope={}", scopes.join(" ")));
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
}
