"""Notification handling at the client/transport boundary.

The server answers a JSON-RPC notification with an empty 204 over HTTP and with no
line at all over stdio, so a notification has no envelope to decode. Both are
successes, and an id-bearing request that comes back with no envelope is a failure
that must be reported as one.
"""

import asyncio

from velocity_mcp_client.client import McpClient
from velocity_mcp_client.errors import TransportError
from velocity_mcp_client.transport import Transport
from velocity_mcp_client.types import JsonRpcRequest, JsonRpcResponse


class SilentTransport(Transport):
    """Returns no envelope for anything, the way a 204 does."""

    async def send(self, request: JsonRpcRequest):
        return None

    async def close(self) -> None:
        return None


class NotificationAwareTransport(Transport):
    """Replies to id-bearing requests and stays silent for notifications."""

    def __init__(self):
        self.requests = []

    async def send(self, request: JsonRpcRequest):
        self.requests.append(request)
        if request.id is None:
            return None
        return JsonRpcResponse(
            result={
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "serverInfo": {"name": "stub", "version": "3.2.0"},
            },
            id=request.id,
        )

    async def close(self) -> None:
        return None


def test_request_without_envelope_is_a_transport_error():
    client = McpClient(SilentTransport())
    try:
        asyncio.run(client.list_tools())
    except TransportError as error:
        assert "No response envelope for request 1" in str(error)
        return
    raise AssertionError("a request answered with no envelope reported success")


def test_initialize_completes_when_the_notification_has_no_envelope():
    transport = NotificationAwareTransport()
    result = asyncio.run(McpClient(transport).initialize())

    assert result.protocol_version == "2024-11-05"
    notifications = [r for r in transport.requests if r.id is None]
    assert len(notifications) == 1
    assert notifications[0].method == "notifications/initialized"
