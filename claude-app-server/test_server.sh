#!/bin/bash
# Test script for claude-app-server
# Usage: ./test_server.sh

set -e

# Check for API key
if [ -z "$ANTHROPIC_API_KEY" ]; then
    echo "Error: ANTHROPIC_API_KEY environment variable not set"
    exit 1
fi

# Build the server
echo "Building claude-app-server..."
cargo build --release

SERVER="./target/release/claude-app-server"

# Function to send JSON-RPC message and read response
send_message() {
    local msg="$1"
    echo "$msg" | $SERVER 2>/dev/null | head -1
}

echo ""
echo "=== Testing claude-app-server ==="
echo ""

# Test 1: Initialize
echo "Test 1: Initialize"
response=$(echo '{"id": 1, "method": "initialize", "params": {}}' | timeout 5 $SERVER 2>/dev/null | head -1)
echo "Response: $response"
echo ""

# Test 2: Model list
echo "Test 2: Model list"
response=$(echo -e '{"id": 1, "method": "initialize", "params": {}}\n{"id": 2, "method": "model/list", "params": {}}' | timeout 5 $SERVER 2>/dev/null | tail -1)
echo "Response: $response"
echo ""

# Test 3: Thread start
echo "Test 3: Thread start"
response=$(echo -e '{"id": 1, "method": "initialize", "params": {}}\n{"id": 2, "method": "thread/start", "params": {"name": "Test Thread"}}' | timeout 5 $SERVER 2>/dev/null | tail -1)
echo "Response: $response"
echo ""

echo "=== Basic tests completed ==="
echo ""
echo "For interactive testing, run:"
echo "  RUST_LOG=debug ./target/release/claude-app-server"
echo ""
echo "Then paste JSON-RPC messages like:"
echo '  {"id": 1, "method": "initialize", "params": {}}'
echo '  {"id": 2, "method": "model/list", "params": {}}'
