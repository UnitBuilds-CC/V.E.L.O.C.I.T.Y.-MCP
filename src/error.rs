//! Comprehensive error handling for VELOCITY-MCP server.
//!
//! Provides custom error types for each module with detailed error messages,
//! error codes, and recovery strategies.

use thiserror::Error;

/// Main error type for the VELOCITY-MCP server.
#[derive(Error, Debug)]
pub enum VelocityError {
    /// Protocol-related errors.
    #[error("Protocol error: {message}")]
    Protocol { message: String, code: i32 },
    
    /// Registry-related errors.
    #[error("Registry error: {message}")]
    Registry { message: String },
    
    /// Resource-related errors.
    #[error("Resource error: {message}")]
    Resource { message: String },
    
    /// Configuration-related errors.
    #[error("Configuration error: {message}")]
    Config { message: String },
    
    /// Authentication/authorization errors.
    #[error("Authentication error: {message}")]
    Auth { message: String },
    
    /// Rate limiting errors.
    #[error("Rate limit exceeded: {message}")]
    RateLimit { message: String },
    
    /// Database errors.
    #[cfg(feature = "database")]
    #[error("Database error: {message}")]
    Database { message: String },
    
    /// OAuth2 errors.
    #[cfg(feature = "oauth2")]
    #[error("OAuth2 error: {message}")]
    OAuth2 { message: String },
    
    /// IO errors.
    #[error("IO error: {source}")]
    Io { #[from] source: std::io::Error },
    
    /// JSON serialization/deserialization errors.
    #[error("JSON error: {source}")]
    Json { #[from] source: serde_json::Error },
    
    /// Generic internal errors.
    #[error("Internal error: {message}")]
    Internal { message: String },
}

impl VelocityError {
    /// Create a protocol error.
    pub fn protocol(message: impl Into<String>, code: i32) -> Self {
        Self::Protocol {
            message: message.into(),
            code,
        }
    }
    
    /// Create a registry error.
    pub fn registry(message: impl Into<String>) -> Self {
        Self::Registry {
            message: message.into(),
        }
    }
    
    /// Create a resource error.
    pub fn resource(message: impl Into<String>) -> Self {
        Self::Resource {
            message: message.into(),
        }
    }
    
    /// Create a configuration error.
    pub fn config(message: impl Into<String>) -> Self {
        Self::Config {
            message: message.into(),
        }
    }
    
    /// Create an authentication error.
    pub fn auth(message: impl Into<String>) -> Self {
        Self::Auth {
            message: message.into(),
        }
    }
    
    /// Create a rate limit error.
    pub fn rate_limit(message: impl Into<String>) -> Self {
        Self::RateLimit {
            message: message.into(),
        }
    }
    
    /// Create an internal error.
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal {
            message: message.into(),
        }
    }
    
    /// Convert to JSON-RPC error response.
    pub fn to_json_rpc_error(&self, id: serde_json::Value) -> serde_json::Value {
        let (code, message) = match self {
            Self::Protocol { message, code } => (*code, message.clone()),
            Self::Registry { message } => (-32000, message.clone()),
            Self::Resource { message } => (-32000, message.clone()),
            Self::Config { message } => (-32000, message.clone()),
            Self::Auth { message } => (-32000, message.clone()),
            Self::RateLimit { message } => (-32000, message.clone()),
            #[cfg(feature = "database")]
            Self::Database { message } => (-32000, message.clone()),
            #[cfg(feature = "oauth2")]
            Self::OAuth2 { message } => (-32000, message.clone()),
            Self::Io { source } => (-32000, source.to_string()),
            Self::Json { source } => (-32700, source.to_string()),
            Self::Internal { message } => (-32603, message.clone()),
        };
        
        serde_json::json!({
            "jsonrpc": "2.0",
            "error": {
                "code": code,
                "message": message
            },
            "id": id
        })
    }
}

/// Result type alias for VELOCITY-MCP operations.
pub type VelocityResult<T> = Result<T, VelocityError>;

/// Error context for better error messages.
#[derive(Debug, Clone)]
pub struct ErrorContext {
    /// The module where the error occurred.
    pub module: String,
    /// The operation that failed.
    pub operation: String,
    /// Additional context information.
    pub context: std::collections::HashMap<String, String>,
}

impl ErrorContext {
    /// Create a new error context.
    pub fn new(module: impl Into<String>, operation: impl Into<String>) -> Self {
        Self {
            module: module.into(),
            operation: operation.into(),
            context: std::collections::HashMap::new(),
        }
    }
    
    /// Add context information.
    pub fn with_context(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.context.insert(key.into(), value.into());
        self
    }
    
    /// Format the context as a string.
    pub fn format(&self) -> String {
        let mut parts = vec![
            format!("module={}", self.module),
            format!("operation={}", self.operation),
        ];
        
        for (key, value) in &self.context {
            parts.push(format!("{}={}", key, value));
        }
        
        parts.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_error_creation() {
        let err = VelocityError::protocol("Invalid method", -32601);
        assert!(err.to_string().contains("Invalid method"));
        
        let err = VelocityError::registry("Tool not found");
        assert!(err.to_string().contains("Tool not found"));
        
        let err = VelocityError::auth("Invalid API key");
        assert!(err.to_string().contains("Invalid API key"));
    }
    
    #[test]
    fn test_error_context() {
        let ctx = ErrorContext::new("http", "handle_request")
            .with_context("method", "tools/call")
            .with_context("tool", "read_file");
        
        let formatted = ctx.format();
        assert!(formatted.contains("module=http"));
        assert!(formatted.contains("operation=handle_request"));
        assert!(formatted.contains("method=tools/call"));
        assert!(formatted.contains("tool=read_file"));
    }
    
    #[test]
    fn test_json_rpc_error() {
        let err = VelocityError::protocol("Method not found", -32601);
        let json_err = err.to_json_rpc_error(serde_json::json!(1));
        
        assert_eq!(json_err["error"]["code"], -32601);
        assert!(json_err["error"]["message"].as_str().unwrap().contains("Method not found"));
        assert_eq!(json_err["id"], 1);
    }
}
