# CodexMonitor + Claude

![CodexMonitor](screenshot.png)

CodexMonitor is a macOS Tauri app for orchestrating AI coding agents across local workspaces. This fork adds support for **Anthropic Claude** models alongside OpenAI Codex.

## Features

### Original Features (from CodexMonitor)
- Add and persist workspaces using the system folder picker
- Spawn one AI agent per workspace and stream events over JSON-RPC
- Restore threads per workspace from history
- Start agent threads, send messages, show reasoning/tool call items, and handle approvals
- Worktree agents per workspace (create/delete git worktrees)
- Git panel with diff stats, file diffs, and commit log
- Model picker, reasoning effort selector, access mode control
- Plan panel for per-turn planning updates and turn interruption controls
- Debug panel for warning/error events

### New: Claude Support 🟣
- **Seamless model switching**: Select a Claude model and the backend automatically switches
- **All Claude models supported**: Sonnet 4, 3.7 Sonnet, 3.5 Sonnet, 3.5 Haiku, and more
- **Same great UI**: Claude models appear alongside Codex models in the model picker
- **Grouped model list**: Models are organized by provider (Claude/Codex) for easy selection

## How It Works

When you select a model:
1. If it's a Claude model (`claude-*`), the app switches to `claude-app-server`
2. If it's a Codex model (`gpt-*`), the app uses `codex app-server`
3. The switch happens automatically - just pick your model and go!

## Requirements

- Node.js + npm
- Rust toolchain (stable)
- For Claude models: `claude-app-server` binary and `ANTHROPIC_API_KEY`
- For Codex models: `codex` CLI and OpenAI authentication

## Setup

### 1. Install Dependencies

```bash
npm install
```

### 2. Build claude-app-server (for Claude support)

```bash
cd claude-app-server
cargo build --release
export PATH="$PATH:$(pwd)/target/release"
```

### 3. Set API Keys

```bash
# For Claude models
export ANTHROPIC_API_KEY="sk-ant-..."

# For Codex models (handled by Codex CLI)
# Run `codex` to authenticate
```

### 4. Run the App

```bash
npm run tauri dev
```

## Usage

1. **Add a workspace**: Click "Add Workspace" and select a directory
2. **Select a model**: Use the model dropdown in the composer
   - 🟣 Claude models (Anthropic)
   - 🟢 Codex models (OpenAI)
3. **Send messages**: Type your request and hit Send
4. **Watch it work**: See tool calls, file changes, and responses in real-time

## Model Selection

The model picker shows all available models grouped by provider:

| Provider | Models |
|----------|--------|
| **🟣 Claude (Anthropic)** | Sonnet 4, 3.7 Sonnet, 3.5 Sonnet, 3.5 Haiku |
| **🟢 Codex (OpenAI)** | GPT-5.2 Codex, GPT-5.1 Codex Max |

Selecting a model from a different provider automatically switches the backend.

## Project Structure

```
.
├── src/                    # Frontend (React + TypeScript)
│   ├── components/         # UI components
│   ├── hooks/              # React hooks (including useModels)
│   ├── services/           # Tauri IPC calls
│   └── types.ts            # TypeScript types
├── src-tauri/              # Backend (Rust + Tauri)
│   └── src/
│       ├── codex.rs        # Backend switching logic
│       ├── types.rs        # Backend enum, workspace types
│       └── workspaces.rs   # Workspace management
└── claude-app-server/      # Claude backend server
    └── src/
        ├── main.rs         # JSON-RPC server
        ├── claude.rs       # Anthropic API client
        └── tools.rs        # Tool execution
```

## Key Changes from Original

1. **Backend enum**: `Backend::Codex | Backend::Claude` tracks which AI provider to use
2. **Model-based switching**: Selecting a Claude model triggers automatic backend switch
3. **Combined model list**: `get_all_models` returns models from both providers
4. **Grouped UI**: Model picker shows models organized by provider

## Development

```bash
# Run in dev mode
npm run tauri dev

# Type check frontend
npm run typecheck

# Build for production
npm run tauri build
```

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                     CodexMonitor UI                              │
│  ┌─────────────────────────────────────────────────────────────┐│
│  │ Model Picker: [🟣 Claude Sonnet 4 ▼]                        ││
│  └─────────────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────────┘
                              │
                    Model Selection
                              │
         ┌────────────────────┴────────────────────┐
         │                                         │
         ▼                                         ▼
┌─────────────────────┐               ┌─────────────────────┐
│  claude-app-server  │               │  codex app-server   │
│  (claude-* models)  │               │  (gpt-* models)     │
└─────────────────────┘               └─────────────────────┘
         │                                         │
         ▼                                         ▼
┌─────────────────────┐               ┌─────────────────────┐
│   Anthropic API     │               │    OpenAI API       │
└─────────────────────┘               └─────────────────────┘
```

## License

MIT

## Credits

- Original [CodexMonitor](https://github.com/Dimillian/CodexMonitor) by Thomas Ricouard
- [Codex CLI](https://github.com/openai/codex) by OpenAI
- [Claude](https://anthropic.com/claude) by Anthropic
