"""MCP client implementation."""

from typing import Any, Dict, List, Optional
import itertools

from .transport import Transport
from .types import (
    JsonRpcRequest,
    InitializeParams,
    ClientCapabilities,
    ClientInfo,
    InitializeResult,
    Tool,
    ToolsListResult,
    ToolCallResult,
    Resource,
    ResourcesListResult,
    ResourceReadResult,
    Prompt,
    PromptsListResult,
    PromptGetResult,
)
from .errors import ProtocolError, ToolExecutionError, ResourceNotFoundError, PromptNotFoundError, ServerError, TransportError


PROTOCOL_VERSION = "2024-11-05"
CLIENT_NAME = "velocity-mcp-python"
CLIENT_VERSION = "3.2.0"


class McpClient:
    """MCP client for communicating with VELOCITY-MCP servers."""
    
    def __init__(self, transport: Transport):
        """
        Initialize MCP client.
        
        Args:
            transport: Transport implementation (HttpTransport or StdioTransport)
        """
        self.transport = transport
        self._request_id = itertools.count(1)
        self._initialized = False
    
    def _next_id(self) -> int:
        """Get next request ID."""
        return next(self._request_id)
    
    async def _send_request(self, method: str, params: Optional[Dict[str, Any]] = None) -> Any:
        """Send JSON-RPC request and return result."""
        request = JsonRpcRequest(
            method=method,
            params=params,
            id=self._next_id(),
        )
        
        response = await self.transport.send(request)

        # _send_request always stamps an id, so a missing envelope means the server
        # answered this request like a notification.
        if response is None:
            raise TransportError(f"No response envelope for request {request.id}")

        if response.error:
            raise ProtocolError(
                code=response.error.code,
                message=response.error.message,
                data=response.error.data,
            )
        
        return response.result
    
    async def initialize(self) -> InitializeResult:
        """
        Initialize the MCP connection.
        
        Returns:
            InitializeResult with server capabilities and info
        """
        params = InitializeParams(
            protocolVersion=PROTOCOL_VERSION,
            capabilities=ClientCapabilities(),
            clientInfo=ClientInfo(name=CLIENT_NAME, version=CLIENT_VERSION),
        )
        
        result = await self._send_request("initialize", params.model_dump(by_alias=True))
        init_result = InitializeResult.model_validate(result)
        
        # Send initialized notification
        await self.transport.send(JsonRpcRequest(method="notifications/initialized"))
        
        self._initialized = True
        return init_result
    
    @property
    def is_initialized(self) -> bool:
        """Check if client is initialized."""
        return self._initialized
    
    async def list_tools(self) -> List[Tool]:
        """
        List available tools.
        
        Returns:
            List of Tool definitions
        """
        result = await self._send_request("tools/list", {})
        tools_result = ToolsListResult.model_validate(result)
        return tools_result.tools
    
    async def call_tool(self, name: str, arguments: Dict[str, Any]) -> ToolCallResult:
        """
        Call a tool with the given arguments.
        
        Args:
            name: Tool name
            arguments: Tool arguments
            
        Returns:
            ToolCallResult with content
            
        Raises:
            ToolExecutionError: If tool execution fails
        """
        params = {"name": name, "arguments": arguments}
        result = await self._send_request("tools/call", params)
        tool_result = ToolCallResult.model_validate(result)
        
        if tool_result.is_error:
            error_msg = "\n".join(
                content.text for content in tool_result.content
                if hasattr(content, "text")
            )
            raise ToolExecutionError(error_msg)
        
        return tool_result
    
    async def list_resources(self) -> List[Resource]:
        """
        List available resources.
        
        Returns:
            List of Resource definitions
        """
        result = await self._send_request("resources/list", {})
        resources_result = ResourcesListResult.model_validate(result)
        return resources_result.resources
    
    async def read_resource(self, uri: str) -> ResourceReadResult:
        """
        Read a resource by URI.
        
        Args:
            uri: Resource URI
            
        Returns:
            ResourceReadResult with contents
            
        Raises:
            ResourceNotFoundError: If resource not found
            ServerError: If the URI resolves but cannot be read
        """
        try:
            result = await self._send_request("resources/read", {"uri": uri})
        except ProtocolError as e:
            # velocity-mcp-core, which Edge dispatches through, reports an
            # unservable URI as a JSON-RPC -32001 error object.
            if e.code == -32001:
                raise ResourceNotFoundError(e.message) from None
            raise
        # The native server reports the same condition in band, with no error object,
        # so its message text is the only cause signal. Both servers say
        # "Resource not found: <uri>" for an unservable URI (src/resources.rs,
        # crates/velocity-mcp-core/src/lib.rs); anything else is a read failure.
        if isinstance(result, dict) and result.get("isError"):
            text = (result.get("content") or [{}])[0].get("text", "")
            if text.startswith("Resource not found"):
                raise ResourceNotFoundError(text)
            raise ServerError(text or "Resource read failed: %s" % uri)
        return ResourceReadResult.model_validate(result)
    
    async def list_prompts(self) -> List[Prompt]:
        """
        List available prompts.
        
        Returns:
            List of Prompt definitions
        """
        result = await self._send_request("prompts/list", {})
        prompts_result = PromptsListResult.model_validate(result)
        return prompts_result.prompts
    
    async def get_prompt(self, name: str, arguments: Dict[str, Any]) -> PromptGetResult:
        """
        Get a prompt with the given arguments.
        
        Args:
            name: Prompt name
            arguments: Prompt arguments
            
        Returns:
            PromptGetResult with messages
            
        Raises:
            PromptNotFoundError: If prompt not found
            ServerError: If the prompt exists but its arguments are rejected
        """
        try:
            params = {"name": name, "arguments": arguments}
            result = await self._send_request("prompts/get", params)
        except ProtocolError as e:
            # velocity-mcp-core, which Edge dispatches through, reports an
            # unknown prompt name as a JSON-RPC -32001 error object.
            if e.code == -32001:
                raise PromptNotFoundError(e.message) from None
            raise
        # The native server reports the same condition in band, with no error object,
        # so its message text is the only cause signal. Both servers say
        # "Prompt not found: <name>" for an unknown name; a rejected argument set
        # is a different failure and is not relabelled as a missing prompt.
        if isinstance(result, dict) and result.get("isError"):
            text = (result.get("content") or [{}])[0].get("text", "")
            if text.startswith("Prompt not found"):
                raise PromptNotFoundError(text)
            raise ServerError(text or "Prompt get failed: %s" % name)
        return PromptGetResult.model_validate(result)
    
    async def ping(self) -> None:
        """Ping the server."""
        await self._send_request("ping", {})
    
    async def set_log_level(self, level: str) -> None:
        """
        Set the logging level.
        
        Args:
            level: Log level (debug, info, warn, error)
        """
        await self._send_request("logging/setLevel", {"level": level})
    
    async def close(self) -> None:
        """Close the client connection."""
        await self.transport.close()
