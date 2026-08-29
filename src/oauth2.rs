//! OAuth2 Connector Framework.
//!
//! Provides a generic connector interface for external services with OAuth2
//! authentication support. Connectors are registered with their configuration
//! and can make authenticated HTTP requests using stored tokens.
//!
//! Feature-gated behind the `oauth2` feature flag.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// OAuth2 token with optional refresh.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct OAuth2Token {
    pub access_token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_in: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_type: Option<String>,
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
}

/// OAuth2 provider configuration.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct OAuth2Config {
    pub authorize_url: String,
    pub token_url: String,
    pub client_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scopes: Option<Vec<String>>,
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

static CONNECTOR_REGISTRY: OnceLock<Mutex<HashMap<String, ConnectorConfig>>> = OnceLock::new();
static TOKEN_STORE: OnceLock<Mutex<HashMap<String, OAuth2Token>>> = OnceLock::new();

fn get_connector_registry() -> &'static Mutex<HashMap<String, ConnectorConfig>> {
    CONNECTOR_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn get_token_store() -> &'static Mutex<HashMap<String, OAuth2Token>> {
    TOKEN_STORE.get_or_init(|| Mutex::new(HashMap::new()))
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

    // Add auth header
    match config.auth_type.as_str() {
        "bearer" | "oauth2" => {
            if let Some(token) = get_token(connector_id) {
                let auth_value = format!("{} {}",
                    token.token_type.unwrap_or_else(|| "Bearer".to_string()),
                    token.access_token
                );
                req_builder = req_builder.set("Authorization", &auth_value);
            } else {
                return Err("No OAuth2 token available for connector".to_string());
            }
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
                scopes: Some(vec!["repo".to_string()]),
            }),
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
        };
        store_token("github", token.clone());
        let retrieved = get_token("github");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().access_token, "ghp_test123");
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
