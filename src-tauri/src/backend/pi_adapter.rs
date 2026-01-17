use serde_json::{json, Value};
use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{oneshot, Mutex};
use tokio::time::timeout;

use crate::backend::events::{AppServerEvent, EventSink};
use crate::types::WorkspaceEntry;

pub(crate) struct PiSession {
    pub(crate) entry: WorkspaceEntry,
    pub(crate) child: Mutex<Child>,
    pub(crate) stdin: Mutex<ChildStdin>,
    pub(crate) pending: Mutex<HashMap<u64, oneshot::Sender<Value>>>,
    pub(crate) next_id: AtomicU64,
}

impl PiSession {
    async fn write_message(&self, value: Value) -> Result<(), String> {
        let mut stdin = self.stdin.lock().await;
        let mut line = serde_json::to_string(&value).map_err(|e| e.to_string())?;
        line.push('\n');
        stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| e.to_string())
    }

    pub(crate) async fn send_request(&self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        self.write_message(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
            .await?;
        rx.await.map_err(|_| "request canceled".to_string())
    }

    pub(crate) async fn send_notification(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> Result<(), String> {
        let value = if let Some(params) = params {
            json!({ "jsonrpc": "2.0", "method": method, "params": params })
        } else {
            json!({ "jsonrpc": "2.0", "method": method })
        };
        self.write_message(value).await
    }

    pub(crate) async fn send_response(&self, id: u64, result: Value) -> Result<(), String> {
        self.write_message(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
            .await
    }
}

/// Check if pi-adapter is available via PI_ADAPTER_BIN env var
pub(crate) fn get_pi_adapter_bin() -> Option<String> {
    env::var("PI_ADAPTER_BIN").ok().filter(|s| !s.is_empty())
}

/// Check if pi-adapter should be used (env var is set)
pub(crate) fn should_use_pi_adapter() -> bool {
    get_pi_adapter_bin().is_some()
}

pub(crate) async fn spawn_pi_session<E: EventSink>(
    entry: WorkspaceEntry,
    event_sink: E,
) -> Result<Arc<PiSession>, String> {
    let adapter_bin = get_pi_adapter_bin()
        .ok_or("PI_ADAPTER_BIN environment variable not set")?;
    
    let mut command = Command::new("node");
    command.arg(&adapter_bin);
    command.current_dir(&entry.path);
    
    // Pass through environment variables for API keys
    if let Ok(key) = env::var("ANTHROPIC_API_KEY") {
        command.env("ANTHROPIC_API_KEY", key);
    }
    if let Ok(key) = env::var("OPENAI_API_KEY") {
        command.env("OPENAI_API_KEY", key);
    }
    if let Ok(key) = env::var("OPENCODE_API_KEY") {
        command.env("OPENCODE_API_KEY", key);
    }
    if let Ok(key) = env::var("MISTRAL_API_KEY") {
        command.env("MISTRAL_API_KEY", key);
    }
    if let Ok(key) = env::var("GOOGLE_API_KEY") {
        command.env("GOOGLE_API_KEY", key);
    }
    if let Ok(path) = env::var("PI_MONOREPO") {
        command.env("PI_MONOREPO", path);
    }
    
    command.stdin(std::process::Stdio::piped());
    command.stdout(std::process::Stdio::piped());
    command.stderr(std::process::Stdio::piped());

    let mut child = command.spawn().map_err(|e| format!("Failed to spawn pi-adapter: {}", e))?;
    let stdin = child.stdin.take().ok_or("missing stdin")?;
    let stdout = child.stdout.take().ok_or("missing stdout")?;
    let stderr = child.stderr.take().ok_or("missing stderr")?;

    let session = Arc::new(PiSession {
        entry: entry.clone(),
        child: Mutex::new(child),
        stdin: Mutex::new(stdin),
        pending: Mutex::new(HashMap::new()),
        next_id: AtomicU64::new(1),
    });

    let session_clone = Arc::clone(&session);
    let workspace_id = entry.id.clone();
    let event_sink_clone = event_sink.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if line.trim().is_empty() {
                continue;
            }
            let value: Value = match serde_json::from_str(&line) {
                Ok(value) => value,
                Err(err) => {
                    let payload = AppServerEvent {
                        workspace_id: workspace_id.clone(),
                        message: json!({
                            "method": "codex/parseError",
                            "params": { "error": err.to_string(), "raw": line },
                        }),
                    };
                    event_sink_clone.emit_app_server_event(payload);
                    continue;
                }
            };

            let maybe_id = value.get("id").and_then(|id| id.as_u64());
            let has_method = value.get("method").is_some();
            let has_result_or_error = value.get("result").is_some() || value.get("error").is_some();
            if let Some(id) = maybe_id {
                if has_result_or_error {
                    if let Some(tx) = session_clone.pending.lock().await.remove(&id) {
                        let _ = tx.send(value);
                    }
                } else if has_method {
                    let payload = AppServerEvent {
                        workspace_id: workspace_id.clone(),
                        message: value,
                    };
                    event_sink_clone.emit_app_server_event(payload);
                } else if let Some(tx) = session_clone.pending.lock().await.remove(&id) {
                    let _ = tx.send(value);
                }
            } else if has_method {
                let payload = AppServerEvent {
                    workspace_id: workspace_id.clone(),
                    message: value,
                };
                event_sink_clone.emit_app_server_event(payload);
            }
        }
    });

    let workspace_id = entry.id.clone();
    let event_sink_clone = event_sink.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if line.trim().is_empty() {
                continue;
            }
            let payload = AppServerEvent {
                workspace_id: workspace_id.clone(),
                message: json!({
                    "method": "codex/stderr",
                    "params": { "message": line },
                }),
            };
            event_sink_clone.emit_app_server_event(payload);
        }
    });

    // Wait for connected notification (with timeout)
    let init_result = timeout(
        Duration::from_secs(15),
        session.send_request("initialize", json!({})),
    )
    .await;
    
    match init_result {
        Ok(Ok(_)) => {},
        Ok(Err(e)) => {
            let mut child = session.child.lock().await;
            let _ = child.kill().await;
            return Err(format!("Pi adapter initialize failed: {}", e));
        }
        Err(_) => {
            let mut child = session.child.lock().await;
            let _ = child.kill().await;
            return Err("Pi adapter did not respond to initialize (timeout)".to_string());
        }
    }

    let payload = AppServerEvent {
        workspace_id: entry.id.clone(),
        message: json!({
            "method": "codex/connected",
            "params": { "workspaceId": entry.id.clone() }
        }),
    };
    event_sink.emit_app_server_event(payload);

    Ok(session)
}
