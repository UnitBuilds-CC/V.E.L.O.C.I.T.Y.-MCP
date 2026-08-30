# VELOCITY-MCP Examples

This directory contains working examples demonstrating how to use VELOCITY-MCP in different scenarios.

## Prerequisites

Before running the examples, build the server:

```bash
cd ..
cargo build --release --features http,database,oauth2
```

## Examples

### 1. Basic stdio Mode (`basic-stdio.sh`)

The simplest way to use VELOCITY-MCP. Demonstrates:
- Starting the server in stdio mode
- Sending JSON-RPC requests
- Using built-in tools (file_read, shell_exec)
- Basic MCP session flow

**Run:**
```bash
./basic-stdio.sh
```

### 2. HTTP Server Mode (`http-server.sh`)

Run VELOCITY-MCP as an HTTP server. Demonstrates:
- Starting the server in HTTP mode
- Using the health endpoint
- Using the performance endpoint
- Making MCP requests over HTTP
- Testing built-in tools via HTTP

**Run:**
```bash
./http-server.sh
```

**Access endpoints:**
- Health: http://localhost:3000/health
- Performance: http://localhost:3000/performance
- Metrics: http://localhost:3000/metrics
- MCP: http://localhost:3000/mcp (POST)

### 3. File Operations (`file-operations.sh`)

Comprehensive file operations example. Demonstrates:
- Creating files with file_write
- Reading files with file_read
- Processing files with shell_exec
- Building a simple file processing pipeline

**Run:**
```bash
./file-operations.sh
```

### 4. HTTP API Integration (`http-api.sh`)

Integration with external HTTP APIs. Demonstrates:
- Making HTTP GET requests
- Making HTTP POST requests
- Handling JSON responses
- Error handling for failed requests

**Run:**
```bash
./http-api.sh
```

### 5. NDA Document Processing (`nda-processing.sh`)

NDA document format processing. Demonstrates:
- Converting files to NDA format
- Reading NDA documents
- Understanding NDA structure
- Converting JSON tools to NDA for 90x faster execution

**Run:**
```bash
./nda-processing.sh
```

### 6. Custom Tools (`custom-tools.sh`)

Creating and using custom tools. Demonstrates:
- Registering custom tools
- Tool input validation
- Error handling in tools
- Tool composition

**Run:**
```bash
./custom-tools.sh
```

## Example Structure

Each example follows this pattern:

1. **Setup** - Prepare environment and dependencies
2. **Start Server** - Launch VELOCITY-MCP in appropriate mode
3. **Initialize Session** - Send MCP initialize request
4. **Execute Operations** - Use tools to perform operations
5. **Cleanup** - Stop server and clean up resources

## Common Patterns

### Stdio Mode Pattern

```bash
./velocity_mcp --mode stdio <<'EOF'
{"jsonrpc":"2.0","method":"initialize","params":{...},"id":1}
{"jsonrpc":"2.0","method":"tools/call","params":{...},"id":2}
EOF
```

### HTTP Mode Pattern

```bash
# Start server
./velocity_mcp --mode http --addr 127.0.0.1:3000 &

# Make requests
curl -X POST http://127.0.0.1:3000/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"tools/call","params":{...},"id":1}'

# Stop server
kill %1
```

### Tool Call Pattern

```json
{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": {
    "name": "tool_name",
    "arguments": {
      "param1": "value1",
      "param2": "value2"
    }
  },
  "id": 1
}
```

## Troubleshooting

### "Permission denied" when running examples

```bash
chmod +x *.sh
```

### "No such file or directory" for velocity_mcp

Build the server first:
```bash
cd ..
cargo build --release --features http,database,oauth2
```

### Server won't start

Check if port 3000 is in use:
```bash
lsof -i :3000  # Linux/macOS
netstat -ano | findstr :3000  # Windows
```

Use a different port:
```bash
./velocity_mcp --mode http --addr 127.0.0.1:8080
```

## Next Steps

After running the examples:

1. **Read the Getting Started Guide** - `../GETTING_STARTED.md`
2. **Try the Migration Guide** - `../MIGRATION.md` (if coming from Node.js)
3. **Set up your MCP client** - `../CLIENT_INTEGRATION.md`
4. **Explore the API** - `../docs/API.md`

## Contributing Examples

Have a useful example? Contributions welcome!

Example guidelines:
- Keep it simple and focused
- Include clear comments
- Handle errors gracefully
- Clean up resources
- Document what it demonstrates

Submit a pull request with your example!
