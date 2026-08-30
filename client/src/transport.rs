//! Transport implementations for MCP client

use crate::error::{Error, Result};
use crate::types::{JsonRpcRequest, JsonRpcResponse};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

/// Transport trait for MCP communication
#[async_trait::async_trait]
pub trait Transport: Send + Sync {
    /// Send a JSON-RPC request and receive a response
    async fn send(&self, request: JsonRpcRequest) -> Result<JsonRpcResponse>;
    
    /// Close the transport connection
    async fn close(&self) -> Result<()>;
}

/// Stdio transport - communicates via stdin/stdout
pub struct StdioTransport {
    child: Mutex<Child>,
}

impl StdioTransport {
    /// Create a new stdio transport by spawning a process
    pub fn new(command: &str, args: &[&str]) -> Result<Self> {
        let child = Command::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        
        Ok(Self {
            child: Mutex::new(child),
        })
    }
}

#[async_trait::async_trait]
impl Transport for StdioTransport {
    async fn send(&self, request: JsonRpcRequest) -> Result<JsonRpcResponse> {
        let mut child = self.child.lock().await;
        
        // Take stdin and stdout to avoid borrow conflicts
        let mut stdin = child.stdin.take().ok_or(Error::ConnectionClosed)?;
        let mut stdout = child.stdout.take().ok_or(Error::ConnectionClosed)?;
        
        // Serialize and send request
        let request_json = serde_json::to_string(&request)?;
        stdin.write_all(request_json.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await?;
        
        // Read response
        let mut reader = BufReader::new(&mut stdout);
        let mut response_line = String::new();
        reader.read_line(&mut response_line).await?;
        
        if response_line.is_empty() {
            return Err(Error::ConnectionClosed);
        }
        
        let response: JsonRpcResponse = serde_json::from_str(&response_line)?;
        
        // Put stdin and stdout back
        child.stdin = Some(stdin);
        child.stdout = Some(stdout);
        
        Ok(response)
    }
    
    async fn close(&self) -> Result<()> {
        let mut child = self.child.lock().await;
        child.kill().await?;
        Ok(())
    }
}

/// HTTP transport - communicates via HTTP/SSE
pub struct HttpTransport {
    client: reqwest::Client,
    url: String,
    api_key: Option<String>,
}

impl HttpTransport {
    /// Create a new HTTP transport
    pub fn new(url: &str, api_key: Option<String>) -> Result<Self> {
        let client = reqwest::Client::new();
        Ok(Self {
            client,
            url: url.to_string(),
            api_key,
        })
    }
}

#[async_trait::async_trait]
impl Transport for HttpTransport {
    async fn send(&self, request: JsonRpcRequest) -> Result<JsonRpcResponse> {
        let mut req_builder = self.client.post(&self.url).json(&request);
        
        if let Some(api_key) = &self.api_key {
            req_builder = req_builder.header("Authorization", format!("Bearer {}", api_key));
        }
        
        let response = req_builder.send().await?;
        
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(Error::Server(format!("HTTP {}: {}", status, error_text)));
        }
        
        let rpc_response: JsonRpcResponse = response.json().await?;
        Ok(rpc_response)
    }
    
    async fn close(&self) -> Result<()> {
        // HTTP transport doesn't need explicit close
        Ok(())
    }
}
