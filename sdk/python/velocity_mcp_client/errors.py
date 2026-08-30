"""Error types for the MCP client."""

from typing import Any, Optional


class McpError(Exception):
    """Base exception for MCP client errors."""
    pass


class TransportError(McpError):
    """Transport-level error (connection, I/O, etc.)."""
    pass


class ProtocolError(McpError):
    """JSON-RPC protocol error."""
    def __init__(self, code: int, message: str, data: Optional[Any] = None):
        self.code = code
        self.message = message
        self.data = data
        super().__init__(f"Protocol error {code}: {message}")


class ServerError(McpError):
    """Server returned an error response."""
    pass


class InvalidParameterError(McpError):
    """Invalid configuration or parameters."""
    pass


class ConnectionClosedError(McpError):
    """Connection was closed unexpectedly."""
    pass


class TimeoutError(McpError):
    """Request timed out."""
    pass


class ToolExecutionError(McpError):
    """Tool execution failed."""
    pass


class ResourceNotFoundError(McpError):
    """Resource not found."""
    pass


class PromptNotFoundError(McpError):
    """Prompt not found."""
    pass
