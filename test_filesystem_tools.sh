#!/bin/bash
# Test VELOCITY-MCP filesystem tools

echo "Testing VELOCITY-MCP Filesystem Tools"
echo "======================================"

# Create test directory structure
mkdir -p /tmp/velocity_test/subdir1/nested
mkdir -p /tmp/velocity_test/subdir2
echo "Hello World" > /tmp/velocity_test/file1.txt
echo "Test file 2" > /tmp/velocity_test/file2.log
echo "Nested file" > /tmp/velocity_test/subdir1/nested/deep.txt

# Helper function to send JSON-RPC request
send_request() {
    echo "$1" | RUST_LOG=error timeout 3 ./target/release/velocity_mcp.exe --mode stdio 2>/dev/null | tail -1
}

echo ""
echo "1. Testing list_directory..."
REQUEST='{"jsonrpc":"2.0","method":"initialize","params":{"protocolVersion":"2024-11-05"},"id":1}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","method":"tools/call","params":{"name":"list_directory","arguments":{"path":"/tmp/velocity_test"}},"id":2}'
send_request "$REQUEST"

echo ""
echo "2. Testing search_files..."
REQUEST='{"jsonrpc":"2.0","method":"initialize","params":{"protocolVersion":"2024-11-05"},"id":1}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","method":"tools/call","params":{"name":"search_files","arguments":{"path":"/tmp/velocity_test","pattern":"*.txt"}},"id":2}'
send_request "$REQUEST"

echo ""
echo "3. Testing get_file_info..."
REQUEST='{"jsonrpc":"2.0","method":"initialize","params":{"protocolVersion":"2024-11-05"},"id":1}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","method":"tools/call","params":{"name":"get_file_info","arguments":{"path":"/tmp/velocity_test/file1.txt"}},"id":2}'
send_request "$REQUEST"

echo ""
echo "4. Testing create_directory..."
REQUEST='{"jsonrpc":"2.0","method":"initialize","params":{"protocolVersion":"2024-11-05"},"id":1}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","method":"tools/call","params":{"name":"create_directory","arguments":{"path":"/tmp/velocity_test/new_dir"}},"id":2}'
send_request "$REQUEST"

echo ""
echo "5. Testing edit_file (dry run)..."
REQUEST='{"jsonrpc":"2.0","method":"initialize","params":{"protocolVersion":"2024-11-05"},"id":1}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","method":"tools/call","params":{"name":"edit_file","arguments":{"path":"/tmp/velocity_test/file1.txt","edits":[{"oldText":"Hello","newText":"Goodbye"}],"dryRun":true}},"id":2}'
send_request "$REQUEST"

echo ""
echo "All tests completed!"
