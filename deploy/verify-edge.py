#!/usr/bin/env python3
import argparse
import json
import os
import subprocess
import urllib.error
import urllib.request


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def main():
    parser = argparse.ArgumentParser(description="Verify live Edge HTTP and MCP tool execution")
    parser.add_argument("url", nargs="?", help="Base URL; defaults to the app in app.yaml")
    parser.add_argument("--version-id", help="Require this x-edge-app-version-id")
    args = parser.parse_args()
    if not args.url:
        app = subprocess.run(["wasmer", "app", "get", "-f", "json"],
                             check=True, capture_output=True, text=True, timeout=30)
        args.url = json.loads(app.stdout)["url"]
    opener = urllib.request.build_opener(NoRedirect())

    def request(path, body=None, status=200):
        headers = {"Content-Type": "application/json"}
        if body is not None and os.environ.get("VELOCITY_API_KEY"):
            headers["X-API-Key"] = os.environ["VELOCITY_API_KEY"]
        req = urllib.request.Request(args.url.rstrip("/") + path, data=body, headers=headers)
        try:
            response = opener.open(req, timeout=30)
        except urllib.error.HTTPError as error:
            response = error
        with response:
            if response.status != status:
                raise RuntimeError(f"{path}: expected HTTP {status}, got {response.status}")
            if args.version_id and response.headers.get("x-edge-app-version-id") != args.version_id:
                raise RuntimeError("Response came from a different Edge version")
            return json.loads(response.read(1_048_577))

    def rpc(method, params, request_id):
        response = request("/mcp", json.dumps({"jsonrpc": "2.0", "id": request_id,
                                               "method": method, "params": params}).encode())
        if response.get("jsonrpc") != "2.0" or response.get("id") != request_id or "error" in response:
            raise RuntimeError(f"{method}: invalid JSON-RPC result or request id")
        return response["result"]

    if request("/health").get("status") != "healthy":
        raise RuntimeError("Health response is not healthy")
    print("PASS health")
    initialized = rpc("initialize", {"protocolVersion": "2024-11-05", "capabilities": {},
                                   "clientInfo": {"name": "edge-verifier", "version": "1"}}, 1)
    if initialized["serverInfo"]["name"] != "velocity-mcp-edge":
        raise RuntimeError("Unexpected MCP server")
    print("PASS initialize")
    tools = rpc("tools/list", {}, "tools-list")["tools"]
    if len(tools) != 10 or "echo" not in {tool["name"] for tool in tools}:
        raise RuntimeError("Expected ten Edge tools including echo")
    print("PASS tools/list and response correlation")
    for index in range(3):
        message = f"edge-verifier-{index}"
        result = rpc("tools/call", {"name": "echo", "arguments": {"message": message}}, index + 2)
        if result.get("isError") or result["content"] != [{"type": "text", "text": message}]:
            raise RuntimeError("Echo did not return the submitted message")
    print("PASS three repeated tool calls")
    if "error" not in request("/mcp", b"{", status=400):
        raise RuntimeError("Malformed JSON was not rejected")
    print("PASS malformed JSON rejection")


if __name__ == "__main__":
    main()
