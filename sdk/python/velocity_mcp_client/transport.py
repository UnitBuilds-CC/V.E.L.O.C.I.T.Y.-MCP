"""Transport implementations for MCP client."""

import asyncio
import json
from abc import ABC, abstractmethod
from typing import Optional
import httpx

from .types import JsonRpcRequest, JsonRpcResponse
from .errors import TransportError, ConnectionClosedError


class Transport(ABC):
    """Abstract transport for MCP communication."""
    
    @abstractmethod
    async def send(self, request: JsonRpcRequest) -> JsonRpcResponse:
        """Send a JSON-RPC request and receive a response."""
        pass
    
    @abstractmethod
    async def close(self) -> None:
        """Close the transport connection."""
        pass


class HttpTransport(Transport):
    """HTTP transport - communicates via HTTP POST."""
    
    def __init__(self, url: str, api_key: Optional[str] = None, timeout: float = 30.0):
        """
        Initialize HTTP transport.
        
        Args:
            url: MCP server URL (e.g., "http://localhost:3000/mcp")
            api_key: Optional API key for authentication
            timeout: Request timeout in seconds
        """
        self.url = url
        self.api_key = api_key
        self.timeout = timeout
        self._client: Optional[httpx.AsyncClient] = None
    
    async def _get_client(self) -> httpx.AsyncClient:
        """Get or create HTTP client."""
        if self._client is None or self._client.is_closed:
            headers = {}
            if self.api_key:
                headers["Authorization"] = f"Bearer {self.api_key}"
            
            self._client = httpx.AsyncClient(
                timeout=httpx.Timeout(self.timeout),
                headers=headers,
            )
        return self._client
    
    async def send(self, request: JsonRpcRequest) -> JsonRpcResponse:
        """Send HTTP request."""
        try:
            client = await self._get_client()
            response = await client.post(
                self.url,
                json=request.model_dump(by_alias=True, exclude_none=True),
            )
            response.raise_for_status()
            return JsonRpcResponse.model_validate(response.json())
        except httpx.TimeoutException as e:
            raise TransportError(f"Request timed out: {e}")
        except httpx.HTTPStatusError as e:
            raise TransportError(f"HTTP error {e.response.status_code}: {e.response.text}")
        except Exception as e:
            raise TransportError(f"Transport error: {e}")
    
    async def close(self) -> None:
        """Close HTTP client."""
        if self._client and not self._client.is_closed:
            await self._client.aclose()


class StdioTransport(Transport):
    """Stdio transport - communicates via stdin/stdout of a subprocess."""
    
    def __init__(self, command: str, args: Optional[list[str]] = None):
        """
        Initialize stdio transport.
        
        Args:
            command: Command to run (e.g., "velocity_mcp")
            args: Command arguments (e.g., ["--mode", "stdio"])
        """
        self.command = command
        self.args = args or []
        self._process: Optional[asyncio.subprocess.Process] = None
        self._request_id = 0
    
    async def _ensure_process(self) -> asyncio.subprocess.Process:
        """Ensure subprocess is running."""
        if self._process is None or self._process.returncode is not None:
            self._process = await asyncio.create_subprocess_exec(
                self.command,
                *self.args,
                stdin=asyncio.subprocess.PIPE,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
            )
        return self._process
    
    async def send(self, request: JsonRpcRequest) -> JsonRpcResponse:
        """Send request via stdio."""
        try:
            process = await self._ensure_process()
            
            if process.stdin is None or process.stdout is None:
                raise ConnectionClosedError("Process stdin/stdout not available")
            
            # Send request
            request_json = request.model_dump_json(by_alias=True, exclude_none=True)
            process.stdin.write((request_json + "\n").encode())
            await process.stdin.drain()
            
            # Read response
            response_line = await process.stdout.readline()
            if not response_line:
                raise ConnectionClosedError("Process closed stdout")
            
            response_json = json.loads(response_line.decode())
            return JsonRpcResponse.model_validate(response_json)
            
        except Exception as e:
            raise TransportError(f"Stdio transport error: {e}")
    
    async def close(self) -> None:
        """Close subprocess."""
        if self._process and self._process.returncode is None:
            self._process.terminate()
            try:
                await asyncio.wait_for(self._process.wait(), timeout=5.0)
            except asyncio.TimeoutError:
                self._process.kill()
