#!/bin/bash
# Development run script for CodexMonitor with Claude support via pi-adapter

set -e

echo "🚀 CodexMonitor + Claude (via pi) Development Setup"
echo "===================================================="
echo ""

# Check for Anthropic API key
if [ -z "$ANTHROPIC_API_KEY" ]; then
    echo "⚠️  Warning: ANTHROPIC_API_KEY not set. Claude models won't work."
    echo "   Set it with: export ANTHROPIC_API_KEY='sk-ant-...'"
    echo ""
fi

# Build pi-adapter if needed
PI_ADAPTER_DIR="./pi-adapter"
if [ -d "$PI_ADAPTER_DIR" ] && [ ! -f "$PI_ADAPTER_DIR/dist/index.js" ]; then
    echo "📦 Building pi-adapter..."
    cd "$PI_ADAPTER_DIR"
    npm install
    npm run build
    cd ..
    echo "✅ pi-adapter built successfully"
    echo ""
fi

# Set pi-adapter path
if [ -f "$PI_ADAPTER_DIR/dist/index.js" ]; then
    export PI_ADAPTER_BIN="$(pwd)/$PI_ADAPTER_DIR/dist/index.js"
    echo "✅ Using pi-adapter: $PI_ADAPTER_BIN"
fi

# Check if pi is available
if command -v pi &> /dev/null; then
    echo "✅ pi CLI found: $(which pi)"
    export PI_BIN="$(which pi)"
else
    echo "⚠️  Warning: pi CLI not found. Install from: npm install -g @mariozechner/pi-coding-agent"
fi

# Check if codex is available (for Codex models)
if command -v codex &> /dev/null; then
    echo "✅ Codex CLI found: $(which codex)"
else
    echo "ℹ️  Codex CLI not found (only needed for OpenAI Codex models)"
fi

echo ""
echo "📋 Environment:"
echo "   ANTHROPIC_API_KEY: ${ANTHROPIC_API_KEY:+set}${ANTHROPIC_API_KEY:-not set}"
echo "   PI_ADAPTER_BIN: ${PI_ADAPTER_BIN:-not set}"
echo "   PI_BIN: ${PI_BIN:-not set}"
echo ""

# Run the app
echo "🎬 Starting CodexMonitor..."
echo ""
npm run tauri dev
