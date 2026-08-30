//! Error types for the MCP client

use thiserror::Error;

/// Result type for MCP client operations
pub type Result<T> = std::result::Result<T, Error>;

/// Errors that can occur when using the MCP client
#[derive(Error, Debug)]
pub enum Error {
    /// Transport-level error (connection, I/O, etc.)
    #[error("Transport error: {0}")]
    Transport(String),

    /// JSON-RPC protocol error
    #[error("Protocol error: {code} - {message}")]
    Protocol {
        code: i32,
        message: String,
        data: Option<serde_json::Value>,
    },

    /// JSON serialization/deserialization error
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Server returned an error response
    #[error("Server error: {0}")]
    Server(String),

    /// Invalid configuration or parameters
    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),

    /// Connection was closed unexpectedly
    #[error("Connection closed")]
    ConnectionClosed,

    /// Request timed out
    #[error("Request timed out")]
    Timeout,

    /// Tool execution failed
    #[error("Tool execution failed: {0}")]
    ToolExecution(String),

    /// Resource not found
    #[error("Resource not found: {0}")]
    ResourceNotFound(String),

    /// Prompt not found
    #[error("Prompt not found: {0}")]
    PromptNotFound(String),

    /// Internal client error
    #[error("Internal error: {0}")]
    Internal(String),
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::Transport(err.to_string())
    }
}

impl From<reqwest::Error> for Error {
    fn from(err: reqwest::Error) -> Self {
        Error::Transport(err.to_string())
    }
}
