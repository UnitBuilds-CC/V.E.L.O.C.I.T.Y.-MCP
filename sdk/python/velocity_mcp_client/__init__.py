"""
VELOCITY-MCP Python Client SDK

A type-safe Python client for connecting to VELOCITY-MCP servers.

Example:
    ```python
    import asyncio
    from velocity_mcp_client import McpClient, HttpTransport

    async def main():
        # Connect via HTTP
        transport = HttpTransport("http://localhost:3000/mcp", api_key="your-key")
        client = McpClient(transport)
        
        # Initialize connection
        await client.initialize()
        
        # List tools
        tools = await client.list_tools()
        print(f"Available tools: {len(tools)}")
        
        # Call a tool
        result = await client.call_tool("file_read", {"path": "/path/to/file.txt"})
        print(result)
        
        await client.close()

    asyncio.run(main())
    ```
"""

from .client import McpClient
from .transport import Transport, HttpTransport, StdioTransport
from .types import (
    Tool,
    Resource,
    Prompt,
    Content,
    ToolCallResult,
    InitializeResult,
)
from .errors import McpError, TransportError, ProtocolError

__version__ = "3.0.0"
__all__ = [
    "McpClient",
    "Transport",
    "HttpTransport",
    "StdioTransport",
    "Tool",
    "Resource",
    "Prompt",
    "Content",
    "ToolCallResult",
    "InitializeResult",
    "McpError",
    "TransportError",
    "ProtocolError",
]
