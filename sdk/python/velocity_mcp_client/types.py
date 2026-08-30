"""MCP protocol types."""

from typing import Any, Dict, List, Optional, Union
from pydantic import BaseModel, Field


class JsonRpcRequest(BaseModel):
    """JSON-RPC request."""
    jsonrpc: str = "2.0"
    method: str
    params: Optional[Dict[str, Any]] = None
    id: Optional[Union[int, str]] = None


class JsonRpcError(BaseModel):
    """JSON-RPC error."""
    code: int
    message: str
    data: Optional[Any] = None


class JsonRpcResponse(BaseModel):
    """JSON-RPC response."""
    jsonrpc: str = "2.0"
    result: Optional[Any] = None
    error: Optional[JsonRpcError] = None
    id: Optional[Union[int, str]] = None


class ClientInfo(BaseModel):
    """Client information."""
    name: str
    version: str


class ClientCapabilities(BaseModel):
    """Client capabilities."""
    roots: Optional[Dict[str, Any]] = None


class InitializeParams(BaseModel):
    """Initialize request parameters."""
    protocol_version: str = Field(alias="protocolVersion")
    capabilities: ClientCapabilities
    client_info: ClientInfo = Field(alias="clientInfo")


class ServerInfo(BaseModel):
    """Server information."""
    name: str
    version: str


class ToolsCapability(BaseModel):
    """Tools capability."""
    list_changed: bool = Field(alias="listChanged")


class ResourcesCapability(BaseModel):
    """Resources capability."""
    subscribe: bool
    list_changed: bool = Field(alias="listChanged")


class PromptsCapability(BaseModel):
    """Prompts capability."""
    list_changed: bool = Field(alias="listChanged")


class ServerCapabilities(BaseModel):
    """Server capabilities."""
    tools: Optional[ToolsCapability] = None
    resources: Optional[ResourcesCapability] = None
    prompts: Optional[PromptsCapability] = None
    sampling: Optional[Dict[str, Any]] = None
    logging: Optional[Dict[str, Any]] = None


class InitializeResult(BaseModel):
    """Initialize response."""
    protocol_version: str = Field(alias="protocolVersion")
    capabilities: ServerCapabilities
    server_info: ServerInfo = Field(alias="serverInfo")


class Tool(BaseModel):
    """Tool definition."""
    name: str
    description: str
    input_schema: Dict[str, Any] = Field(alias="inputSchema")


class ToolsListResult(BaseModel):
    """Tools list result."""
    tools: List[Tool]
    next_cursor: Optional[str] = Field(None, alias="nextCursor")


class TextContent(BaseModel):
    """Text content."""
    type: str = "text"
    text: str


class ImageContent(BaseModel):
    """Image content."""
    type: str = "image"
    data: str
    mime_type: str = Field(alias="mimeType")


class ResourceContent(BaseModel):
    """Resource content."""
    uri: str
    mime_type: Optional[str] = Field(None, alias="mimeType")
    text: Optional[str] = None


class ResourceContentBlock(BaseModel):
    """Resource content block."""
    type: str = "resource"
    resource: ResourceContent


Content = Union[TextContent, ImageContent, ResourceContentBlock]


class ToolCallResult(BaseModel):
    """Tool call result."""
    content: List[Content]
    is_error: bool = Field(False, alias="isError")


class Resource(BaseModel):
    """Resource definition."""
    uri: str
    name: str
    description: Optional[str] = None
    mime_type: Optional[str] = Field(None, alias="mimeType")


class ResourcesListResult(BaseModel):
    """Resources list result."""
    resources: List[Resource]
    next_cursor: Optional[str] = Field(None, alias="nextCursor")


class ResourceReadResult(BaseModel):
    """Resource read result."""
    contents: List[ResourceContent]


class PromptArgument(BaseModel):
    """Prompt argument."""
    name: str
    description: Optional[str] = None
    required: bool = False


class Prompt(BaseModel):
    """Prompt definition."""
    name: str
    description: Optional[str] = None
    arguments: Optional[List[PromptArgument]] = None


class PromptsListResult(BaseModel):
    """Prompts list result."""
    prompts: List[Prompt]
    next_cursor: Optional[str] = Field(None, alias="nextCursor")


class PromptMessage(BaseModel):
    """Prompt message."""
    role: str
    content: Content


class PromptGetResult(BaseModel):
    """Prompt get result."""
    description: str
    messages: List[PromptMessage]
