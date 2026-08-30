#!/usr/bin/env bash
# HTTP server mode example
# This demonstrates running VELOCITY-MCP as an HTTP server

set -e

echo "Starting VELOCITY-MCP in HTTP mode on port 3000..."
echo ""

# Start the server in HTTP mode (runs in background)
../target/release/velocity_mcp --mode http --addr 127.0.0.1:3000 &
SERVER_PID=$!

# Wait for server to start
sleep 2

echo "Server started with PID $SERVER_PID"
echo ""

# Test health endpoint
echo "Testing health endpoint..."
curl -s http://127.0.0.1:3000/health | jq .
echo ""

# Test performance endpoint
echo "Testing performance endpoint..."
curl -s http://127.0.0.1:3000/performance | jq .
echo ""

# Test MCP endpoint
echo "Testing MCP tools/list..."
curl -s -X POST http://127.0.0.1:3000/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"tools/list","id":1}' | jq .
echo ""

# Test file_read tool
echo "Testing file_read tool..."
curl -s -X POST http://127.0.0.1:3000/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"tools/call","params":{"name":"file_read","arguments":{"path":"README.md"}},"id":2}' | jq .
echo ""

# Test shell_exec tool
echo "Testing shell_exec tool..."
curl -s -X POST http://127.0.0.1:3000/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"tools/call","params":{"name":"shell_exec","arguments":{"command":"echo Hello from HTTP!"}},"id":3}' | jq .
echo ""

# Cleanup
echo "Stopping server..."
kill $SERVER_PID 2>/dev/null || true

echo ""
echo "Example completed!"
