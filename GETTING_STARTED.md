# Getting Started with VELOCITY-MCP

Welcome! This guide will get you up and running with VELOCITY-MCP in under 5 minutes.

## What is VELOCITY-MCP?

VELOCITY-MCP is a high-performance Model Context Protocol (MCP) server written in Rust. It's:

- **3.8x faster** than Node.js MCP servers
- **8x smaller** memory footprint
- **Zero-config** - works out of the box
- **Production-hardened** - timeouts, rate limiting, security built-in
- **Cross-platform** - Windows, macOS, Linux

## Quick Start (30 seconds)

### 1. Download the binary

**Windows:**
```powershell
# Download latest release
Invoke-WebRequest -Uri "https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/releases/latest/download/velocity_mcp.exe" -OutFile "velocity_mcp.exe"
```

**macOS/Linux:**
```bash
# Download latest release
curl -L https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/releases/latest/download/velocity_mcp -o velocity_mcp
chmod +x velocity_mcp
```

### 2. Run the server

```bash
./velocity_mcp
```

That's it! The server starts in stdio mode by default, ready to accept MCP connections.

### 3. Connect your MCP client

Configure your MCP client (Claude Desktop, Cursor, etc.) to use VELOCITY-MCP:

```json
{
  "mcpServers": {
    "velocity": {
      "command": "/path/to/velocity_mcp",
      "args": []
    }
  }
}
```

## Built-in Tools

VELOCITY-MCP comes with 8 powerful tools ready to use:

### File Operations

**file_read** - Read file contents
```json
{
  "name": "file_read",
  "arguments": {
    "path": "/path/to/file.txt"
  }
}
```

**file_write** - Write to files
```json
{
  "name": "file_write",
  "arguments": {
    "path": "/path/to/file.txt",
    "content": "Hello, World!"
  }
}
```

### Shell & HTTP

**shell_exec** - Execute shell commands (with timeout)
```json
{
  "name": "shell_exec",
  "arguments": {
    "command": "ls -la",
    "timeout": 30
  }
}
```

**http_request** - Make HTTP requests (with retry logic)
```json
{
  "name": "http_request",
  "arguments": {
    "url": "https://api.example.com/data",
    "method": "GET",
    "timeout": 30
  }
}
```

### NDA Document Processing

**convert_to_nda_document** - Convert files to NDA format
**read_nda** - Read NDA documents
**execute_nda** - Execute NDA containers
**convert_to_nda_tool** - Convert JSON tools to NDA for 90x faster execution

## Configuration

VELOCITY-MCP works with zero configuration, but you can customize it:

### Command-line options

```bash
# HTTP mode with custom port
./velocity_mcp --mode http --addr 0.0.0.0:8080

# With TLS/HTTPS
./velocity_mcp --mode http --tls-cert cert.pem --tls-key key.pem

# With API key authentication
./velocity_mcp --mode http --api-key your-secret-key

# Custom config file
./velocity_mcp --config config.toml
```

### Configuration file (config.toml)

```toml
[server]
mode = "http"
addr = "0.0.0.0:3000"

[http]
api_key = "your-secret-key"
max_request_size = 10485760  # 10MB
enable_rate_limit = true
cors_origins = ["https://example.com"]

[security]
tls_cert = "cert.pem"
tls_key = "key.pem"

[logging]
level = "info"
```

## Common Workflows

### Workflow 1: File Processing Pipeline

```bash
# 1. Read a source file
./velocity_mcp --mode stdio <<EOF
{"jsonrpc":"2.0","method":"tools/call","params":{"name":"file_read","arguments":{"path":"input.txt"}},"id":1}
EOF

# 2. Process with shell command
./velocity_mcp --mode stdio <<EOF
{"jsonrpc":"2.0","method":"tools/call","params":{"name":"shell_exec","arguments":{"command":"grep pattern input.txt"}},"id":2}
EOF

# 3. Write results
./velocity_mcp --mode stdio <<EOF
{"jsonrpc":"2.0","method":"tools/call","params":{"name":"file_write","arguments":{"path":"output.txt","content":"processed data"}},"id":3}
EOF
```

### Workflow 2: HTTP API Integration

```bash
# Fetch data from API
./velocity_mcp --mode stdio <<EOF
{"jsonrpc":"2.0","method":"tools/call","params":{"name":"http_request","arguments":{"url":"https://api.github.com/repos/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP","method":"GET"}},"id":1}
EOF
```

### Workflow 3: NDA Document Processing

```bash
# Convert a file to NDA format
./velocity_mcp --mode stdio <<EOF
{"jsonrpc":"2.0","method":"tools/call","params":{"name":"convert_to_nda_document","arguments":{"filePath":"document.pdf"}},"id":1}
EOF

# Read the NDA document
./velocity_mcp --mode stdio <<EOF
{"jsonrpc":"2.0","method":"tools/call","params":{"name":"read_nda","arguments":{"ndaPath":"document.nda"}},"id":2}
EOF
```

## Monitoring & Observability

### Health Check

```bash
curl http://localhost:3000/health
```

Response:
```json
{
  "status": "healthy",
  "version": "3.0.0",
  "uptime_seconds": 3600
}
```

### Performance Metrics

```bash
curl http://localhost:3000/performance
```

Response includes:
- Request latency (average, p95, p99)
- Throughput (requests/second)
- Active connections
- Memory usage
- Comparison with Node.js performance

### Server Metrics

```bash
curl http://localhost:3000/metrics
```

Response includes:
- Total requests
- Success/failure rates
- Rate limit hits
- Authentication failures

## Security Features

VELOCITY-MCP includes production-grade security:

- **Authentication** - API key or TLS client certificates
- **Rate limiting** - Token bucket algorithm (20 req/sec default)
- **Timeouts** - All operations have configurable timeouts
- **Input validation** - SSRF prevention, command injection protection
- **Resource limits** - Max sessions, file sizes, request sizes
- **CORS** - Configurable cross-origin restrictions
- **TLS/HTTPS** - Full TLS 1.3 support

## Troubleshooting

### Server won't start

**Problem:** "Address already in use"
**Solution:** Change the port: `./velocity_mcp --addr 0.0.0.0:8080`

### Connection refused

**Problem:** Client can't connect to server
**Solution:** 
1. Check server is running: `curl http://localhost:3000/health`
2. Check firewall settings
3. Verify correct address/port in client config

### Authentication failed

**Problem:** "401 Unauthorized"
**Solution:** 
1. Check API key is correct
2. Ensure Authorization header format: `Bearer your-api-key`
3. Verify server config has `api_key` set

### Tool execution timeout

**Problem:** "Command timed out after 30 seconds"
**Solution:** Increase timeout: `"timeout": 60` in tool arguments

## Next Steps

- **Examples:** See `examples/` directory for working code samples
- **Migration:** See `MIGRATION.md` if migrating from Node.js MCP
- **Client Integration:** See `CLIENT_INTEGRATION.md` for Claude Desktop, Cursor setup
- **API Reference:** See `docs/API.md` for complete tool documentation

## Getting Help

- **GitHub Issues:** https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/issues
- **Discussions:** https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/discussions
- **Documentation:** https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/wiki

## What's Next?

Now that you're up and running:

1. **Try the examples** in the `examples/` directory
2. **Connect your favorite MCP client** (see CLIENT_INTEGRATION.md)
3. **Explore the built-in tools** - file operations, shell, HTTP
4. **Monitor performance** at `/performance` endpoint
5. **Customize configuration** for your use case

Welcome to the fastest MCP server in the world! 🚀
