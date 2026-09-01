# Client Integration Guide

How to connect MCP clients to VELOCITY-MCP.

## Claude Desktop

### stdio mode (recommended)

Edit your Claude Desktop config (`claude_desktop_config.json`):

```json
{
  "mcpServers": {
    "velocity": {
      "command": "velocity_mcp",
      "args": ["--mode", "stdio"]
    }
  }
}
```

### HTTP mode

```json
{
  "mcpServers": {
    "velocity": {
      "url": "http://localhost:3000/v1/mcp"
    }
  }
}
```

Start the server first: `velocity_mcp --mode http --addr 127.0.0.1:3000`

## Cursor

### stdio mode

In Cursor Settings → MCP, add:

```json
{
  "mcpServers": {
    "velocity": {
      "command": "velocity_mcp",
      "args": ["--mode", "stdio"]
    }
  }
}
```

### HTTP mode

```json
{
  "mcpServers": {
    "velocity": {
      "url": "http://localhost:3000/v1/mcp"
    }
  }
}
```

## Windsurf

### stdio mode

Add to your Windsurf MCP configuration:

```json
{
  "mcpServers": {
    "velocity": {
      "command": "velocity_mcp",
      "args": ["--mode", "stdio"]
    }
  }
}
```

## Custom Clients via SDKs

### Rust

```rust
use velocity_mcp_client::Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::new("http://localhost:3000");
    let tools = client.list_tools().await?;
    let result = client.call_tool("file_read", serde_json::json!({"path": "README.md"})).await?;
    Ok(())
}
```

### Python

```python
from velocity_mcp import Client

client = Client("http://localhost:3000")
tools = client.list_tools()
result = client.call_tool("file_read", {"path": "README.md"})
```

### TypeScript

```typescript
import { Client } from 'velocity-mcp-client';

const client = new Client('http://localhost:3000');
const tools = await client.listTools();
const result = await client.callTool('file_read', { path: 'README.md' });
```

### Go

```go
import velocity_mcp "github.com/UnitBuilds-CC/velocity-mcp/client/go"

client := velocity_mcp.NewClient("http://localhost:3000")
tools, err := client.ListTools()
result, err := client.CallTool("file_read", map[string]interface{}{"path": "README.md"})
```

## Transport Selection Guide

| Transport | Latency | Use Case | Client Support |
|-----------|---------|----------|----------------|
| **stdio** | ~35us | Universal compatibility | All MCP clients |
| **HTTP/SSE** | ~200us | Web clients, REST APIs | Claude Desktop, Cursor |
| **WebSocket** | ~100us | Real-time bidirectional | Custom clients |
| **Shared Memory** | ~1us | Ultra-low latency IPC | VELOCITY SDKs only |

**Recommendation:** Start with stdio for compatibility. Switch to shmem when you need maximum throughput and both client and server run on the same machine.

## Authentication

For HTTP mode, set an API key:

```bash
velocity_mcp --mode http --api-key your-secret-key
```

Clients authenticate via the `Authorization` header:

```
Authorization: Bearer your-secret-key
```

The `/health` endpoint does not require authentication.
