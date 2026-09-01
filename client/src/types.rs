//! MCP protocol types

use serde::{Deserialize, Serialize};

/// JSON-RPC request
#[derive(Debug, Serialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<serde_json::Value>,
}

/// JSON-RPC response
#[derive(Debug, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<serde_json::Value>,
}

/// JSON-RPC error
#[derive(Debug, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// Initialize request parameters
#[derive(Debug, Serialize)]
pub struct InitializeParams {
    pub protocol_version: String,
    pub capabilities: ClientCapabilities,
    pub client_info: ClientInfo,
}

/// Client capabilities
#[derive(Debug, Serialize, Default)]
pub struct ClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roots: Option<RootsCapability>,
}

/// Roots capability
#[derive(Debug, Serialize)]
pub struct RootsCapability {
    pub list_changed: bool,
}

/// Client information
#[derive(Debug, Serialize)]
pub struct ClientInfo {
    pub name: String,
    pub version: String,
}

/// Initialize response
#[derive(Debug, Deserialize)]
pub struct InitializeResult {
    #[serde(rename = "protocolVersion", alias = "protocol_version")]
    pub protocol_version: String,
    pub capabilities: ServerCapabilities,
    #[serde(rename = "serverInfo", alias = "server_info")]
    pub server_info: ServerInfo,
}

/// Server capabilities
#[derive(Debug, Deserialize)]
pub struct ServerCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<ToolsCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourcesCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompts: Option<PromptsCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sampling: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logging: Option<serde_json::Value>,
}

/// Tools capability
#[derive(Debug, Deserialize)]
pub struct ToolsCapability {
    #[serde(default, rename = "listChanged", alias = "list_changed")]
    pub list_changed: bool,
}

/// Resources capability
#[derive(Debug, Deserialize)]
pub struct ResourcesCapability {
    #[serde(default)]
    pub subscribe: bool,
    #[serde(default, rename = "listChanged", alias = "list_changed")]
    pub list_changed: bool,
}

/// Prompts capability
#[derive(Debug, Deserialize)]
pub struct PromptsCapability {
    #[serde(default, rename = "listChanged", alias = "list_changed")]
    pub list_changed: bool,
}

/// Server information
#[derive(Debug, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
}

/// Tool definition
#[derive(Debug, Deserialize, Clone)]
pub struct Tool {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema", alias = "input_schema")]
    pub input_schema: serde_json::Value,
}

/// Tools list result
#[derive(Debug, Deserialize)]
pub struct ToolsListResult {
    pub tools: Vec<Tool>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "nextCursor", alias = "next_cursor")]
    pub next_cursor: Option<String>,
}

/// Tool call result
#[derive(Debug, Deserialize)]
pub struct ToolCallResult {
    pub content: Vec<Content>,
    #[serde(default, rename = "isError", alias = "is_error")]
    pub is_error: bool,
}

/// Content block
#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum Content {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image { data: String, #[serde(rename = "mimeType", alias = "mime_type")] mime_type: String },
    #[serde(rename = "resource")]
    Resource { resource: ResourceContent },
}

/// Resource content
#[derive(Debug, Deserialize, Clone)]
pub struct ResourceContent {
    pub uri: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "mimeType", alias = "mime_type")]
    pub mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// Resource definition
#[derive(Debug, Deserialize, Clone)]
pub struct Resource {
    pub uri: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "mimeType", alias = "mime_type")]
    pub mime_type: Option<String>,
}

/// Resources list result
#[derive(Debug, Deserialize)]
pub struct ResourcesListResult {
    pub resources: Vec<Resource>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "nextCursor", alias = "next_cursor")]
    pub next_cursor: Option<String>,
}

/// Resource read result
#[derive(Debug, Deserialize)]
pub struct ResourceReadResult {
    pub contents: Vec<ResourceContent>,
}

/// Prompt definition
#[derive(Debug, Deserialize, Clone)]
pub struct Prompt {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Vec<PromptArgument>>,
}

/// Prompt argument
#[derive(Debug, Deserialize, Clone)]
pub struct PromptArgument {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub required: bool,
}

/// Prompts list result
#[derive(Debug, Deserialize)]
pub struct PromptsListResult {
    pub prompts: Vec<Prompt>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "nextCursor", alias = "next_cursor")]
    pub next_cursor: Option<String>,
}

/// Prompt get result
#[derive(Debug, Deserialize)]
pub struct PromptGetResult {
    pub description: String,
    pub messages: Vec<PromptMessage>,
}

/// Prompt message
#[derive(Debug, Deserialize)]
pub struct PromptMessage {
    pub role: String,
    pub content: Content,
}
