#!/bin/bash
# Compare native vs Edge MCP latency

NATIVE_URL="http://localhost:3000/mcp"
EDGE_URL="https://velocity-mcp-edge.wasmer.app/mcp"

echo "=== Native vs Wasmer Edge MCP Comparison ==="
echo ""

# Function to measure latency
measure_latency() {
    local url="$1"
    local payload="$2"
    local label="$3"

    result=$(curl -s -o /dev/null -w "%{time_total}" -X POST "$url" \
        -H "Content-Type: application/json" \
        -d "$payload" 2>/dev/null)
    latency_ms=$(awk "BEGIN {printf \"%.3f\", $result * 1000}")
    printf "%-40s %10s ms\n" "$label" "$latency_ms"
}

echo "Native (localhost):"
echo "-------------------------------------------"
measure_latency "$NATIVE_URL" '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"bench","version":"1.0"}}}' "initialize"
measure_latency "$NATIVE_URL" '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' "tools/list"
measure_latency "$NATIVE_URL" '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"bench_echo","arguments":{"message":"test"}}}' "tools/call (bench_echo)"

echo ""
echo "Wasmer Edge (us-ashburn):"
echo "-------------------------------------------"
measure_latency "$EDGE_URL" '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"bench","version":"1.0"}}}' "initialize"
measure_latency "$EDGE_URL" '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' "tools/list"
measure_latency "$EDGE_URL" '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"edge_ping","arguments":{}}}' "tools/call (edge_ping) - NOT SUPPORTED"

echo ""
echo "Speedup Factor:"
echo "-------------------------------------------"

# Measure native
native_result=$(curl -s -o /dev/null -w "%{time_total}" -X POST "$NATIVE_URL" \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"bench_echo","arguments":{"message":"test"}}}' 2>/dev/null)
native_ms=$(awk "BEGIN {printf \"%.3f\", $native_result * 1000}")

# Measure edge
edge_result=$(curl -s -o /dev/null -w "%{time_total}" -X POST "$EDGE_URL" \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}' 2>/dev/null)
edge_ms=$(awk "BEGIN {printf \"%.3f\", $edge_result * 1000}")

speedup=$(awk "BEGIN {printf \"%.1f\", $edge_ms / $native_ms}")

printf "Native tools/call:              %10s ms\n" "$native_ms"
printf "Edge tools/list:                %10s ms\n" "$edge_ms"
printf "Speedup (native vs edge):       %10sx\n" "$speedup"
echo ""
echo "NOTE: Edge doesn't support tools/call, so this comparison"
echo "      is native tool execution vs edge tool listing only."
