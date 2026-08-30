# VELOCITY-MCP Python Client SDK

A type-safe Python client for connecting to VELOCITY-MCP servers.

## Installation

```bash
pip install velocity-mcp-client
```

Or install from source:

```bash
cd sdk/python
pip install -e .
```

## Quick Start

### HTTP Transport

```python
import asyncio
from velocity_mcp_client import McpClient, HttpTransport

async def main():
    # Connect via HTTP
    transport = HttpTransport(
        url="http://localhost:3000/mcp",
        api_key="your-api-key",  # Optional
        timeout=30.0
    )
    client = McpClient(transport)
    
    try:
        # Initialize connection
        init_result = await client.initialize()
        print(f"Connected to {init_result.server_info.name} v{init_result.server_info.version}")
        
        # List available tools
        tools = await client.list_tools()
        print(f"Available tools: {len(tools)}")
        for tool in tools:
            print(f"  - {tool.name}: {tool.description}")
        
        # Call a tool
        result = await client.call_tool("file_read", {"path": "/path/to/file.txt"})
        for content in result.content:
            if hasattr(content, "text"):
                print(f"File contents:\n{content.text}")
        
    finally:
        await client.close()

asyncio.run(main())
```

### Stdio Transport

```python
import asyncio
from velocity_mcp_client import McpClient, StdioTransport

async def main():
    # Connect via stdio (spawns server process)
    transport = StdioTransport(
        command="velocity_mcp",
        args=["--mode", "stdio"]
    )
    client = McpClient(transport)
    
    try:
        await client.initialize()
        
        # Use the client...
        tools = await client.list_tools()
        print(f"Found {len(tools)} tools")
        
    finally:
        await client.close()

asyncio.run(main())
```

## API Reference

### McpClient

The main client class for interacting with MCP servers.

#### Methods

- `initialize()` - Initialize the MCP connection
- `is_initialized` - Check if client is initialized (property)
- `list_tools()` - List available tools
- `call_tool(name, arguments)` - Call a tool
- `list_resources()` - List available resources
- `read_resource(uri)` - Read a resource
- `list_prompts()` - List available prompts
- `get_prompt(name, arguments)` - Get a prompt
- `ping()` - Ping the server
- `set_log_level(level)` - Set logging level
- `close()` - Close the connection

### Transport Classes

#### HttpTransport

HTTP transport for connecting to MCP servers over HTTP.

```python
HttpTransport(
    url="http://localhost:3000/mcp",  # Required
    api_key="your-api-key",           # Optional
    timeout=30.0                       # Optional, seconds
)
```

#### StdioTransport

Stdio transport for spawning and communicating with MCP server processes.

```python
StdioTransport(
    command="velocity_mcp",           # Required
    args=["--mode", "stdio"]          # Optional
)
```

### Error Handling

The SDK provides specific exception types for different error scenarios:

```python
from velocity_mcp_client import (
    McpError,           # Base exception
    TransportError,     # Connection/IO errors
    ProtocolError,      # JSON-RPC protocol errors
    ToolExecutionError, # Tool execution failures
    ResourceNotFoundError,
    PromptNotFoundError,
)

try:
    result = await client.call_tool("file_read", {"path": "/missing.txt"})
except ToolExecutionError as e:
    print(f"Tool failed: {e}")
except TransportError as e:
    print(f"Connection error: {e}")
```

## Examples

### File Operations

```python
# Read a file
result = await client.call_tool("file_read", {
    "path": "/path/to/file.txt"
})

# Write a file
result = await client.call_tool("file_write", {
    "path": "/path/to/output.txt",
    "content": "Hello, World!"
})
```

### Shell Commands

```python
# Execute a shell command
result = await client.call_tool("shell_exec", {
    "command": "ls -la",
    "timeout": 30
})

for content in result.content:
    if hasattr(content, "text"):
        print(content.text)
```

### HTTP Requests

```python
# Make an HTTP request
result = await client.call_tool("http_request", {
    "url": "https://api.example.com/data",
    "method": "GET",
    "timeout": 60
})
```

### Working with Resources

```python
# List all resources
resources = await client.list_resources()
for resource in resources:
    print(f"{resource.uri}: {resource.name}")

# Read a specific resource
result = await client.read_resource("file:///path/to/file.txt")
for content in result.contents:
    if content.text:
        print(content.text)
```

### Working with Prompts

```python
# List all prompts
prompts = await client.list_prompts()
for prompt in prompts:
    print(f"{prompt.name}: {prompt.description}")

# Get a prompt with arguments
result = await client.get_prompt("code-review", {
    "code": "def hello(): print('Hello')"
})
for message in result.messages:
    print(f"{message.role}: {message.content}")
```

## Type Safety

The SDK uses Pydantic models for all types, providing full type safety and validation:

```python
from velocity_mcp_client import Tool, Resource, Prompt

# All types are properly typed
tools: List[Tool] = await client.list_tools()
resources: List[Resource] = await client.list_resources()
prompts: List[Prompt] = await client.list_prompts()
```

## Async Support

The SDK is fully async using `asyncio`:

```python
import asyncio
from velocity_mcp_client import McpClient, HttpTransport

async def main():
    async with HttpTransport("http://localhost:3000/mcp") as transport:
        client = McpClient(transport)
        await client.initialize()
        
        # Make multiple concurrent requests
        tools, resources = await asyncio.gather(
            client.list_tools(),
            client.list_resources()
        )
        
        print(f"Tools: {len(tools)}, Resources: {len(resources)}")

asyncio.run(main())
```

## Development

### Setup

```bash
cd sdk/python
pip install -e ".[dev]"
```

### Testing

```bash
pytest
```

### Type Checking

```bash
mypy velocity_mcp_client
```

### Formatting

```bash
black velocity_mcp_client
```

## Requirements

- Python 3.8+
- httpx >= 0.25.0
- pydantic >= 2.0.0

## License

MIT OR Apache-2.0

## Links

- [VELOCITY-MCP Server](https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP)
- [MCP Protocol Specification](https://modelcontextprotocol.io/)
- [API Documentation](https://github.com/UnitBuilds-CC/V.E.L.O.C.I.T.Y.-MCP/tree/main/docs/API.md)
