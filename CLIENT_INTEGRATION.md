# Client Integration Guide

This guide shows how to integrate VELOCITY-MCP with popular MCP clients.

## Table of Contents

- [Claude Desktop](#claude-desktop)
- [Cursor](#cursor)
- [Windsurf](#windsurf)
- [Continue](#continue)
- [Cline](#cline)
- [Custom Clients](#custom-clients)

## Claude Desktop

### Installation

1. **Download VELOCITY-MCP**
   ```bash
   # macOS/Linux
   curl -L https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/releases/latest/download/velocity_mcp -o /usr/local/bin/velocity_mcp
   chmod +x /usr/local/bin/velocity_mcp
   
   # Windows (PowerShell)
   Invoke-WebRequest -Uri "https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/releases/latest/download/velocity_mcp.exe" -OutFile "$env:LOCALAPPDATA\Programs\velocity_mcp.exe"
   ```

2. **Configure Claude Desktop**
   
   Open Claude Desktop config file:
   - **macOS:** `~/Library/Application Support/Claude/claude_desktop_config.json`
   - **Windows:** `%APPDATA%\Claude\claude_desktop_config.json`
   - **Linux:** `~/.config/Claude/claude_desktop_config.json`

3. **Add VELOCITY-MCP configuration**
   ```json
   {
     "mcpServers": {
       "velocity": {
         "command": "/usr/local/bin/velocity_mcp",
         "args": []
       }
     }
   }
   ```
   
   **Windows:**
   ```json
   {
     "mcpServers": {
       "velocity": {
         "command": "%LOCALAPPDATA%\\Programs\\velocity_mcp.exe",
         "args": []
       }
     }
   }
   ```

4. **Restart Claude Desktop**

### With Authentication

If you've enabled API key authentication:

```json
{
  "mcpServers": {
    "velocity": {
      "command": "/usr/local/bin/velocity_mcp",
      "args": ["--api-key", "your-secret-key"]
    }
  }
}
```

### With HTTP Mode

For HTTP mode (useful for remote servers):

```json
{
  "mcpServers": {
    "velocity": {
      "command": "/usr/local/bin/velocity_mcp",
      "args": ["--mode", "http", "--addr", "127.0.0.1:3000"]
    }
  }
}
```

### Verification

After configuration, Claude Desktop will show VELOCITY-MCP tools:
- `file_read` - Read file contents
- `file_write` - Write to files
- `shell_exec` - Execute shell commands
- `http_request` - Make HTTP requests
- And more...

Try asking Claude: "Read the contents of my README.md file"

## Cursor

### Installation

1. **Download VELOCITY-MCP** (same as Claude Desktop)

2. **Open Cursor Settings**
   - Click the gear icon ⚙️
   - Navigate to "MCP" section

3. **Add New MCP Server**
   - Click "Add New MCP Server"
   - Fill in the details:
     - **Name:** `velocity`
     - **Type:** `command`
     - **Command:** `/usr/local/bin/velocity_mcp` (or full path on Windows)
     - **Args:** (leave empty)

4. **Save and Restart**

### Configuration File Alternative

You can also edit the Cursor MCP config directly:

**Location:**
- **macOS:** `~/.cursor/mcp.json`
- **Windows:** `%USERPROFILE%\.cursor\mcp.json`
- **Linux:** `~/.cursor/mcp.json`

**Content:**
```json
{
  "servers": {
    "velocity": {
      "command": "/usr/local/bin/velocity_mcp",
      "args": []
    }
  }
}
```

### Verification

In Cursor's AI chat, try:
- "List the tools available in VELOCITY-MCP"
- "Read the file at /path/to/file.txt"
- "Execute the command: ls -la"

## Windsurf

### Installation

1. **Download VELOCITY-MCP** (same as above)

2. **Open Windsurf Settings**
   - Click the cascade icon 🌊
   - Navigate to "Windsurf Settings" > "MCP"

3. **Add MCP Server**
   - Click "Add Server"
   - Configure:
     - **Name:** `velocity`
     - **Type:** `stdio`
     - **Command:** Full path to `velocity_mcp`
     - **Environment Variables:** (optional)

4. **Save Configuration**

### Configuration File

**Location:** `~/.codeium/windsurf/mcp_config.json`

```json
{
  "mcpServers": {
    "velocity": {
      "command": "/usr/local/bin/velocity_mcp",
      "args": [],
      "env": {}
    }
  }
}
```

### Verification

In Windsurf chat:
- "What tools do you have access to?"
- "Use file_read to check my project structure"

## Continue

### Installation

1. **Download VELOCITY-MCP** (same as above)

2. **Open Continue Config**
   - In VS Code, open Command Palette (Cmd/Ctrl+Shift+P)
   - Search for "Continue: Open Config"

3. **Add MCP Server**
   
   Add to your `config.json`:
   ```json
   {
     "mcpServers": [
       {
         "name": "velocity",
         "command": "/usr/local/bin/velocity_mcp",
         "args": []
       }
     ]
   }
   ```

4. **Reload Continue**

### Verification

In Continue chat:
- "List available MCP tools"
- "Read my current file using file_read"

## Cline

### Installation

1. **Download VELOCITY-MCP** (same as above)

2. **Open Cline Settings**
   - In VS Code, open Cline extension
   - Click the settings icon ⚙️

3. **Configure MCP Servers**
   
   Add to MCP settings:
   ```json
   {
     "mcpServers": {
       "velocity": {
         "command": "/usr/local/bin/velocity_mcp",
         "args": []
       }
     }
   }
   ```

4. **Save and Restart**

### Verification

In Cline chat:
- "What MCP tools are available?"
- "Use shell_exec to run: echo 'Hello from VELOCITY-MCP'"

## Custom Clients

If you're building a custom MCP client, here's how to integrate VELOCITY-MCP:

### stdio Mode Integration

```python
import subprocess
import json

# Start VELOCITY-MCP process
process = subprocess.Popen(
    ["/path/to/velocity_mcp", "--mode", "stdio"],
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    text=True
)

# Send initialize request
init_request = {
    "jsonrpc": "2.0",
    "method": "initialize",
    "params": {
        "protocolVersion": "2024-11-05",
        "capabilities": {},
        "clientInfo": {
            "name": "my-client",
            "version": "1.0.0"
        }
    },
    "id": 1
}

process.stdin.write(json.dumps(init_request) + "\n")
process.stdin.flush()

# Read response
response = process.stdout.readline()
print(json.loads(response))

# Call a tool
tool_request = {
    "jsonrpc": "2.0",
    "method": "tools/call",
    "params": {
        "name": "file_read",
        "arguments": {
            "path": "/path/to/file.txt"
        }
    },
    "id": 2
}

process.stdin.write(json.dumps(tool_request) + "\n")
process.stdin.flush()

response = process.stdout.readline()
print(json.loads(response))
```

### HTTP Mode Integration

```python
import requests

# Start VELOCITY-MCP in HTTP mode first:
# ./velocity_mcp --mode http --addr 127.0.0.1:3000

# Health check
health = requests.get("http://127.0.0.1:3000/health")
print(health.json())

# Call a tool
tool_request = {
    "jsonrpc": "2.0",
    "method": "tools/call",
    "params": {
        "name": "shell_exec",
        "arguments": {
            "command": "echo 'Hello from VELOCITY-MCP'"
        }
    },
    "id": 1
}

response = requests.post(
    "http://127.0.0.1:3000/mcp",
    json=tool_request,
    headers={"Content-Type": "application/json"}
)

print(response.json())
```

### With Authentication

```python
# HTTP mode with API key
response = requests.post(
    "http://127.0.0.1:3000/mcp",
    json=tool_request,
    headers={
        "Content-Type": "application/json",
        "Authorization": "Bearer your-api-key"
    }
)
```

## Troubleshooting

### Client Can't Find VELOCITY-MCP

**Problem:** "MCP server not found" or similar error

**Solutions:**
1. Verify the binary path is correct:
   ```bash
   which velocity_mcp  # Linux/macOS
   where velocity_mcp  # Windows
   ```

2. Check file permissions:
   ```bash
   chmod +x /path/to/velocity_mcp
   ```

3. Use absolute path in configuration

### Connection Issues

**Problem:** Client can't connect to server

**Solutions:**
1. Test server manually:
   ```bash
   ./velocity_mcp --mode stdio
   # Then send: {"jsonrpc":"2.0","method":"initialize","params":{},"id":1}
   ```

2. Check for port conflicts (HTTP mode):
   ```bash
   lsof -i :3000  # Linux/macOS
   netstat -ano | findstr :3000  # Windows
   ```

3. Try a different port:
   ```bash
   ./velocity_mcp --mode http --addr 127.0.0.1:8080
   ```

### Tools Not Showing Up

**Problem:** Client connects but tools aren't available

**Solutions:**
1. Verify initialization:
   ```bash
   curl http://127.0.0.1:3000/health
   ```

2. Check client logs for MCP protocol errors

3. Restart both server and client

### Authentication Failures

**Problem:** "401 Unauthorized" errors

**Solutions:**
1. Verify API key matches server configuration
2. Check Authorization header format: `Bearer your-api-key`
3. Ensure server was started with `--api-key` flag

## Performance Tips

### Use HTTP Mode for Remote Servers

If your MCP client and server are on different machines:
```bash
./velocity_mcp --mode http --addr 0.0.0.0:3000
```

### Enable Compression

For HTTP mode, enable gzip compression in your client to reduce bandwidth.

### Monitor Performance

Check the `/performance` endpoint to monitor:
- Request latency
- Throughput
- Error rates
- Resource usage

```bash
curl http://127.0.0.1:3000/performance | jq
```

## Security Best Practices

### Use API Keys

Always use API key authentication in production:
```bash
./velocity_mcp --mode http --api-key "$(openssl rand -hex 32)"
```

### Enable TLS

For HTTPS:
```bash
./velocity_mcp --mode http --tls-cert cert.pem --tls-key key.pem
```

### Restrict CORS

Limit cross-origin requests:
```bash
./velocity_mcp --mode http --cors-origins "https://your-domain.com"
```

### Use Rate Limiting

VELOCITY-MCP includes built-in rate limiting (enabled by default):
- 20 requests/second
- Configurable via `--rate-limit` flag

## Next Steps

After successful integration:

1. **Explore built-in tools** - Try all 8 built-in tools
2. **Monitor performance** - Use `/performance` endpoint
3. **Enable security** - API keys, TLS, CORS
4. **Set up monitoring** - Integrate `/metrics` with your observability stack
5. **Share your setup** - Let us know how it went!

## Getting Help

- **Integration Issues:** https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/issues
- **Documentation:** https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/wiki
- **Community:** https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/discussions

## Client-Specific Resources

- **Claude Desktop:** https://docs.anthropic.com/en/docs/claude-desktop
- **Cursor:** https://docs.cursor.com/
- **Windsurf:** https://docs.windsurf.com/
- **Continue:** https://docs.continue.dev/
- **Cline:** https://github.com/cline/cline

Happy coding with the fastest MCP server! 🚀
