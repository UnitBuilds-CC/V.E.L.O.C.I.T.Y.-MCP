# Migration Guide: Node.js MCP to VELOCITY-MCP

This guide covers migrating from the Node.js reference MCP implementation to VELOCITY-MCP.

## Why Migrate?

| Metric | Node.js MCP | VELOCITY-MCP |
|--------|------------|--------------|
| Average latency | Baseline | 27.7x faster (NDA/shmem) |
| Memory usage | ~120 MB | ~15 MB |
| Startup time | ~500ms | <50ms |
| Binary size | ~80 MB (Node runtime) | ~5 MB (single executable) |

## Transport Mapping

| Node.js | VELOCITY-MCP | Notes |
|---------|--------------|-------|
| `stdio` | `--mode stdio` | Drop-in replacement, same JSON-RPC protocol |
| `streamable-http` | `--mode http` | Compatible HTTP transport |
| N/A | `--mode shmem` | Shared memory IPC (1us round-trip) |
| N/A | `--mode ws` | WebSocket transport |

**Most users start with stdio mode** — it works with all existing MCP clients with zero configuration changes.

## Configuration

### Node.js (before)
```json
{
  "mcpServers": {
    "my-server": {
      "command": "node",
      "args": ["server.js"],
      "env": { "PORT": "3000" }
    }
  }
}
```

### VELOCITY-MCP (after)
```json
{
  "mcpServers": {
    "my-server": {
      "command": "velocity_mcp",
      "args": ["--mode", "stdio"]
    }
  }
}
```

No `node` runtime needed. No `npm install`. Single binary.

### TOML Configuration (optional)

For advanced configuration, use a `config.toml`:

```toml
[server]
mode = "http"
addr = "0.0.0.0:3000"

[security]
rate_limit_rps = 100
max_request_size = 1048576

[audit]
enabled = true
max_entries = 10000
```

Run with: `velocity_mcp --config config.toml`

## Tool Compatibility

VELOCITY-MCP implements all standard MCP methods:

- `initialize` / `initialized`
- `ping`
- `tools/list` / `tools/call`
- `resources/list` / `resources/read` / `resources/subscribe`
- `prompts/list` / `prompts/get`
- `sampling/createMessage`
- `elicitation/create`
- `notifications/*`

All tools registered via the Node.js `server.tool()` API work identically. VELOCITY-MCP adds 16 built-in tools for file operations, shell execution, HTTP requests, and NDA document handling.

## Performance Expectations

### stdio mode (fair comparison, same transport)
- **1.0x average** (tied with Node.js)
- **1.7x faster at p99** (better tail latency)

### NDA/shmem mode (maximum performance)
- **27.7x faster average**
- **40.8x faster at p99**

The biggest gains come from shared memory transport, which eliminates JSON serialization overhead entirely. To use shmem, both the client and server must support it (see [Client Integration](CLIENT_INTEGRATION.md)).

## Step-by-Step Migration

1. **Download the binary** for your platform from [releases](https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/releases)
2. **Replace the command** in your MCP client config: `node server.js` → `velocity_mcp --mode stdio`
3. **Test** that your tools work identically (they should — same protocol)
4. **Optionally switch to HTTP mode** for web clients: `velocity_mcp --mode http`
5. **Optionally enable shmem** for maximum performance with compatible clients

## What Changes

- **Config format**: Environment variables → TOML file (optional, env vars still work)
- **Plugin system**: npm packages → VELOCITY-MCP plugin marketplace (JSON manifests)
- **Monitoring**: None built-in → Prometheus metrics, Grafana dashboards, OpenTelemetry

## What Stays the Same

- **MCP protocol**: Identical JSON-RPC over stdio
- **Tool interface**: Same input/output schema
- **Client compatibility**: Works with Claude Desktop, Cursor, Windsurf, and any MCP client
