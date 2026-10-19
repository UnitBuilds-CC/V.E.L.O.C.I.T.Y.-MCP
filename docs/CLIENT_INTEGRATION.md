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

Module `github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/sdk/go` (source in `sdk/go`):

```go
package main

import (
    "context"
    "time"

    velocity_mcp "github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/sdk/go"
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

**shmem needs one path on both sides, and the two sides do not share a name for it.** The buffer file is the rendezvous point and each side chooses it independently: the server takes `--buffer-path <path>`, the top-level `buffer_path` config key, or `VELOCITY_BUFFER_PATH`, all defaulting to `nmcp_buffer.bin` (`default_buffer_path`), while `ShmemTransport::new()` in the Rust client has no default at all and requires the path as an argument. The shipped `shmem_client` example reads a third name, `VELOCITY_MCP_BUFFER`, whose own fallback is `velocity_mcp.bin` -- a different file from the one the server creates when left on its default. The example's module header therefore pairs it with `--mode shmem --buffer-path velocity_mcp.bin`; either match that, or export `VELOCITY_MCP_BUFFER` with the server's actual path. Getting it wrong surfaces as the client's `open` returning an error rather than as a hang, because nothing creates the file the client is asking for.

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

## Notification Handling

A JSON-RPC notification carries no `id`, so the server never answers it with an envelope.
What "never" means depends on the transport:

| Transport | Server reply to a notification |
|-----------|--------------------------------|
| stdio | no line at all |
| HTTP | `204 No Content` with an empty body |
| shared memory (JSON) | an envelope with `"id": null` and an empty `result` |
| shared memory (NDA frames) | a `STATUS_OK` frame |

The two silent legs are why every SDK treats a notification as *accepted, nothing to
decode* rather than as a reply to wait for: `initialize` sends `notifications/initialized`
as part of the handshake, so a client that blocks on an envelope for it never finishes
connecting. Each SDK reports the accepted notification in its own idiom, and the shared
memory transports need no special case because a reply does arrive there:

- **Rust** (`velocity-mcp-client`): `Transport::send` returns a `JsonRpcResponse` whose
  `result` is `{}` and whose `id` is `null`. **Released 3.2.0 does not do this yet**, and the
  difference is not cosmetic: that version has no notification arm on either of its two
  simple transports, so the `notifications/initialized` leg of `initialize()` is decoded as a
  response that never arrives — over HTTP it fails with `Transport("error decoding response
  body")`, and over stdio it blocks on a line the server will never print. The library has no
  timeout on that read, so the stdio leg hangs rather than erroring; `client/tests/notification_test.rs`
  wraps the handshake in its own 20 s guard and therefore reports `initialize did not return
  within 20s`. Both are measured against this repo's server on the
  `v3.2.0` tag, and both are the calls `client/README.md` shows first. The fix is in this
  tree's `Transport` implementations and is gated by `client/tests/notification_test.rs`,
  whose server-backed pair runs with `--include-ignored`; it needs a release to reach
  crates.io.
- **Python**: `Transport.send` returns `None`.
- **TypeScript**: `Transport.send` resolves to `undefined`.
- **Go**: `Transport.Send` returns `(nil, nil)`.

The mirror case is an error, not an ack. A client's own requests always carry an `id`, so a
`204` in answer to one means the envelope the request asked for never arrived. All four
clients raise their transport-level error (`Error::Transport`, `TransportError`) naming the
request id instead of reading a property off the missing envelope; the Go client used to
panic on exactly that path.

Each arm is enforced where that client's own test suite lives:

- **Rust**: `client/tests/notification_test.rs` (the two `#[ignore]`d legs need the built
  server binary; set `VELOCITY_MCP_SERVER` to run them).
- **Go**: `sdk/go/notification_test.go`, run with `go test ./...` in `sdk/go`.
- **Python**: `sdk/python/test_notifications.py`, run with `python -m pytest sdk/python -q`.
- **TypeScript**: `python scripts/check_ts_client.py`, which executes the shipped
  `client.ts` under Node's type stripper because the package has no jest suite yet.
