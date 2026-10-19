# Migration Guide: Node.js MCP to VELOCITY-MCP

This guide covers migrating from the Node.js reference MCP implementation to VELOCITY-MCP.

## Why Migrate?

| Metric | Node.js MCP | VELOCITY-MCP |
|--------|------------|--------------|
| Average latency, same transport (stdio + JSON-RPC) | Baseline | 2.2x faster on `ping`, 1.4x on `tools/call`, 4.6x **slower** on `tools/list` |
| Average latency, NDA/shmem vs Node JSON/stdio | Baseline | 9.7x–14.5x faster |
| Memory usage | not currently measured | not currently measured |
| Startup time | not measured | 4.6 ms to bind with 19 plugins loaded (warm); 173 ms cold |
| Download size | 92,825,416 bytes (`node.exe` v24.19.0 on this box) | 14,682,112 bytes (`velocity_mcp.exe`, release) |

## Transport Mapping

| Node.js | VELOCITY-MCP | Notes |
|---------|--------------|-------|
| `stdio` | `--mode stdio` | Drop-in replacement, same JSON-RPC protocol |
| `streamable-http` | `--mode http` | Compatible HTTP transport |
| N/A | `--mode shmem` | Shared memory IPC (1us round-trip) |
| N/A | `--mode http` + `GET /v1/ws` | WebSocket upgrade on the HTTP server (there is no `ws` mode) |

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
mode = "http"

[http]
addr = "0.0.0.0:3000"
max_request_size = 10485760   # bytes
enable_rate_limit = true      # boolean switch only

[logging]
level = "info"

[features]
nda_merkle = true
```

Rate limiting is a boolean switch (the 20 req/s, burst 100 bucket is compiled
in), and audit logging is always on — there are no per-second rate keys and no
audit toggle.

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

### stdio mode (fair comparison: same transport, same JSON-RPC encoding)
Measured 2026-09-17, warm p50, Core 5 210H (raw: [Performance Comparison](COMPARISON.md)):
- **ping**: 0.044 ms vs 0.098 ms → Rust 2.2x
- **tools/call**: 0.061 ms vs 0.083 ms → Rust 1.4x
- **tools/list**: 0.807 ms vs 0.175 ms → **Node 4.6x**. Rust assembles 40 tools from 5 registries; the Node stub returns a static array.

### NDA/shmem mode (maximum performance)
- **vs this server's own JSON/stdio path** (2026-09-17): ping 82.0x, tools/list 249.7x, tools/call 36.2x on average
- **vs Node.js JSON/stdio** (2026-09-02 full-stack run): ping 14.5x, tools/list 11.4x, tools/call 9.7x

The shmem headline ratios are keyed to VELOCITY-MCP's own JSON/stdio baseline, not to Node.js — they measure the transport and encoding change, not the language. To use shmem, both the client and server must support it (see [Client Integration](CLIENT_INTEGRATION.md)).

## Step-by-Step Migration

1. **Download the binary** for your platform from [releases](https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/releases)
2. **Replace the command** in your MCP client config: `node server.js` → `velocity_mcp --mode stdio`
3. **Test** that your tools work identically (they should — same protocol)
4. **Optionally switch to HTTP mode** for web clients: `velocity_mcp --mode http`
5. **Optionally enable shmem** for maximum performance with compatible clients

## What Changes

- **Config format**: Environment variables → TOML file (optional, env vars still work)
- **Plugin system**: npm packages → VELOCITY-MCP plugin marketplace (JSON manifests)
- **Monitoring**: None built-in → JSON counters at `/v1/metrics` and Prometheus exposition at `/v1/metrics/prometheus`, both needing the `http` feature the published release binary and Docker image build with, plus a Grafana dashboard JSON under `deploy/grafana/`. OpenTelemetry is library-only: the `observability` feature is compiled into neither artifact, and no code path calls `init_observability`, so spans export only for an embedder that sets up the tracer itself.

## What Stays the Same

- **MCP protocol**: Identical JSON-RPC over stdio
- **Tool interface**: Same input/output schema
- **Client compatibility**: Works with Claude Desktop, Cursor, Windsurf, and any MCP client
