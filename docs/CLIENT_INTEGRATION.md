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

Client crate `velocity-mcp-client` (source in `client/`):

```rust
use velocity_mcp_client::{HttpTransport, McpClient};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let transport = HttpTransport::new(
        "http://localhost:3000/v1/mcp",
        Some("your-secret-key".to_string()),
    )?;
    let mut client = McpClient::new(transport);

    client.initialize().await?;
    let tools = client.list_tools().await?;
    let result = client
        .call_tool("file_read", serde_json::json!({"path": "README.md"}))
        .await?;
    Ok(())
}
```

### Python

Package `velocity_mcp_client` (source in `sdk/python`):

```python
import asyncio
from velocity_mcp_client import HttpTransport, McpClient

async def main():
    transport = HttpTransport(
        url="http://localhost:3000/v1/mcp",
        api_key="your-secret-key",  # optional
    )
    client = McpClient(transport)
    await client.initialize()
    tools = await client.list_tools()
    result = await client.call_tool("file_read", {"path": "README.md"})
    await client.close()

asyncio.run(main())
```

### TypeScript

Package `@velocity-mcp/client` (source in `sdk/typescript`):

```typescript
import { HttpTransport, McpClient } from '@velocity-mcp/client';

const transport = new HttpTransport('http://localhost:3000/v1/mcp', {
  apiKey: 'your-secret-key', // optional
});
const client = new McpClient(transport);

await client.initialize();
const tools = await client.listTools();
const result = await client.callTool('file_read', { path: 'README.md' });
await client.close();
```

### Go

Module `github.com/UnitBuilds-CC/velocity-mcp/sdk/go` (source in `sdk/go`):

```go
package main

import (
    "context"
    "time"

    velocity_mcp "github.com/UnitBuilds-CC/velocity-mcp/sdk/go"
)

func main() {
    transport := velocity_mcp.NewHttpTransport(
        "http://localhost:3000/v1/mcp",
        "your-secret-key", // optional; empty for no auth
        30*time.Second,
    )
    client := velocity_mcp.NewMcpClient(transport)
    defer client.Close()

    ctx := context.Background()
    _, _ = client.Initialize(ctx)
    tools, err := client.ListTools(ctx)
    result, err := client.CallTool(ctx, "file_read", map[string]interface{}{"path": "README.md"})
}
```

## Transport Selection Guide

| Transport | Latency | Use Case | Client Support |
|-----------|---------|----------|----------------|
| **stdio** | not currently measured | Universal compatibility | All MCP clients |
| **HTTP/SSE** | not currently measured | Web clients, REST APIs | Claude Desktop, Cursor |
| **WebSocket** | not currently measured | Real-time bidirectional | Custom clients |
| **Shared Memory** | not currently measured | Ultra-low latency IPC | VELOCITY SDKs only |

Per-transport latency figures are not currently measured here; see the
benchmark documentation in this repository for the numbers that are recorded.

**Recommendation:** Start with stdio for compatibility. Switch to shmem when you need maximum throughput and both client and server run on the same machine.

## Authentication

For HTTP mode, set an API key (there is no CLI flag for it):

```toml
# config.toml
[http]
api_key = "your-secret-key"
```

```bash
VELOCITY_API_KEY=your-secret-key velocity_mcp --mode http
```

Clients authenticate via the `Authorization` header:

```
Authorization: Bearer your-secret-key
```

`/health` is the only top-level route and needs no authentication; every
endpoint under `/v1` does whenever an API key is configured.
