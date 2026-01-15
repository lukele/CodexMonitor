//! Claude App Server - A JSON-RPC server compatible with CodexMonitor
//!
//! This server implements the same protocol as `codex app-server` but uses
//! Anthropic's Claude API instead of OpenAI.

use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

mod claude;
mod protocol;
mod tools;

use claude::{ApiMessage, ClaudeClient, ContentBlock};
use protocol::*;
use tools::ToolExecutor;

const PROTOCOL_VERSION: &str = "2.0";
const MAX_TOOL_ITERATIONS: usize = 25;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging to stderr (stdout is for JSON-RPC)
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("claude_app_server=info".parse().unwrap()),
        )
        .with_writer(std::io::stderr)
        .init();

    info!("Starting Claude App Server v{}", env!("CARGO_PKG_VERSION"));

    let server = Arc::new(Server::new().await?);
    let stdin = tokio::io::stdin();
    let stdout = Arc::new(Mutex::new(tokio::io::stdout()));

    let mut reader = BufReader::new(stdin).lines();

    while let Ok(Some(line)) = reader.next_line().await {
        if line.trim().is_empty() {
            continue;
        }

        debug!("Received: {}", line);

        match serde_json::from_str::<JSONRPCMessage>(&line) {
            Ok(msg) => {
                let server = Arc::clone(&server);
                let stdout = Arc::clone(&stdout);

                // Handle message and send responses
                let responses = server.handle_message(msg).await;
                let mut out = stdout.lock().await;
                for response in responses {
                    let json_str = serde_json::to_string(&response)?;
                    debug!("Sending: {}", json_str);
                    out.write_all(format!("{}\n", json_str).as_bytes()).await?;
                    out.flush().await?;
                }
            }
            Err(e) => {
                error!("Failed to parse JSON-RPC message: {}", e);
                let error_response = json!({
                    "error": {
                        "code": -32700,
                        "message": "Parse error",
                        "data": e.to_string()
                    }
                });
                let mut out = stdout.lock().await;
                out.write_all(format!("{}\n", error_response).as_bytes())
                    .await?;
                out.flush().await?;
            }
        }
    }

    info!("Claude App Server shutting down");
    Ok(())
}

struct Server {
    claude: ClaudeClient,
    threads: Mutex<HashMap<String, Thread>>,
    cwd: PathBuf,
    tool_executor: ToolExecutor,
}

impl Server {
    async fn new() -> Result<Self> {
        let api_key = env::var("ANTHROPIC_API_KEY")
            .context("ANTHROPIC_API_KEY environment variable not set")?;

        let cwd = env::current_dir().context("Failed to get current directory")?;
        info!("Working directory: {}", cwd.display());

        Ok(Self {
            claude: ClaudeClient::new(api_key),
            threads: Mutex::new(HashMap::new()),
            cwd: cwd.clone(),
            tool_executor: ToolExecutor::new(cwd),
        })
    }

    async fn handle_message(&self, msg: JSONRPCMessage) -> Vec<JSONRPCMessage> {
        match msg {
            JSONRPCMessage::Request { id, method, params } => {
                self.handle_request(id, &method, params.unwrap_or(json!({})))
                    .await
            }
            JSONRPCMessage::Notification { method, params } => {
                self.handle_notification(&method, params).await;
                vec![]
            }
            JSONRPCMessage::Response { .. } => {
                // We don't expect responses from the client
                vec![]
            }
        }
    }

    async fn handle_request(
        &self,
        id: RequestId,
        method: &str,
        params: Value,
    ) -> Vec<JSONRPCMessage> {
        info!("Handling request: {} (id: {})", method, id);

        let result = match method {
            "initialize" => self.handle_initialize().await,
            "model/list" => self.handle_model_list().await,
            "skills/list" => self.handle_skills_list().await,
            "thread/list" => self.handle_thread_list().await,
            "thread/start" => self.handle_thread_start(params).await,
            "thread/resume" => self.handle_thread_resume(params).await,
            "thread/archive" => self.handle_thread_archive(params).await,
            "thread/sendMessage" => return self.handle_send_message(id, params).await,
            "turn/start" => return self.handle_turn_start(id, params).await,
            "thread/interrupt" => self.handle_thread_interrupt(params).await,
            "turn/interrupt" => self.handle_thread_interrupt(params).await,
            "codex/respondToRequest" => self.handle_respond_to_request(params).await,
            "account/rateLimits" => self.handle_rate_limits().await,
            _ => {
                warn!("Unknown method: {}", method);
                Err(anyhow::anyhow!("Method not found: {}", method))
            }
        };

        match result {
            Ok(result) => vec![JSONRPCMessage::Response {
                id,
                result: Some(result),
                error: None,
            }],
            Err(e) => vec![JSONRPCMessage::Response {
                id,
                result: None,
                error: Some(json!({
                    "code": -32000,
                    "message": e.to_string()
                })),
            }],
        }
    }

    async fn handle_notification(&self, method: &str, _params: Option<Value>) {
        debug!("Handling notification: {}", method);
    }

    async fn handle_initialize(&self) -> Result<Value> {
        Ok(json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {
                "tools": true,
                "streaming": true,
                "skills": false
            },
            "serverInfo": {
                "name": "claude-app-server",
                "version": env!("CARGO_PKG_VERSION")
            }
        }))
    }

    async fn handle_model_list(&self) -> Result<Value> {
        Ok(json!({
            "data": get_claude_models()
        }))
    }

    async fn handle_skills_list(&self) -> Result<Value> {
        Ok(json!({ "skills": [] }))
    }

    async fn handle_thread_list(&self) -> Result<Value> {
        let threads = self.threads.lock().await;
        let thread_list: Vec<Value> = threads
            .values()
            .map(|t| {
                json!({
                    "id": t.id,
                    "name": t.name,
                    "cwd": t.cwd.to_string_lossy(),
                    "createdAt": t.created_at.timestamp_millis(),
                    "source": "app"
                })
            })
            .collect();

        Ok(json!({ "threads": thread_list }))
    }

    async fn handle_thread_start(&self, params: Value) -> Result<Value> {
        let thread_id = uuid::Uuid::new_v4().to_string();
        let name = params["name"]
            .as_str()
            .unwrap_or("New Thread")
            .to_string();

        let model = params["model"]
            .as_str()
            .unwrap_or("claude-sonnet-4-20250514")
            .to_string();

        info!("Starting new thread: {} with model: {}", thread_id, model);

        let thread = Thread {
            id: thread_id.clone(),
            name,
            cwd: self.cwd.clone(),
            messages: vec![],
            created_at: chrono::Utc::now(),
            model,
        };

        self.threads.lock().await.insert(thread_id.clone(), thread.clone());

        Ok(json!({ 
            "thread": {
                "id": thread_id,
                "name": thread.name,
                "createdAt": thread.created_at.to_rfc3339()
            }
        }))
    }

    async fn handle_thread_resume(&self, params: Value) -> Result<Value> {
        let thread_id = params["threadId"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing threadId"))?;

        let threads = self.threads.lock().await;
        let thread = threads
            .get(thread_id)
            .ok_or_else(|| anyhow::anyhow!("Thread not found"))?;

        // Convert thread history to items
        let items: Vec<Value> = thread
            .messages
            .iter()
            .enumerate()
            .map(|(i, msg)| {
                json!({
                    "id": format!("{}-{}", thread_id, i),
                    "type": "message",
                    "role": msg.role,
                    "content": [{"type": "text", "text": msg.get_text_content()}]
                })
            })
            .collect();

        Ok(json!({
            "threadId": thread_id,
            "items": items
        }))
    }

    async fn handle_thread_archive(&self, params: Value) -> Result<Value> {
        let thread_id = params["threadId"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing threadId"))?;

        self.threads.lock().await.remove(thread_id);
        info!("Archived thread: {}", thread_id);

        Ok(json!({ "success": true }))
    }

    async fn handle_send_message(
        &self,
        request_id: RequestId,
        params: Value,
    ) -> Vec<JSONRPCMessage> {
        let thread_id = match params["threadId"].as_str() {
            Some(id) => id.to_string(),
            None => {
                return vec![JSONRPCMessage::Response {
                    id: request_id,
                    result: None,
                    error: Some(json!({"code": -32000, "message": "Missing threadId"})),
                }];
            }
        };

        let message_content = params["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        info!(
            "Processing message for thread {}: {}...",
            thread_id,
            &message_content.chars().take(50).collect::<String>()
        );

        let mut responses = vec![];
        let turn_id = uuid::Uuid::new_v4().to_string();

        // Emit turn started
        responses.push(JSONRPCMessage::Notification {
            method: "codex/turnStarted".to_string(),
            params: Some(json!({
                "threadId": thread_id,
                "turnId": turn_id
            })),
        });

        // Get thread info and add user message
        let model = {
            let mut threads = self.threads.lock().await;
            let thread = match threads.get_mut(&thread_id) {
                Some(t) => t,
                None => {
                    responses.push(JSONRPCMessage::Response {
                        id: request_id,
                        result: None,
                        error: Some(json!({"code": -32000, "message": "Thread not found"})),
                    });
                    return responses;
                }
            };

            // Add user message
            thread.messages.push(ConversationMessage {
                role: "user".to_string(),
                content: vec![ContentBlock::Text {
                    text: message_content.clone(),
                }],
            });

            thread.model.clone()
        };

        // Run the agentic loop
        let agentic_responses = self
            .run_agentic_loop(&thread_id, &turn_id, &model)
            .await;
        responses.extend(agentic_responses);

        // Emit turn completed
        responses.push(JSONRPCMessage::Notification {
            method: "codex/turnCompleted".to_string(),
            params: Some(json!({
                "threadId": thread_id,
                "turnId": turn_id
            })),
        });

        // Send success response
        responses.push(JSONRPCMessage::Response {
            id: request_id,
            result: Some(json!({"success": true})),
            error: None,
        });

        responses
    }

    /// Handle turn/start - the v2 protocol for starting a turn
    async fn handle_turn_start(
        &self,
        request_id: RequestId,
        params: Value,
    ) -> Vec<JSONRPCMessage> {
        let thread_id = match params["threadId"].as_str() {
            Some(id) => id.to_string(),
            None => {
                return vec![JSONRPCMessage::Response {
                    id: request_id,
                    result: None,
                    error: Some(json!({"code": -32000, "message": "Missing threadId"})),
                }];
            }
        };

        // Extract user input from the "input" array
        let input = params["input"].as_array();
        let message_content = if let Some(inputs) = input {
            inputs
                .iter()
                .filter_map(|item| {
                    if item["type"].as_str() == Some("text") {
                        item["text"].as_str().map(|s| s.to_string())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            String::new()
        };

        if message_content.is_empty() {
            return vec![JSONRPCMessage::Response {
                id: request_id,
                result: None,
                error: Some(json!({"code": -32000, "message": "No text input provided"})),
            }];
        }

        info!(
            "turn/start for thread {}: {}...",
            thread_id,
            &message_content.chars().take(50).collect::<String>()
        );

        let mut responses = vec![];
        let turn_id = uuid::Uuid::new_v4().to_string();

        // Get thread info and add user message
        let model = {
            let mut threads = self.threads.lock().await;
            let thread = match threads.get_mut(&thread_id) {
                Some(t) => t,
                None => {
                    responses.push(JSONRPCMessage::Response {
                        id: request_id,
                        result: None,
                        error: Some(json!({"code": -32000, "message": "Thread not found"})),
                    });
                    return responses;
                }
            };

            // Override model if provided
            if let Some(m) = params["model"].as_str() {
                thread.model = m.to_string();
            }

            // Add user message
            thread.messages.push(ConversationMessage {
                role: "user".to_string(),
                content: vec![ContentBlock::Text {
                    text: message_content.clone(),
                }],
            });

            thread.model.clone()
        };

        // Send success response with turn object
        responses.push(JSONRPCMessage::Response {
            id: request_id,
            result: Some(json!({
                "turn": {
                    "id": turn_id,
                    "items": [],
                    "status": "inProgress"
                }
            })),
            error: None,
        });

        // Send turn/started notification
        responses.push(JSONRPCMessage::Notification {
            method: "turn/started".to_string(),
            params: Some(json!({
                "turn": {
                    "id": turn_id,
                    "threadId": thread_id,
                    "status": "inProgress"
                }
            })),
        });

        // Run the agentic loop
        let agentic_responses = self
            .run_agentic_loop(&thread_id, &turn_id, &model)
            .await;
        responses.extend(agentic_responses);

        // Emit turn/completed notification
        responses.push(JSONRPCMessage::Notification {
            method: "turn/completed".to_string(),
            params: Some(json!({
                "turn": {
                    "id": turn_id,
                    "threadId": thread_id,
                    "status": "completed"
                }
            })),
        });

        responses
    }

    /// Run the agentic loop: call Claude, execute tools, repeat until done
    async fn run_agentic_loop(
        &self,
        thread_id: &str,
        turn_id: &str,
        model: &str,
    ) -> Vec<JSONRPCMessage> {
        let mut responses = vec![];
        let tools = self.tool_executor.get_tool_definitions();
        let system_prompt = self.get_system_prompt();

        for iteration in 0..MAX_TOOL_ITERATIONS {
            debug!("Agentic loop iteration {} for thread {}", iteration, thread_id);

            // Build messages from thread history
            let api_messages = {
                let threads = self.threads.lock().await;
                let thread = match threads.get(thread_id) {
                    Some(t) => t,
                    None => {
                        error!("Thread {} not found during agentic loop", thread_id);
                        break;
                    }
                };

                thread
                    .messages
                    .iter()
                    .map(|m| ApiMessage {
                        role: m.role.clone(),
                        content: m.content.clone(),
                    })
                    .collect::<Vec<_>>()
            };

            // Call Claude API
            let response = match self
                .claude
                .send_message(model, api_messages, tools.clone(), &system_prompt)
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    error!("Claude API error: {}", e);
                    responses.push(JSONRPCMessage::Notification {
                        method: "codex/error".to_string(),
                        params: Some(json!({
                            "threadId": thread_id,
                            "turnId": turn_id,
                            "error": {
                                "code": "api_error",
                                "message": e.to_string()
                            }
                        })),
                    });
                    break;
                }
            };

            // Process response content
            let mut has_tool_use = false;
            let mut assistant_content: Vec<ContentBlock> = vec![];
            let mut tool_results: Vec<ContentBlock> = vec![];

            for block in &response.content {
                match block {
                    ContentBlock::Text { text } => {
                        assistant_content.push(block.clone());

                        // Emit agent message
                        responses.push(JSONRPCMessage::Notification {
                            method: "codex/agentMessage".to_string(),
                            params: Some(json!({
                                "threadId": thread_id,
                                "turnId": turn_id,
                                "message": {
                                    "type": "text",
                                    "content": text
                                }
                            })),
                        });
                    }
                    ContentBlock::ToolUse { id, name, input } => {
                        has_tool_use = true;
                        assistant_content.push(block.clone());

                        info!("Tool call: {} with id {}", name, id);

                        // Emit tool call event
                        responses.push(JSONRPCMessage::Notification {
                            method: "codex/toolCall".to_string(),
                            params: Some(json!({
                                "threadId": thread_id,
                                "turnId": turn_id,
                                "toolCall": {
                                    "id": id,
                                    "name": name,
                                    "input": input
                                }
                            })),
                        });

                        // Execute the tool
                        let result = self.tool_executor.execute(name, input).await;
                        let result_str = serde_json::to_string_pretty(&result).unwrap_or_default();
                        let is_error = result.get("error").is_some();

                        info!(
                            "Tool result for {}: {}",
                            id,
                            &result_str.chars().take(100).collect::<String>()
                        );

                        // Emit tool result event
                        responses.push(JSONRPCMessage::Notification {
                            method: "codex/toolResult".to_string(),
                            params: Some(json!({
                                "threadId": thread_id,
                                "turnId": turn_id,
                                "toolResult": {
                                    "id": id,
                                    "result": result,
                                    "isError": is_error
                                }
                            })),
                        });

                        // Add to tool results for next iteration
                        tool_results.push(ContentBlock::ToolResult {
                            tool_use_id: id.clone(),
                            content: result_str,
                            is_error: if is_error { Some(true) } else { None },
                        });
                    }
                    _ => {}
                }
            }

            // Save assistant message to thread
            {
                let mut threads = self.threads.lock().await;
                if let Some(thread) = threads.get_mut(thread_id) {
                    thread.messages.push(ConversationMessage {
                        role: "assistant".to_string(),
                        content: assistant_content,
                    });

                    // If there were tool uses, add tool results as user message
                    if !tool_results.is_empty() {
                        thread.messages.push(ConversationMessage {
                            role: "user".to_string(),
                            content: tool_results,
                        });
                    }
                }
            }

            // Check stop reason
            let should_continue = has_tool_use && response.stop_reason.as_deref() == Some("tool_use");

            if !should_continue {
                debug!(
                    "Stopping agentic loop: stop_reason={:?}, has_tool_use={}",
                    response.stop_reason, has_tool_use
                );
                break;
            }
        }

        responses
    }

    async fn handle_thread_interrupt(&self, params: Value) -> Result<Value> {
        let thread_id = params["threadId"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing threadId"))?;

        info!("Interrupt requested for thread: {}", thread_id);
        // TODO: Implement proper cancellation with CancellationToken
        Ok(json!({ "success": true }))
    }

    async fn handle_respond_to_request(&self, params: Value) -> Result<Value> {
        let request_id = params["requestId"].as_u64();
        debug!("Respond to request: {:?}", request_id);
        Ok(json!({ "success": true }))
    }

    async fn handle_rate_limits(&self) -> Result<Value> {
        Ok(json!({
            "primary": null,
            "secondary": null,
            "credits": {
                "hasCredits": true,
                "unlimited": false,
                "balance": null
            },
            "planType": "api"
        }))
    }

    fn get_system_prompt(&self) -> String {
        format!(
            r#"You are Claude, an expert AI coding assistant created by Anthropic. You help users with software development tasks in their local workspace.

Current working directory: {}

You have access to tools for:
- Executing shell commands (shell)
- Reading file contents (read_file)
- Writing files (write_file)
- Editing files with surgical precision (edit_file)
- Listing directory contents (list_files)
- Searching for text in files (search_files)

Guidelines:
1. ALWAYS explore and understand the codebase before making changes
2. Use read_file to examine existing code structure and patterns
3. Make minimal, focused changes that follow existing code style
4. Use edit_file for surgical edits to existing files (safer than write_file)
5. Use shell to run tests, builds, and verify your changes work
6. Explain your reasoning and what you're doing
7. Be careful with destructive operations - prefer creating backups

When writing code:
- Follow the language's conventions and existing project patterns
- Add helpful comments for complex logic
- Handle errors appropriately
- Consider edge cases

You can make multiple tool calls to accomplish complex tasks. Work step by step."#,
            self.cwd.display()
        )
    }
}

#[derive(Clone)]
struct Thread {
    id: String,
    name: String,
    cwd: PathBuf,
    messages: Vec<ConversationMessage>,
    created_at: chrono::DateTime<chrono::Utc>,
    model: String,
}

#[derive(Clone)]
struct ConversationMessage {
    role: String,
    content: Vec<ContentBlock>,
}

impl ConversationMessage {
    fn get_text_content(&self) -> String {
        self.content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn get_claude_models() -> Vec<Value> {
    vec![
        json!({
            "id": "claude-sonnet-4-20250514",
            "model": "claude-sonnet-4-20250514",
            "displayName": "Claude Sonnet 4",
            "description": "Most intelligent model, best for complex coding tasks",
            "supportedReasoningEfforts": [
                {"reasoningEffort": "default", "description": "Standard reasoning"}
            ],
            "defaultReasoningEffort": "default",
            "isDefault": true
        }),
        json!({
            "id": "claude-3-7-sonnet-20250219",
            "model": "claude-3-7-sonnet-20250219",
            "displayName": "Claude 3.7 Sonnet",
            "description": "Fast and capable with extended thinking",
            "supportedReasoningEfforts": [
                {"reasoningEffort": "default", "description": "Standard reasoning"}
            ],
            "defaultReasoningEffort": "default",
            "isDefault": false
        }),
        json!({
            "id": "claude-3-5-sonnet-20241022",
            "model": "claude-3-5-sonnet-20241022",
            "displayName": "Claude 3.5 Sonnet",
            "description": "Fast and capable for most coding tasks",
            "supportedReasoningEfforts": [
                {"reasoningEffort": "default", "description": "Standard reasoning"}
            ],
            "defaultReasoningEffort": "default",
            "isDefault": false
        }),
        json!({
            "id": "claude-3-5-haiku-20241022",
            "model": "claude-3-5-haiku-20241022",
            "displayName": "Claude 3.5 Haiku",
            "description": "Fastest model, good for simple tasks",
            "supportedReasoningEfforts": [
                {"reasoningEffort": "default", "description": "Standard reasoning"}
            ],
            "defaultReasoningEffort": "default",
            "isDefault": false
        }),
        json!({
            "id": "claude-3-opus-20240229",
            "model": "claude-3-opus-20240229",
            "displayName": "Claude 3 Opus",
            "description": "Previous generation flagship model",
            "supportedReasoningEfforts": [
                {"reasoningEffort": "default", "description": "Standard reasoning"}
            ],
            "defaultReasoningEffort": "default",
            "isDefault": false
        }),
    ]
}
