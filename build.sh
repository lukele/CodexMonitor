#!/bin/bash
# Build script for CodexMonitor with Claude support

set -e

echo "🏗️  Building CodexMonitor + Claude"
echo "=================================="
echo ""

# Build claude-app-server
echo "📦 Building claude-app-server..."
cd claude-app-server
cargo build --release
cd ..
echo "✅ claude-app-server built"
echo ""

# Build the Tauri app
echo "📦 Building CodexMonitor..."
npm run tauri build
echo ""

echo "✅ Build complete!"
echo ""
echo "📍 Outputs:"
echo "   claude-app-server: ./claude-app-server/target/release/claude-app-server"
echo "   CodexMonitor app:  ./src-tauri/target/release/bundle/macos/"
echo ""
echo "📋 Installation:"
echo "   1. Copy claude-app-server to ~/.local/bin/ or /usr/local/bin/"
echo "   2. Set ANTHROPIC_API_KEY environment variable"
echo "   3. Run the CodexMonitor app"
