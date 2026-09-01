//! VELOCITY-MCP Client SDK
//!
//! A type-safe Rust client for connecting to VELOCITY-MCP servers.
//!
//! # Example
//!
//! ```no_run
//! use velocity_mcp_client::{McpClient, StdioTransport};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Connect to server via stdio
//!     let transport = StdioTransport::new("velocity_mcp", &["--mode", "stdio"])?;
//!     let mut client = McpClient::new(transport);
//!     
//!     // Initialize connection
//!     client.initialize().await?;
//!     
//!     // List available tools
//!     let tools = client.list_tools().await?;
//!     println!("Available tools: {:?}", tools);
//!     
//!     // Call a tool
//!     let result = client.call_tool("file_read", serde_json::json!({
//!         "path": "/path/to/file.txt"
//!     })).await?;
//!     println!("Tool result: {:?}", result);
//!     
//!     Ok(())
//! }
//! ```

mod error;
mod transport;
mod types;
mod client;

pub use error::{Error, Result};
pub use transport::{Transport, StdioTransport, HttpTransport, ShmemTransport, JsonShmemTransport};
pub use types::*;
pub use client::McpClient;

/// Protocol version supported by this client
pub const PROTOCOL_VERSION: &str = "2024-11-05";

/// Client version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
