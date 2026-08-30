#!/usr/bin/env bash
# Basic stdio mode example
# This demonstrates the simplest way to use VELOCITY-MCP

set -e

echo "Starting VELOCITY-MCP in stdio mode..."
echo ""

# Start the server in stdio mode
../target/release/velocity_mcp --mode stdio <<'MCP_SESSION'
{"jsonrpc":"2.0","method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"example-client","version":"1.0"}},"id":1}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","method":"tools/list","id":2}
{"jsonrpc":"2.0","method":"tools/call","params":{"name":"file_read","arguments":{"path":"README.md"}},"id":3}
{"jsonrpc":"2.0","method":"tools/call","params":{"name":"shell_exec","arguments":{"command":"echo 'Hello from VELOCITY-MCP!'"}},"id":4}
MCP_SESSION

echo ""
echo "Example completed!"
