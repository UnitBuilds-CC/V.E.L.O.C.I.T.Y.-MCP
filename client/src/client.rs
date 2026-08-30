//! MCP client implementation

use crate::error::{Error, Result};
use crate::transport::Transport;
use crate::types::*;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

/// MCP client for communicating with VELOCITY-MCP servers
pub struct McpClient {
    transport: Arc<dyn Transport>,
    request_id: AtomicI64,
    initialized: bool,
}

impl McpClient {
    /// Create a new MCP client with the given transport
    pub fn new<T: Transport + 'static>(transport: T) -> Self {
        Self {
            transport: Arc::new(transport),
            request_id: AtomicI64::new(1),
            initialized: false,
        }
    }
    
    /// Get the next request ID
    fn next_id(&self) -> serde_json::Value {
        serde_json::Value::Number(serde_json::Number::from(self.request_id.fetch_add(1, Ordering::SeqCst)))
    }
    
    /// Send a JSON-RPC request
    async fn send_request(&self, method: &str, params: Option<serde_json::Value>) -> Result<serde_json::Value> {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params,
            id: Some(self.next_id()),
        };
        
        let response = self.transport.send(request).await?;
        
        if let Some(error) = response.error {
            return Err(Error::Protocol {
                code: error.code,
                message: error.message,
                data: error.data,
            });
        }
        
        response.result.ok_or(Error::Server("No result in response".to_string()))
    }
    
    /// Initialize the MCP connection
    pub async fn initialize(&mut self) -> Result<InitializeResult> {
        let params = InitializeParams {
            protocol_version: crate::PROTOCOL_VERSION.to_string(),
            capabilities: ClientCapabilities::default(),
            client_info: ClientInfo {
                name: "velocity-mcp-client".to_string(),
                version: crate::VERSION.to_string(),
            },
        };
        
        let result = self.send_request("initialize", Some(serde_json::to_value(params)?)).await?;
        let init_result: InitializeResult = serde_json::from_value(result)?;
        
        // Send initialized notification
        let notification = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "notifications/initialized".to_string(),
            params: None,
            id: None,
        };
        self.transport.send(notification).await?;
        
        self.initialized = true;
        Ok(init_result)
    }
    
    /// Check if the client is initialized
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }
    
    /// List available tools
    pub async fn list_tools(&self) -> Result<Vec<Tool>> {
        let result = self.send_request("tools/list", Some(serde_json::json!({}))).await?;
        let tools_result: ToolsListResult = serde_json::from_value(result)?;
        Ok(tools_result.tools)
    }
    
    /// Call a tool with the given arguments
    pub async fn call_tool(&self, name: &str, arguments: serde_json::Value) -> Result<ToolCallResult> {
        let params = serde_json::json!({
            "name": name,
            "arguments": arguments
        });
        
        let result = self.send_request("tools/call", Some(params)).await?;
        let tool_result: ToolCallResult = serde_json::from_value(result)?;
        
        if tool_result.is_error {
            let error_msg = tool_result.content.iter()
                .filter_map(|c| match c {
                    Content::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            return Err(Error::ToolExecution(error_msg));
        }
        
        Ok(tool_result)
    }
    
    /// List available resources
    pub async fn list_resources(&self) -> Result<Vec<Resource>> {
        let result = self.send_request("resources/list", Some(serde_json::json!({}))).await?;
        let resources_result: ResourcesListResult = serde_json::from_value(result)?;
        Ok(resources_result.resources)
    }
    
    /// Read a resource by URI
    pub async fn read_resource(&self, uri: &str) -> Result<Vec<ResourceContent>> {
        let params = serde_json::json!({
            "uri": uri
        });
        
        let result = self.send_request("resources/read", Some(params)).await?;
        let resource_result: ResourceReadResult = serde_json::from_value(result)?;
        Ok(resource_result.contents)
    }
    
    /// List available prompts
    pub async fn list_prompts(&self) -> Result<Vec<Prompt>> {
        let result = self.send_request("prompts/list", Some(serde_json::json!({}))).await?;
        let prompts_result: PromptsListResult = serde_json::from_value(result)?;
        Ok(prompts_result.prompts)
    }
    
    /// Get a prompt with the given arguments
    pub async fn get_prompt(&self, name: &str, arguments: serde_json::Value) -> Result<PromptGetResult> {
        let params = serde_json::json!({
            "name": name,
            "arguments": arguments
        });
        
        let result = self.send_request("prompts/get", Some(params)).await?;
        let prompt_result: PromptGetResult = serde_json::from_value(result)?;
        Ok(prompt_result)
    }
    
    /// Ping the server
    pub async fn ping(&self) -> Result<()> {
        self.send_request("ping", Some(serde_json::json!({}))).await?;
        Ok(())
    }
    
    /// Set the logging level
    pub async fn set_log_level(&self, level: &str) -> Result<()> {
        let params = serde_json::json!({
            "level": level
        });
        self.send_request("logging/setLevel", Some(params)).await?;
        Ok(())
    }
    
    /// Close the client connection
    pub async fn close(&self) -> Result<()> {
        self.transport.close().await
    }
}
