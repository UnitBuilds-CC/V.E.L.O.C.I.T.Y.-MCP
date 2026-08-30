# VELOCITY-MCP Client SDK

A type-safe Rust client for connecting to VELOCITY-MCP servers.

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
velocity-mcp-client = "3.0.0"
tokio = { version = "1", features = ["full"] }
```

## Quick Start

### Stdio Transport

Connect to a VELOCITY-MCP server via stdio:

```rust
use velocity_mcp_client::{McpClient, StdioTransport};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create transport by spawning the server process
    let transport = StdioTransport::new("velocity_mcp", &["--mode", "stdio"])?;
    let mut client = McpClient::new(transport);
    
    // Initialize the connection
    let init_result = client.initialize().await?;
    println!("Connected to: {} v{}", init_result.server_info.name, init_result.server_info.version);
    
    // List available tools
    let tools = client.list_tools().await?;
    println!("Available tools: {}", tools.len());
    for tool in tools {
        println!("  - {}: {}", tool.name, tool.description);
    }
    
    // Call a tool
    let result = client.call_tool("file_read", serde_json::json!({
        "path": "/path/to/file.txt"
    })).await?;
    
    // Extract text content from result
    for content in result.content {
        if let velocity_mcp_client::Content::Text { text } = content {
            println!("File contents:\n{}", text);
        }
    }
    
    // Close the connection
    client.close().await?;
    
    Ok(())
}
```

### HTTP Transport

Connect to a VELOCITY-MCP server via HTTP:

```rust
use velocity_mcp_client::{McpClient, HttpTransport};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create HTTP transport
    let transport = HttpTransport::new(
        "http://localhost:3000/mcp",
        Some("your-api-key".to_string())
    )?;
    let mut client = McpClient::new(transport);
    
    // Initialize and use the client
    client.initialize().await?;
    
    // List resources
    let resources = client.list_resources().await?;
    println!("Available resources: {}", resources.len());
    
    // Read a resource
    let contents = client.read_resource("file:///path/to/file.txt").await?;
    for content in contents {
        if let Some(text) = content.text {
            println!("Resource contents: {}", text);
        }
    }
    
    Ok(())
}
```

## API Reference

### McpClient

The main client struct for interacting with MCP servers.

#### Methods

- `new(transport)` - Create a new client with the given transport
- `initialize()` - Initialize the MCP connection
- `is_initialized()` - Check if the client is initialized
- `list_tools()` - List available tools
- `call_tool(name, arguments)` - Call a tool with arguments
- `list_resources()` - List available resources
- `read_resource(uri)` - Read a resource by URI
- `list_prompts()` - List available prompts
- `get_prompt(name, arguments)` - Get a prompt with arguments
- `ping()` - Ping the server
- `set_log_level(level)` - Set the logging level
- `close()` - Close the connection

### Transport Types

#### StdioTransport

Communicates with the server via stdin/stdout by spawning a process.

```rust
let transport = StdioTransport::new("velocity_mcp", &["--mode", "stdio"])?;
```

#### HttpTransport

Communicates with the server via HTTP POST requests.

```rust
let transport = HttpTransport::new("http://localhost:3000/mcp", Some("api-key".to_string()))?;
```

### Error Handling

The client returns `Result<T, Error>` for all operations. Common error types:

- `Error::Transport` - Connection or I/O errors
- `Error::Protocol` - JSON-RPC protocol errors
- `Error::Server` - Server returned an error
- `Error::ToolExecution` - Tool execution failed
- `Error::ResourceNotFound` - Resource not found
- `Error::Timeout` - Request timed out

```rust
match client.call_tool("file_read", serde_json::json!({"path": "/missing.txt"})).await {
    Ok(result) => println!("Success: {:?}", result),
    Err(Error::ToolExecution(msg)) => eprintln!("Tool failed: {}", msg),
    Err(e) => eprintln!("Error: {}", e),
}
```

## Examples

### File Operations

```rust
// Read a file
let result = client.call_tool("file_read", serde_json::json!({
    "path": "/path/to/file.txt"
})).await?;

// Write a file
let result = client.call_tool("file_write", serde_json::json!({
    "path": "/path/to/output.txt",
    "content": "Hello, World!"
})).await?;
```

### Shell Commands

```rust
// Execute a shell command
let result = client.call_tool("shell_exec", serde_json::json!({
    "command": "ls -la",
    "timeout": 30
})).await?;
```

### HTTP Requests

```rust
// Make an HTTP request
let result = client.call_tool("http_request", serde_json::json!({
    "url": "https://api.example.com/data",
    "method": "GET",
    "timeout": 60
})).await?;
```

### Working with Resources

```rust
// List all resources
let resources = client.list_resources().await?;

// Read a specific resource
let contents = client.read_resource("file:///path/to/file.txt").await?;

// Extract text content
for content in contents {
    if let Some(text) = content.text {
        println!("Content: {}", text);
    }
}
```

### Working with Prompts

```rust
// List all prompts
let prompts = client.list_prompts().await?;

// Get a prompt with arguments
let prompt = client.get_prompt("code-review", serde_json::json!({
    "code": "fn main() { println!(\"Hello\"); }"
})).await?;

// Use the prompt messages
for message in prompt.messages {
    println!("Role: {}", message.role);
    if let Content::Text { text } = message.content {
        println!("Content: {}", text);
    }
}
```

## Advanced Usage

### Custom Transport

Implement the `Transport` trait for custom communication:

```rust
use velocity_mcp_client::{Transport, JsonRpcRequest, JsonRpcResponse, Error};

struct MyCustomTransport {
    // Your transport implementation
}

#[async_trait::async_trait]
impl Transport for MyCustomTransport {
    async fn send(&self, request: JsonRpcRequest) -> Result<JsonRpcResponse, Error> {
        // Implement your custom send logic
        todo!()
    }
    
    async fn close(&self) -> Result<(), Error> {
        // Implement cleanup logic
        todo!()
    }
}
```

### Error Recovery

```rust
use velocity_mcp_client::Error;

async fn call_tool_with_retry(
    client: &McpClient,
    name: &str,
    args: serde_json::Value,
    max_retries: usize,
) -> Result<velocity_mcp_client::ToolCallResult, Error> {
    let mut last_error = None;
    
    for attempt in 0..max_retries {
        match client.call_tool(name, args.clone()).await {
            Ok(result) => return Ok(result),
            Err(Error::Transport(_)) | Err(Error::Timeout) => {
                // Retry on transport errors
                last_error = Some(err);
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
            Err(e) => return Err(e), // Don't retry other errors
        }
    }
    
    Err(last_error.unwrap())
}
```

## License

MIT OR Apache-2.0

## Links

- [VELOCITY-MCP Server](https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP)
- [MCP Protocol Specification](https://modelcontextprotocol.io/)
- [API Documentation](https://docs.rs/velocity-mcp-client)
