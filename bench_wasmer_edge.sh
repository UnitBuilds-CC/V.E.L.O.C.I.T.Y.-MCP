#!/bin/bash
# Benchmark Wasmer Edge MCP latency

EDGE_URL="https://velocity-mcp-edge.wasmer.app/mcp"

echo "=== Wasmer Edge MCP Latency Benchmark ==="
echo ""

# Function to measure latency
measure_latency() {
    local payload="$1"
    local label="$2"

    # curl returns time in seconds, multiply by 1000 for ms
    result=$(curl -s -o /dev/null -w "%{time_total}" -X POST "$EDGE_URL" \
        -H "Content-Type: application/json" \
        -d "$payload")
    # result is in seconds (e.g., 0.849), convert to ms
    latency_ms=$(awk "BEGIN {printf \"%.2f\", $result * 1000}")
    printf "%-30s %8s ms\n" "$label" "$latency_ms"
}

echo "First Call (Cold Start):"
echo "-----------------------------------"

measure_latency '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"bench","version":"1.0"}}}' "initialize"
measure_latency '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' "tools/list"
measure_latency '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"benchmark_echo","arguments":{"message":"test"}}}' "tool call (echo)"

echo ""
echo "Second Call (Warm):"
echo "-----------------------------------"

measure_latency '{"jsonrpc":"2.0","id":4,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"bench","version":"1.0"}}}' "initialize"
measure_latency '{"jsonrpc":"2.0","id":5,"method":"tools/list","params":{}}' "tools/list"
measure_latency '{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"benchmark_echo","arguments":{"message":"test2"}}}' "tool call (echo)"

echo ""
echo "Sustained Load (20 rapid calls):"
echo "-----------------------------------"

total=0
for i in $(seq 1 20); do
    result=$(curl -s -o /dev/null -w "%{time_total}" -X POST "$EDGE_URL" \
        -H "Content-Type: application/json" \
        -d '{"jsonrpc":"2.0","id":'"$i"',"method":"tools/call","params":{"name":"benchmark_echo","arguments":{"message":"iter'"$i"'"}}}')
    latency_ms=$(awk "BEGIN {printf \"%.2f\", $result * 1000}")
    printf "%-30s %8s ms\n" "call $i" "$latency_ms"
    total=$(awk "BEGIN {print $total + $latency_ms}")
done

avg=$(awk "BEGIN {printf \"%.2f\", $total / 20}")
echo "-----------------------------------"
printf "%-30s %8s ms\n" "Average" "$avg"
printf "%-30s %8s ms\n" "Total" "$total"
