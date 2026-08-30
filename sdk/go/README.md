# VELOCITY-MCP Go Client SDK

A type-safe Go client for connecting to VELOCITY-MCP servers.

## Installation

```bash
go get github.com/UnitBuilds-CC/velocity-mcp/sdk/go
```

## Quick Start

### HTTP Transport

```go
package main

import (
    "context"
    "fmt"
    "time"
    
    velocity_mcp "github.com/UnitBuilds-CC/velocity-mcp/sdk/go"
)

func main() {
    // Create HTTP transport
    transport := velocity_mcp.NewHttpTransport(
        "http://localhost:3000/mcp",
        "your-api-key", // Optional
        30*time.Second,
    )
    
    // Create client
    client := velocity_mcp.NewMcpClient(transport)
    defer client.Close()
    
    // Initialize connection
    ctx := context.Background()
    initResult, err := client.Initialize(ctx)
    if err != nil {
        panic(err)
    }
    fmt.Printf("Connected to %s v%s\n", initResult.ServerInfo.Name, initResult.ServerInfo.Version)
    
    // List tools
    tools, err := client.ListTools(ctx)
    if err != nil {
        panic(err)
    }
    fmt.Printf("Available tools: %d\n", len(tools))
    
    // Call a tool
    result, err := client.CallTool(ctx, "file_read", map[string]interface{}{
        "path": "/path/to/file.txt",
    })
    if err != nil {
        panic(err)
    }
    
    for _, content := range result.Content {
        if content.Type == "text" {
            fmt.Printf("File contents:\n%s\n", content.Text)
        }
    }
}
```

### WebSocket Transport

```go
package main

import (
    "context"
    "fmt"
    
    velocity_mcp "github.com/UnitBuilds-CC/velocity-mcp/sdk/go"
)

func main() {
    // Create WebSocket transport
    transport := velocity_mcp.NewWebSocketTransport(
        "ws://localhost:3000/ws",
        "your-api-key", // Optional
    )
    
    // Create client
    client := velocity_mcp.NewMcpClient(transport)
    defer client.Close()
    
    // Initialize and use...
    ctx := context.Background()
    _, err := client.Initialize(ctx)
    if err != nil {
        panic(err)
    }
    
    // Use the client...
}
```

## API Reference

### McpClient

The main client struct for interacting with MCP servers.

#### Methods

- `Initialize(ctx) (*InitializeResult, error)` - Initialize the MCP connection
- `IsInitialized() bool` - Check if client is initialized
- `ListTools(ctx) ([]Tool, error)` - List available tools
- `CallTool(ctx, name, args) (*ToolCallResult, error)` - Call a tool
- `ListResources(ctx) ([]Resource, error)` - List available resources
- `ReadResource(ctx, uri) (*ResourceReadResult, error)` - Read a resource
- `ListPrompts(ctx) ([]Prompt, error)` - List available prompts
- `GetPrompt(ctx, name, args) (*PromptGetResult, error)` - Get a prompt
- `Ping(ctx) error` - Ping the server
- `SetLogLevel(ctx, level) error` - Set logging level
- `Close() error` - Close the connection

### Transport Types

#### HttpTransport

HTTP transport for connecting to MCP servers over HTTP.

```go
transport := velocity_mcp.NewHttpTransport(
    "http://localhost:3000/mcp", // URL
    "your-api-key",              // API key (optional)
    30*time.Second,              // Timeout
)
```

#### WebSocketTransport

WebSocket transport for bidirectional communication.

```go
transport := velocity_mcp.NewWebSocketTransport(
    "ws://localhost:3000/ws", // WebSocket URL
    "your-api-key",           // API key (optional)
)
```

### Error Handling

The SDK provides specific error types for different error scenarios:

```go
result, err := client.CallTool(ctx, "file_read", args)
if err != nil {
    switch e := err.(type) {
    case *velocity_mcp.ToolExecutionError:
        fmt.Printf("Tool failed: %s\n", e.Message)
    case *velocity_mcp.TransportError:
        fmt.Printf("Connection error: %s\n", e.Message)
    case *velocity_mcp.ProtocolError:
        fmt.Printf("Protocol error %d: %s\n", e.Code, e.Message)
    }
}
```

## Examples

### File Operations

```go
// Read a file
result, err := client.CallTool(ctx, "file_read", map[string]interface{}{
    "path": "/path/to/file.txt",
})

// Write a file
result, err := client.CallTool(ctx, "file_write", map[string]interface{}{
    "path": "/path/to/output.txt",
    "content": "Hello, World!",
})
```

### Shell Commands

```go
// Execute a shell command
result, err := client.CallTool(ctx, "shell_exec", map[string]interface{}{
    "command": "ls -la",
    "timeout": 30,
})
```

### HTTP Requests

```go
// Make an HTTP request
result, err := client.CallTool(ctx, "http_request", map[string]interface{}{
    "url": "https://api.example.com/data",
    "method": "GET",
    "timeout": 60,
})
```

### Working with Resources

```go
// List all resources
resources, err := client.ListResources(ctx)
for _, resource := range resources {
    fmt.Printf("%s: %s\n", resource.URI, resource.Name)
}

// Read a specific resource
result, err := client.ReadResource(ctx, "file:///path/to/file.txt")
for _, content := range result.Contents {
    if content.Text != nil {
        fmt.Printf("Content: %s\n", *content.Text)
    }
}
```

### Working with Prompts

```go
// List all prompts
prompts, err := client.ListPrompts(ctx)
for _, prompt := range prompts {
    fmt.Printf("%s: %s\n", prompt.Name, *prompt.Description)
}

// Get a prompt with arguments
result, err := client.GetPrompt(ctx, "code-review", map[string]interface{}{
    "code": "func main() { fmt.Println(\"Hello\") }",
})
```

## Requirements

- Go 1.21+
- github.com/gorilla/websocket (for WebSocket transport)

## License

MIT OR Apache-2.0

## Links

- [VELOCITY-MCP Server](https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP)
- [MCP Protocol Specification](https://modelcontextprotocol.io/)
- [API Documentation](https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/tree/main/docs/API.md)
