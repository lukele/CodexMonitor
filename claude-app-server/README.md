# Claude App Server

A JSON-RPC server that implements the Codex app-server protocol using Anthropic's Claude API. This allows CodexMonitor (and other tools designed for OpenAI's Codex CLI) to work with Claude models instead.

## Overview

```
┌─────────────────────────────────────────────────────────────┐
│                     CodexMonitor                             │
│                   (or any compatible UI)                     │
└─────────────────────┬───────────────────────────────────────┘
                      │ JSON-RPC over stdio
                      ▼
┌─────────────────────────────────────────────────────────────┐
│                   claude-app-server                          │
│  ┌─────────────┐  ┌─────────────┐  ┌────────────────────┐   │
│  │ JSON-RPC    │  │   Claude    │  │  Tool Execution    │   │
│  │ Protocol    │──│   Client    │──│  (shell, files)    │   │
│  └─────────────┘  └─────────────┘  └────────────────────┘   │
└─────────────────────┬───────────────────────────────────────┘
                      │ HTTPS
                      ▼
┌─────────────────────────────────────────────────────────────┐
│                  Anthropic Claude API                        │
└─────────────────────────────────────────────────────────────┘
```

## Features

- **Full Protocol Compatibility**: Implements the Codex app-server JSON-RPC protocol
- **Agentic Loop**: Automatically continues execution when Claude requests tool use
- **Multiple Models**: Support for Claude Sonnet 4, Claude 3.7 Sonnet, Claude 3.5 Sonnet, Claude 3.5 Haiku, and Claude 3 Opus
- **Rich Tooling**: Shell execution, file operations, search, and surgical edits
- **Safety Features**: Path sandboxing, command filtering, output truncation

## Installation

### Prerequisites

- Rust toolchain 1.70+ (`rustup.rs`)
- Anthropic API key

### Build from Source

```bash
# Clone the repository
git clone https://github.com/yourusername/claude-app-server.git
cd claude-app-server

# Build release binary
cargo build --release

# Install to PATH (optional)
cp target/release/claude-app-server ~/.local/bin/
# or
sudo cp target/release/claude-app-server /usr/local/bin/
```

### Environment Setup

```bash
# Required: Set your Anthropic API key
export ANTHROPIC_API_KEY="sk-ant-api03-..."

# Optional: Custom max tokens (default: 16384)
export CLAUDE_MAX_TOKENS=8192

# Optional: Debug logging
export RUST_LOG=debug
```

## Usage

### Standalone

```bash
# Navigate to your project directory
cd /path/to/your/project

# Start the server
claude-app-server
```

The server reads JSON-RPC messages from stdin and writes responses to stdout.

### With CodexMonitor

1. Build and install claude-app-server
2. Set `ANTHROPIC_API_KEY` environment variable
3. Option A: Set `CLAUDE_APP_SERVER_BIN` to point to the binary
4. Option B: Apply the integration patch (see `codexmonitor-integration.patch`)
5. Select "Claude" as the backend in workspace settings

### Example Session

```bash
# Start server (logs go to stderr)
$ claude-app-server 2>/dev/null

# Send initialize request
{"id": 1, "method": "initialize", "params": {}}

# Response
{"id":1,"result":{"protocolVersion":"2.0","capabilities":{"tools":true,"streaming":true,"skills":false},"serverInfo":{"name":"claude-app-server","version":"0.1.0"}}}

# List available models
{"id": 2, "method": "model/list", "params": {}}

# Start a new thread
{"id": 3, "method": "thread/start", "params": {"name": "My Task", "model": "claude-sonnet-4-20250514"}}

# Send a message (triggers agentic loop)
{"id": 4, "method": "thread/sendMessage", "params": {"threadId": "<thread-id>", "message": {"content": "List all Rust files in this directory"}}}
```

## Protocol Reference

### Methods

| Method | Description |
|--------|-------------|
| `initialize` | Initialize the server, returns capabilities |
| `model/list` | List available Claude models |
| `skills/list` | List skills (returns empty for Claude) |
| `thread/list` | List conversation threads |
| `thread/start` | Start a new thread |
| `thread/resume` | Resume an existing thread |
| `thread/archive` | Archive (delete) a thread |
| `thread/sendMessage` | Send a user message |
| `thread/interrupt` | Interrupt current generation |
| `account/rateLimits` | Get rate limit info |

### Events (Notifications)

| Event | Description |
|-------|-------------|
| `codex/turnStarted` | A new turn has started |
| `codex/agentMessage` | Assistant text response |
| `codex/toolCall` | Tool invocation request |
| `codex/toolResult` | Tool execution result |
| `codex/error` | An error occurred |
| `codex/turnCompleted` | Turn has completed |

### Tools

| Tool | Description |
|------|-------------|
| `shell` | Execute shell commands |
| `read_file` | Read file contents |
| `write_file` | Write/create files |
| `edit_file` | Surgical text replacement |
| `list_files` | List directory contents |
| `search_files` | Search for text patterns |

## Available Models

| Model ID | Description |
|----------|-------------|
| `claude-sonnet-4-20250514` | Latest, most capable (default) |
| `claude-3-7-sonnet-20250219` | Fast with extended thinking |
| `claude-3-5-sonnet-20241022` | Balanced speed and capability |
| `claude-3-5-haiku-20241022` | Fastest, for simple tasks |
| `claude-3-opus-20240229` | Previous flagship |

## Configuration

| Variable | Description | Default |
|----------|-------------|---------|
| `ANTHROPIC_API_KEY` | API key (required) | - |
| `CLAUDE_MAX_TOKENS` | Max response tokens | 16384 |
| `RUST_LOG` | Log level | info |

## Architecture

```
src/
├── main.rs       # Server loop, request routing, agentic loop
├── protocol.rs   # JSON-RPC message types
├── claude.rs     # Anthropic API client
└── tools.rs      # Tool definitions and execution
```

### Agentic Loop

When Claude responds with tool_use, the server:
1. Executes the requested tool
2. Sends the result back to Claude
3. Continues until Claude responds without tool_use or max iterations reached

This allows Claude to autonomously complete multi-step tasks like:
- Reading multiple files to understand a codebase
- Making changes and running tests
- Iterating on errors

## Differences from Codex

| Feature | Codex | Claude App Server |
|---------|-------|-------------------|
| Models | GPT-5.x | Claude 3.x/4 |
| Reasoning Effort | Configurable | Not applicable |
| Skills | Supported | Not supported |
| Rate Limits | OpenAI-style | Anthropic-style |
| Authentication | OpenAI/ChatGPT login | API key only |

## Development

```bash
# Run with debug logging
RUST_LOG=debug cargo run

# Run tests
cargo test

# Run integration tests (requires build)
cargo test --test integration_test -- --ignored

# Format code
cargo fmt

# Lint
cargo clippy
```

## Troubleshooting

### "ANTHROPIC_API_KEY environment variable not set"
```bash
export ANTHROPIC_API_KEY="sk-ant-api03-..."
```

### "Authentication failed"
- Verify your API key is correct
- Check the key hasn't expired
- Ensure you have API access enabled

### "Rate limit exceeded"
- Wait a few minutes and retry
- Consider using a smaller model (Haiku)
- Check your Anthropic usage limits

### Tool execution errors
- Check file permissions
- Verify paths are within the workspace
- Review stderr for detailed errors

## License

MIT

## Contributing

Contributions welcome! Please:
1. Fork the repository
2. Create a feature branch
3. Add tests for new functionality
4. Submit a pull request

## Related Projects

- [CodexMonitor](https://github.com/Dimillian/CodexMonitor) - The original Codex orchestration UI
- [Codex CLI](https://github.com/openai/codex) - OpenAI's Codex CLI
- [Anthropic Claude](https://anthropic.com/claude) - Claude AI models
