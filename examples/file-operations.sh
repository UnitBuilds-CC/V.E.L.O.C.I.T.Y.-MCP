#!/usr/bin/env bash
# File operations example
# This demonstrates using file_read, file_write, and shell_exec tools

set -e

echo "VELOCITY-MCP File Operations Example"
echo "====================================="
echo ""

# Create a temporary directory
TEMP_DIR=$(mktemp -d)
echo "Using temporary directory: $TEMP_DIR"
echo ""

# Start MCP server in stdio mode and run operations
../target/release/velocity_mcp --mode stdio <<MCP_SESSION
{"jsonrpc":"2.0","method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"file-ops-example","version":"1.0"}},"id":1}
{"jsonrpc":"2.0","method":"notifications/initialized"}

{"jsonrpc":"2.0","method":"tools/call","params":{"name":"file_write","arguments":{"path":"${TEMP_DIR}/test.txt","content":"Hello, VELOCITY-MCP!\nThis is a test file.\nLine 3 of the file."}},"id":2}

{"jsonrpc":"2.0","method":"tools/call","params":{"name":"file_read","arguments":{"path":"${TEMP_DIR}/test.txt"}},"id":3}

{"jsonrpc":"2.0","method":"tools/call","params":{"name":"shell_exec","arguments":{"command":"wc -l ${TEMP_DIR}/test.txt"}},"id":4}

{"jsonrpc":"2.0","method":"tools/call","params":{"name":"shell_exec","arguments":{"command":"cat ${TEMP_DIR}/test.txt | grep 'VELOCITY'"}},"id":5}

{"jsonrpc":"2.0","method":"tools/call","params":{"name":"file_write","arguments":{"path":"${TEMP_DIR}/processed.txt","content":"Processed content:\n- Original file had 3 lines\n- Found 'VELOCITY' in the file\n- Processing complete!"}},"id":6}

{"jsonrpc":"2.0","method":"tools/call","params":{"name":"file_read","arguments":{"path":"${TEMP_DIR}/processed.txt"}},"id":7}
MCP_SESSION

echo ""
echo "Cleaning up temporary files..."
rm -rf "$TEMP_DIR"

echo ""
echo "Example completed!"
