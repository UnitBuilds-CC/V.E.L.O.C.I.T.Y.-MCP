#!/usr/bin/env python3
"""Test VELOCITY-MCP filesystem tools"""
import subprocess
import json
import sys

def send_request(proc, request):
    """Send JSON-RPC request and get response"""
    proc.stdin.write(json.dumps(request) + "\n")
    proc.stdin.flush()
    line = proc.stdout.readline()
    if line:
        return json.loads(line.strip())
    return None

# Start VELOCITY-MCP
print("Starting VELOCITY-MCP...")
env = {"RUST_LOG": "error"}
proc = subprocess.Popen(
    ["./target/release/velocity_mcp.exe", "--mode", "stdio"],
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    stderr=subprocess.DEVNULL,
    text=True,
    env=env
)

# Initialize
print("Initializing...")
resp = send_request(proc, {
    "jsonrpc": "2.0",
    "method": "initialize",
    "params": {"protocolVersion": "2024-11-05"},
    "id": 1
})
print(f"Server: {resp['result']['serverInfo']['name']} v{resp['result']['serverInfo']['version']}")

send_request(proc, {"jsonrpc": "2.0", "method": "notifications/initialized"})

# Test 1: list_directory
print("\n1. Testing list_directory...")
resp = send_request(proc, {
    "jsonrpc": "2.0",
    "method": "tools/call",
    "params": {
        "name": "list_directory",
        "arguments": {"path": "C:/tmp/velocity_test"}
    },
    "id": 2
})
if resp and 'result' in resp:
    content = resp['result']['content'][0]['text']
    print(f"✓ Success: {content[:200]}")
else:
    print(f"✗ Failed: {resp}")

# Test 2: search_files
print("\n2. Testing search_files...")
resp = send_request(proc, {
    "jsonrpc": "2.0",
    "method": "tools/call",
    "params": {
        "name": "search_files",
        "arguments": {"path": "C:/tmp/velocity_test", "pattern": "*.txt"}
    },
    "id": 3
})
if resp and 'result' in resp:
    content = resp['result']['content'][0]['text']
    print(f"✓ Success: {content}")
else:
    print(f"✗ Failed: {resp}")

# Test 3: get_file_info
print("\n3. Testing get_file_info...")
resp = send_request(proc, {
    "jsonrpc": "2.0",
    "method": "tools/call",
    "params": {
        "name": "get_file_info",
        "arguments": {"path": "C:/tmp/velocity_test/file1.txt"}
    },
    "id": 4
})
if resp and 'result' in resp:
    content = resp['result']['content'][0]['text']
    print(f"✓ Success: {content[:300]}")
else:
    print(f"✗ Failed: {resp}")

# Test 4: create_directory
print("\n4. Testing create_directory...")
resp = send_request(proc, {
    "jsonrpc": "2.0",
    "method": "tools/call",
    "params": {
        "name": "create_directory",
        "arguments": {"path": "C:/tmp/velocity_test/new_dir"}
    },
    "id": 5
})
if resp and 'result' in resp:
    content = resp['result']['content'][0]['text']
    print(f"✓ Success: {content}")
else:
    print(f"✗ Failed: {resp}")

# Test 5: edit_file (dry run)
print("\n5. Testing edit_file (dry run)...")
resp = send_request(proc, {
    "jsonrpc": "2.0",
    "method": "tools/call",
    "params": {
        "name": "edit_file",
        "arguments": {
            "path": "C:/tmp/velocity_test/file1.txt",
            "edits": [{"oldText": "Hello", "newText": "Goodbye"}],
            "dryRun": True
        }
    },
    "id": 6
})
if resp and 'result' in resp:
    content = resp['result']['content'][0]['text']
    print(f"✓ Success: {content}")
else:
    print(f"✗ Failed: {resp}")

proc.terminate()
proc.wait(timeout=5)
print("\nAll tests completed!")
