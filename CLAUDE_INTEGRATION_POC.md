# Claude Integration for CodexMonitor - Implementation

## Status: ✅ Complete

This repository contains a working implementation of Claude support for CodexMonitor via a drop-in replacement server called `claude-app-server`.

## What's Included

### 1. claude-app-server (`./claude-app-server/`)

A complete Rust implementation that:
- Implements the Codex app-server JSON-RPC protocol
- Uses Anthropic's Claude API (Messages API)
- Supports the full agentic loop (tool use → execute → continue)
- Provides 6 development tools: shell, read_file, write_file, edit_file, list_files, search_files

**Files:**
- `src/main.rs` - Server loop, request routing, thread management, agentic loop
- `src/protocol.rs` - JSON-RPC message types
- `src/claude.rs` - Anthropic API client with error handling
- `src/tools.rs` - Tool definitions and secure execution
- `Cargo.toml` - Dependencies
- `README.md` - Complete documentation

### 2. Integration Patch (`./codexmonitor-integration.patch`)

A patch file showing the minimal changes needed to CodexMonitor to support backend selection between Codex and Claude.

## Quick Start

```bash
# 1. Build the server
cd claude-app-server
cargo build --release

# 2. Set your API key
export ANTHROPIC_API_KEY="sk-ant-api03-..."

# 3. Run in any project directory
cd /path/to/your/project
/path/to/claude-app-server

# 4. Send JSON-RPC messages via stdin
{"id": 1, "method": "initialize", "params": {}}
{"id": 2, "method": "model/list", "params": {}}
{"id": 3, "method": "thread/start", "params": {"name": "Test"}}
{"id": 4, "method": "thread/sendMessage", "params": {"threadId": "...", "message": {"content": "Hello!"}}}
```

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        CodexMonitor                              │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │                    Frontend (React)                       │   │
│  │  - Workspace sidebar                                      │   │
│  │  - Message composer                                       │   │
│  │  - Model picker (now shows Claude models)                 │   │
│  │  - Tool call display                                      │   │
│  └──────────────────────────────────────────────────────────┘   │
│                              │ Tauri IPC                         │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │                    Backend (Rust)                         │   │
│  │  - Spawns backend process per workspace                   │   │
│  │  - Routes JSON-RPC messages                               │   │
│  │  - Backend selection: Codex OR Claude                     │   │
│  └──────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
         │                                    │
         ▼                                    ▼
┌─────────────────────┐            ┌─────────────────────┐
│  codex app-server   │            │  claude-app-server  │
│  (OpenAI backend)   │            │  (Anthropic backend)│
└─────────────────────┘            └─────────────────────┘
         │                                    │
         ▼                                    ▼
┌─────────────────────┐            ┌─────────────────────┐
│   OpenAI API        │            │   Anthropic API     │
│   (Responses API)   │            │   (Messages API)    │
└─────────────────────┘            └─────────────────────┘
```

## Protocol Compatibility

### Implemented Methods

| Method | Status | Notes |
|--------|--------|-------|
| `initialize` | ✅ | Returns capabilities |
| `model/list` | ✅ | Returns Claude models |
| `skills/list` | ✅ | Returns empty (not supported) |
| `thread/list` | ✅ | Lists in-memory threads |
| `thread/start` | ✅ | Creates new thread |
| `thread/resume` | ✅ | Returns thread history |
| `thread/archive` | ✅ | Removes thread |
| `thread/sendMessage` | ✅ | Full agentic loop |
| `thread/interrupt` | ⚠️ | Stub (needs cancellation) |
| `account/rateLimits` | ✅ | Returns mock data |

### Implemented Events

| Event | Status | Description |
|-------|--------|-------------|
| `codex/turnStarted` | ✅ | Emitted when processing begins |
| `codex/agentMessage` | ✅ | Emitted for text responses |
| `codex/toolCall` | ✅ | Emitted when tool is invoked |
| `codex/toolResult` | ✅ | Emitted after tool execution |
| `codex/error` | ✅ | Emitted on errors |
| `codex/turnCompleted` | ✅ | Emitted when turn finishes |

### Implemented Tools

| Tool | Description | Safety Features |
|------|-------------|-----------------|
| `shell` | Execute commands | Command filtering, timeout, output truncation |
| `read_file` | Read file contents | Path sandboxing, size limits |
| `write_file` | Write/create files | Path sandboxing, parent creation |
| `edit_file` | Surgical text edits | Exact match required, single occurrence |
| `list_files` | List directory | Depth limiting, ignore patterns |
| `search_files` | Grep-like search | Result limits, path sandboxing |

## Supported Models

| Model | ID | Default |
|-------|-----|---------|
| Claude Sonnet 4 | `claude-sonnet-4-20250514` | ✅ |
| Claude 3.7 Sonnet | `claude-3-7-sonnet-20250219` | |
| Claude 3.5 Sonnet | `claude-3-5-sonnet-20241022` | |
| Claude 3.5 Haiku | `claude-3-5-haiku-20241022` | |
| Claude 3 Opus | `claude-3-opus-20240229` | |

## Integration with CodexMonitor

### Option 1: Environment Variable (Simplest)

Set the binary path and CodexMonitor will use it:
```bash
export CLAUDE_APP_SERVER_BIN="/path/to/claude-app-server"
```

Then modify CodexMonitor's spawn logic to check this variable.

### Option 2: Apply Integration Patch

The included `codexmonitor-integration.patch` adds:
- `Backend` enum (Codex | Claude)
- Backend field to workspace settings
- Backend selector in settings UI
- Modified spawn logic

### Option 3: Fork CodexMonitor

Full fork with native Claude support and UI improvements.

## Key Implementation Details

### Agentic Loop

The server implements a full agentic loop:

```rust
for iteration in 0..MAX_TOOL_ITERATIONS {
    // 1. Build messages from thread history
    // 2. Call Claude API with tools
    // 3. Process response:
    //    - Emit text as agentMessage
    //    - For tool_use: execute tool, emit events
    // 4. If stop_reason == "tool_use": continue loop
    //    Else: break
}
```

This allows Claude to autonomously:
- Read files to understand context
- Make edits and verify them
- Run commands and handle errors
- Iterate until the task is complete

### Security

- **Path Sandboxing**: All file operations restricted to workspace
- **Command Filtering**: Blocks obviously dangerous commands
- **Output Truncation**: Large outputs truncated to prevent memory issues
- **Timeouts**: Shell commands have configurable timeouts

### Error Handling

- API errors mapped to specific user-friendly messages
- Tool errors returned to Claude for self-correction
- Network failures logged with context

## What's NOT Implemented

1. **Thread Persistence**: Threads are in-memory only (lost on restart)
2. **Streaming**: Uses non-streaming API (simpler, slightly higher latency)
3. **Skills System**: Codex's skills aren't supported (different paradigm)
4. **Approval Workflow**: No human-in-the-loop for dangerous operations
5. **Extended Thinking**: Not using Claude's thinking blocks yet
6. **Request Cancellation**: Interrupt is a stub

## Future Improvements

1. **Add streaming** for lower latency responses
2. **Persist threads** to disk for resume across restarts
3. **Implement approval workflow** for dangerous operations
4. **Add extended thinking** support for Claude 3.7+
5. **Better token tracking** for usage display
6. **WebSocket transport** option for non-stdio usage

## Testing

```bash
# Unit tests (no API needed)
cargo test

# Manual testing
./test_server.sh

# Interactive testing
cargo run --example interactive
```

## Lines of Code

```
claude-app-server/src/main.rs       703 lines  (server, routing, agentic loop)
claude-app-server/src/tools.rs      601 lines  (tool definitions, execution)
claude-app-server/src/claude.rs     275 lines  (API client)
claude-app-server/src/protocol.rs    90 lines  (JSON-RPC types)
─────────────────────────────────────────────
Total                              1669 lines
```

## License

MIT
