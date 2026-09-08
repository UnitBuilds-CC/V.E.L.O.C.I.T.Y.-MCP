#!/bin/bash
set -e
cd "$(dirname "$0")"
go build -o go_tool go_tool.go
echo "Built: go_tool ($(wc -c < go_tool) bytes)"
