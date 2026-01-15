use serde::Serialize;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::env;
use std::io::ErrorKind;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter, State};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{oneshot, Mutex};
use tokio::time::timeout;

use crate::state::AppState;
use crate::types::{Backend, WorkspaceEntry};

#[derive(Serialize, Clone)]
struct AppServerEvent {
    workspace_id: String,
    message: Value,
}

pub(crate) struct WorkspaceSession {
    pub(crate) entry: WorkspaceEntry,
    pub(crate) backend: Backend,
    pub(crate) child: Mutex<Child>,
    pub(crate) stdin: Mutex<ChildStdin>,
    pub(crate) pending: Mutex<HashMap<u64, oneshot::Sender<Value>>>,
    pub(crate) next_id: AtomicU64,
}

impl WorkspaceSession {
    async fn write_message(&self, value: Value) -> Result<(), String> {
        let mut stdin = self.stdin.lock().await;
        let mut line = serde_json::to_string(&value).map_err(|e| e.to_string())?;
        line.push('\n');
        stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| e.to_string())
    }

    async fn send_request(&self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        let msg = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        self.write_message(msg).await?;
        rx.await.map_err(|_| "request canceled".to_string())
    }

    async fn send_notification(&self, method: &str, params: Option<Value>) -> Result<(), String> {
        let value = if let Some(params) = params {
            json!({ "method": method, "params": params })
        } else {
            json!({ "method": method })
        };
        self.write_message(value).await
    }

    async fn send_response(&self, id: u64, result: Value) -> Result<(), String> {
        self.write_message(json!({ "id": id, "result": result }))
            .await
    }
}

fn build_codex_path_env(codex_bin: Option<&str>) -> Option<String> {
    let mut paths: Vec<String> = env::var("PATH")
        .unwrap_or_default()
        .split(':')
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .collect();
    let mut extras = vec![
        "/opt/homebrew/bin",
        "/usr/local/bin",
        "/usr/bin",
        "/bin",
        "/usr/sbin",
        "/sbin",
    ]
    .into_iter()
    .map(|value| value.to_string())
    .collect::<Vec<String>>();
    if let Ok(home) = env::var("HOME") {
        extras.push(format!("{home}/.local/bin"));
        extras.push(format!("{home}/.local/share/mise/shims"));
        extras.push(format!("{home}/.cargo/bin"));
        extras.push(format!("{home}/.bun/bin"));
        let nvm_root = Path::new(&home).join(".nvm/versions/node");
        if let Ok(entries) = std::fs::read_dir(nvm_root) {
            for entry in entries.flatten() {
                let bin_path = entry.path().join("bin");
                if bin_path.is_dir() {
                    extras.push(bin_path.to_string_lossy().to_string());
                }
            }
        }
    }
    if let Some(bin_path) = codex_bin.filter(|value| !value.trim().is_empty()) {
        let parent = Path::new(bin_path).parent();
        if let Some(parent) = parent {
            extras.push(parent.to_string_lossy().to_string());
        }
    }
    for extra in extras {
        if !paths.contains(&extra) {
            paths.push(extra);
        }
    }
    if paths.is_empty() {
        None
    } else {
        Some(paths.join(":"))
    }
}

fn build_codex_command_with_bin(codex_bin: Option<String>) -> Command {
    let bin = codex_bin
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "codex".into());
    let mut command = Command::new(bin);
    if let Some(path_env) = build_codex_path_env(codex_bin.as_deref()) {
        command.env("PATH", path_env);
    }
    command
}

async fn check_codex_installation(codex_bin: Option<String>) -> Result<Option<String>, String> {
    let mut command = build_codex_command_with_bin(codex_bin);
    command.arg("--version");
    command.stdout(std::process::Stdio::piped());
    command.stderr(std::process::Stdio::piped());

    let output = match timeout(Duration::from_secs(5), command.output()).await {
        Ok(result) => result.map_err(|e| {
            if e.kind() == ErrorKind::NotFound {
                "Codex CLI not found. Install Codex and ensure `codex` is on your PATH."
                    .to_string()
            } else {
                e.to_string()
            }
        })?,
        Err(_) => {
            return Err(
                "Timed out while checking Codex CLI. Make sure `codex --version` runs in Terminal."
                    .to_string(),
            );
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = if stderr.trim().is_empty() {
            stdout.trim()
        } else {
            stderr.trim()
        };
        if detail.is_empty() {
            return Err(
                "Codex CLI failed to start. Try running `codex --version` in Terminal."
                    .to_string(),
            );
        }
        return Err(format!(
            "Codex CLI failed to start: {detail}. Try running `codex --version` in Terminal."
        ));
    }

    let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(if version.is_empty() { None } else { Some(version) })
}

fn get_claude_app_server_bin() -> String {
    // Check for pi-adapter first, then claude-app-server, then fall back to node with pi-adapter
    if let Ok(bin) = env::var("PI_ADAPTER_BIN") {
        return bin;
    }
    if let Ok(bin) = env::var("CLAUDE_APP_SERVER_BIN") {
        return bin;
    }
    // Default to pi-adapter
    "pi-adapter".to_string()
}

fn build_claude_command() -> Command {
    let bin = get_claude_app_server_bin();

    
    // Check if it's a node script or binary
    let mut command = if bin.ends_with(".js") || bin.contains("pi-adapter/dist") {

        let mut cmd = Command::new("node");
        cmd.arg(&bin);
        cmd
    } else {

        Command::new(&bin)
    };
    
    // Add common paths to find the binary
    if let Some(path_env) = build_codex_path_env(Some(&bin)) {
        command.env("PATH", path_env);
    }
    
    // Pass through the Anthropic API key
    if let Ok(api_key) = env::var("ANTHROPIC_API_KEY") {
        command.env("ANTHROPIC_API_KEY", api_key);
    }
    
    // Pass through PI_BIN for the adapter to find pi
    if let Ok(pi_bin) = env::var("PI_BIN") {
        command.env("PI_BIN", pi_bin);
    }
    
    // Pass through PI_MONOREPO for the adapter to find pi from monorepo
    if let Ok(pi_monorepo) = env::var("PI_MONOREPO") {
        command.env("PI_MONOREPO", pi_monorepo);
    }
    
    command
}

async fn check_claude_installation() -> Result<Option<String>, String> {
    // Check if pi-adapter is available - it handles auth via pi's auth.json
    let adapter_bin = get_claude_app_server_bin();
    
    // For pi-adapter, we don't require ANTHROPIC_API_KEY - pi handles auth
    if adapter_bin.contains("pi-adapter") {
        return Ok(Some("pi-adapter".to_string()));
    }
    
    // For standalone claude-app-server, check for API key
    if env::var("ANTHROPIC_API_KEY").is_err() {
        return Err(
            "ANTHROPIC_API_KEY environment variable not set. Set it to use Claude models."
                .to_string(),
        );
    }

    // Check if pi is available
    let pi_bin = env::var("PI_BIN").unwrap_or_else(|_| "pi".to_string());
    let mut pi_check = Command::new(&pi_bin);
    pi_check.arg("--version");
    pi_check.stdout(std::process::Stdio::piped());
    pi_check.stderr(std::process::Stdio::piped());
    
    if let Ok(output) = pi_check.output().await {
        if output.status.success() {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
            return Ok(Some(format!("pi-adapter (pi {})", version)));
        }
    }

    // Fall back to checking claude-app-server
    let bin = get_claude_app_server_bin();
    let mut command = if bin.ends_with(".js") || bin.contains("pi-adapter/dist") {
        let mut cmd = Command::new("node");
        cmd.arg(&bin);
        cmd.arg("--help");
        cmd
    } else {
        let mut cmd = Command::new(&bin);
        cmd.arg("--help");
        cmd
    };
    command.stdout(std::process::Stdio::piped());
    command.stderr(std::process::Stdio::piped());
    
    // Add paths
    if let Some(path_env) = build_codex_path_env(Some(&bin)) {
        command.env("PATH", path_env);
    }

    let _output = match timeout(Duration::from_secs(5), command.output()).await {
        Ok(result) => result.map_err(|e| {
            if e.kind() == ErrorKind::NotFound {
                format!(
                    "claude-app-server not found. Install it and ensure it's on your PATH, or set CLAUDE_APP_SERVER_BIN."
                )
            } else {
                e.to_string()
            }
        })?,
        Err(_) => {
            return Err(
                "Timed out while checking claude-app-server."
                    .to_string(),
            );
        }
    };

    // claude-app-server --help returns success even if it shows help
    // We just need to know it exists
    Ok(Some("claude-app-server".to_string()))
}

fn build_backend_command(_backend: &Backend, _codex_bin: Option<String>) -> Command {
    // All backends now use pi-adapter
    build_claude_command()
}

pub(crate) async fn spawn_workspace_session(
    entry: WorkspaceEntry,
    default_codex_bin: Option<String>,
    backend: Backend,
    app_handle: AppHandle,
) -> Result<Arc<WorkspaceSession>, String> {
    let codex_bin = entry
        .codex_bin
        .clone()
        .filter(|value| !value.trim().is_empty())
        .or(default_codex_bin);
    
    // Check pi-adapter installation
    let _ = check_claude_installation().await?;

    let mut command = build_backend_command(&backend, codex_bin);
    command.current_dir(&entry.path);
    command.stdin(std::process::Stdio::piped());
    command.stdout(std::process::Stdio::piped());
    command.stderr(std::process::Stdio::piped());

    let mut child = command.spawn().map_err(|e| {
        format!("Failed to start pi-adapter: {}", e)
    })?;
    let stdin = child.stdin.take().ok_or("missing stdin")?;
    let stdout = child.stdout.take().ok_or("missing stdout")?;
    let stderr = child.stderr.take().ok_or("missing stderr")?;

    let session = Arc::new(WorkspaceSession {
        entry: entry.clone(),
        backend: backend.clone(),
        child: Mutex::new(child),
        stdin: Mutex::new(stdin),
        pending: Mutex::new(HashMap::new()),
        next_id: AtomicU64::new(1),
    });

    let session_clone = Arc::clone(&session);
    let workspace_id = entry.id.clone();
    let app_handle_clone = app_handle.clone();
    tauri::async_runtime::spawn(async move {
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
                    let _ = app_handle_clone.emit("app-server-event", payload);
                    continue;
                }
            };

            let maybe_id = value.get("id").and_then(|id| id.as_u64());
            let has_method = value.get("method").is_some();
            let has_result_or_error =
                value.get("result").is_some() || value.get("error").is_some();
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
                    let _ = app_handle_clone.emit("app-server-event", payload);
                } else if let Some(tx) = session_clone.pending.lock().await.remove(&id) {
                    let _ = tx.send(value);
                }
            } else if has_method {
                let payload = AppServerEvent {
                    workspace_id: workspace_id.clone(),
                    message: value,
                };
                let _ = app_handle_clone.emit("app-server-event", payload);
            }
        }
    });

    let workspace_id = entry.id.clone();
    let app_handle_clone = app_handle.clone();
    tauri::async_runtime::spawn(async move {
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
            let _ = app_handle_clone.emit("app-server-event", payload);
        }
    });

    // Give the stdout reader task a moment to start
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    let init_params = json!({
        "clientInfo": {
            "name": "codex_monitor",
            "title": "CodexMonitor",
            "version": "0.1.0"
        }
    });
    let init_result = timeout(
        Duration::from_secs(15),
        session.send_request("initialize", init_params),
    )
    .await;
    let init_response = match init_result {
        Ok(response) => response,
        Err(_) => {
            let mut child = session.child.lock().await;
            let _ = child.kill().await;
            return Err("Pi adapter did not respond to initialize. Check that pi-adapter is working and PI_ADAPTER_BIN or PI_MONOREPO is set.".to_string());
        }
    };
    init_response?;
    session.send_notification("initialized", None).await?;

    let payload = AppServerEvent {
        workspace_id: entry.id.clone(),
        message: json!({
            "method": "codex/connected",
            "params": { 
                "workspaceId": entry.id.clone(),
                "backend": "pi"
            }
        }),
    };
    let _ = app_handle.emit("app-server-event", payload);

    Ok(session)
}

#[tauri::command]
pub(crate) async fn codex_doctor(
    codex_bin: Option<String>,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let default_bin = {
        let settings = state.app_settings.lock().await;
        settings.codex_bin.clone()
    };
    let resolved = codex_bin
        .clone()
        .filter(|value| !value.trim().is_empty())
        .or(default_bin);
    let path_env = build_codex_path_env(resolved.as_deref());
    let version = check_codex_installation(resolved.clone()).await?;
    let mut command = build_codex_command_with_bin(resolved.clone());
    command.arg("app-server");
    command.arg("--help");
    command.stdout(std::process::Stdio::piped());
    command.stderr(std::process::Stdio::piped());
    let app_server_ok = match timeout(Duration::from_secs(5), command.output()).await {
        Ok(result) => result.map(|output| output.status.success()).unwrap_or(false),
        Err(_) => false,
    };
    let (node_ok, node_version, node_details) = {
        let mut node_command = Command::new("node");
        if let Some(ref path_env) = path_env {
            node_command.env("PATH", path_env);
        }
        node_command.arg("--version");
        node_command.stdout(std::process::Stdio::piped());
        node_command.stderr(std::process::Stdio::piped());
        match timeout(Duration::from_secs(5), node_command.output()).await {
            Ok(result) => match result {
                Ok(output) => {
                    if output.status.success() {
                        let version =
                            String::from_utf8_lossy(&output.stdout).trim().to_string();
                        (
                            !version.is_empty(),
                            if version.is_empty() { None } else { Some(version) },
                            None,
                        )
                    } else {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        let stdout = String::from_utf8_lossy(&output.stdout);
                        let detail = if stderr.trim().is_empty() {
                            stdout.trim()
                        } else {
                            stderr.trim()
                        };
                        (
                            false,
                            None,
                            Some(if detail.is_empty() {
                                "Node failed to start.".to_string()
                            } else {
                                detail.to_string()
                            }),
                        )
                    }
                }
                Err(err) => {
                    if err.kind() == ErrorKind::NotFound {
                        (false, None, Some("Node not found on PATH.".to_string()))
                    } else {
                        (false, None, Some(err.to_string()))
                    }
                }
            },
            Err(_) => (
                false,
                None,
                Some("Timed out while checking Node.".to_string()),
            ),
        }
    };
    let details = if app_server_ok {
        None
    } else {
        Some("Failed to run `codex app-server --help`.".to_string())
    };
    Ok(json!({
        "ok": version.is_some() && app_server_ok,
        "codexBin": resolved,
        "version": version,
        "appServerOk": app_server_ok,
        "details": details,
        "path": path_env,
        "nodeOk": node_ok,
        "nodeVersion": node_version,
        "nodeDetails": node_details,
    }))
}

#[tauri::command]
pub(crate) async fn start_thread(
    workspace_id: String,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let sessions = state.sessions.lock().await;
    let session = sessions
        .get(&workspace_id)
        .ok_or("workspace not connected")?;
    let params = json!({
        "cwd": session.entry.path,
        "approvalPolicy": "on-request"
    });
    session.send_request("thread/start", params).await
}

#[tauri::command]
pub(crate) async fn resume_thread(
    workspace_id: String,
    thread_id: String,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let sessions = state.sessions.lock().await;
    let session = sessions
        .get(&workspace_id)
        .ok_or("workspace not connected")?;
    let params = json!({
        "threadId": thread_id
    });
    session.send_request("thread/resume", params).await
}

#[tauri::command]
pub(crate) async fn list_threads(
    workspace_id: String,
    cursor: Option<String>,
    limit: Option<u32>,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let sessions = state.sessions.lock().await;
    let session = sessions
        .get(&workspace_id)
        .ok_or("workspace not connected")?;
    let params = json!({
        "cursor": cursor,
        "limit": limit,
    });
    session.send_request("thread/list", params).await
}

#[tauri::command]
pub(crate) async fn archive_thread(
    workspace_id: String,
    thread_id: String,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let sessions = state.sessions.lock().await;
    let session = sessions
        .get(&workspace_id)
        .ok_or("workspace not connected")?;
    let params = json!({
        "threadId": thread_id
    });
    session.send_request("thread/archive", params).await
}

#[tauri::command]
pub(crate) async fn send_user_message(
    workspace_id: String,
    thread_id: String,
    text: String,
    model: Option<String>,
    effort: Option<String>,
    access_mode: Option<String>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<Value, String> {
    // Determine which backend we need based on the model
    let required_backend = Backend::Pi;
    
    // Check if we need to switch backends
    {
        let sessions = state.sessions.lock().await;
        if let Some(session) = sessions.get(&workspace_id) {
            if session.backend != required_backend {

                drop(sessions);
                
                // Disconnect current session
                let mut sessions = state.sessions.lock().await;
                if let Some(old_session) = sessions.remove(&workspace_id) {
                    let mut child = old_session.child.lock().await;
                    let _ = child.kill().await;
                }
                drop(sessions);
                
                // Get workspace entry and reconnect with new backend
                let entry = {
                    let mut workspaces = state.workspaces.lock().await;
                    let entry = workspaces.get_mut(&workspace_id)
                        .ok_or("workspace not found")?;
                    entry.backend = required_backend.clone();
                    entry.clone()
                };
                
                let default_bin = {
                    let settings = state.app_settings.lock().await;
                    settings.codex_bin.clone()
                };
                
                let new_session = super::codex::spawn_workspace_session(
                    entry, default_bin, required_backend.clone(), app.clone()
                ).await?;
                
                state.sessions.lock().await.insert(workspace_id.clone(), new_session);

            }
        }
    }
    
    let sessions = state.sessions.lock().await;
    let session = sessions
        .get(&workspace_id)
        .ok_or("workspace not connected")?;
    let access_mode = access_mode.unwrap_or_else(|| "current".to_string());
    let sandbox_policy = match access_mode.as_str() {
        "full-access" => json!({
            "type": "dangerFullAccess"
        }),
        "read-only" => json!({
            "type": "readOnly"
        }),
        _ => json!({
            "type": "workspaceWrite",
            "writableRoots": [session.entry.path],
            "networkAccess": true
        }),
    };

    let approval_policy = if access_mode == "full-access" {
        "never"
    } else {
        "on-request"
    };

    let params = json!({
        "threadId": thread_id,
        "input": [{ "type": "text", "text": text }],
        "cwd": session.entry.path,
        "approvalPolicy": approval_policy,
        "sandboxPolicy": sandbox_policy,
        "model": model,
        "effort": effort,
    });
    session.send_request("turn/start", params).await
}

#[tauri::command]
pub(crate) async fn turn_interrupt(
    workspace_id: String,
    thread_id: String,
    turn_id: String,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let sessions = state.sessions.lock().await;
    let session = sessions
        .get(&workspace_id)
        .ok_or("workspace not connected")?;
    let params = json!({
        "threadId": thread_id,
        "turnId": turn_id,
    });
    session.send_request("turn/interrupt", params).await
}

#[tauri::command]
pub(crate) async fn start_review(
    workspace_id: String,
    thread_id: String,
    target: Value,
    delivery: Option<String>,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let sessions = state.sessions.lock().await;
    let session = sessions
        .get(&workspace_id)
        .ok_or("workspace not connected")?;
    let mut params = Map::new();
    params.insert("threadId".to_string(), json!(thread_id));
    params.insert("target".to_string(), target);
    if let Some(delivery) = delivery {
        params.insert("delivery".to_string(), json!(delivery));
    }
    session
        .send_request("review/start", Value::Object(params))
        .await
}

#[tauri::command]
pub(crate) async fn model_list(
    workspace_id: String,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let sessions = state.sessions.lock().await;
    let session = sessions
        .get(&workspace_id)
        .ok_or("workspace not connected")?;
    let params = json!({});
    session.send_request("model/list", params).await
}

#[tauri::command]
pub(crate) async fn account_rate_limits(
    workspace_id: String,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let sessions = state.sessions.lock().await;
    let session = sessions
        .get(&workspace_id)
        .ok_or("workspace not connected")?;
    session
        .send_request("account/rateLimits/read", Value::Null)
        .await
}

#[tauri::command]
pub(crate) async fn skills_list(
    workspace_id: String,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let sessions = state.sessions.lock().await;
    let session = sessions
        .get(&workspace_id)
        .ok_or("workspace not connected")?;
    let params = json!({
        "cwd": session.entry.path
    });
    session.send_request("skills/list", params).await
}

#[tauri::command]
pub(crate) async fn respond_to_server_request(
    workspace_id: String,
    request_id: u64,
    result: Value,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let sessions = state.sessions.lock().await;
    let session = sessions
        .get(&workspace_id)
        .ok_or("workspace not connected")?;
    session.send_response(request_id, result).await
}

/// Switch the backend for a workspace (now a no-op since all use Pi)
#[tauri::command]
pub(crate) async fn switch_backend(
    _workspace_id: String,
    _backend: String,
    _state: State<'_, AppState>,
    _app: AppHandle,
) -> Result<Value, String> {
    // All backends now use Pi, so switching is a no-op
    Ok(json!({
        "switched": false,
        "backend": "pi",
        "message": "All models use Pi backend"
    }))
}

/// Get the current backend for a workspace (always "pi" now)
#[tauri::command]
pub(crate) async fn get_workspace_backend(
    _workspace_id: String,
    _state: State<'_, AppState>,
) -> Result<String, String> {
    Ok("pi".to_string())
}

/// Get model list from pi-adapter
/// Fetches available models from pi's model registry
#[tauri::command]
pub(crate) async fn get_all_models(
    state: State<'_, AppState>,
) -> Result<Value, String> {
    // Find a connected workspace to query models from
    let sessions = state.sessions.lock().await;
    let session = sessions.values().next();
    
    if let Some(session) = session {
        // Query models from pi-adapter
        let result = session.send_request("model/list", json!({})).await;
        if let Ok(response) = result {
            if let Some(data) = response.get("result").and_then(|r| r.get("data")) {
                return Ok(json!({ "data": data }));
            }
            if let Some(data) = response.get("data") {
                return Ok(json!({ "data": data }));
            }
        }
    }
    drop(sessions);
    
    // Fallback to minimal static list if no workspace connected
    let fallback_models = vec![
        json!({
            "id": "claude-sonnet-4-20250514",
            "model": "claude-sonnet-4-20250514",
            "displayName": "Claude Sonnet 4",
            "description": "Fast and highly capable for complex coding tasks",
            "supportedReasoningEfforts": [
                {"reasoningEffort": "default", "description": "Standard reasoning"}
            ],
            "defaultReasoningEffort": "default",
            "isDefault": true,
            "backend": "pi"
        }),
    ];

    Ok(json!({
        "data": fallback_models
    }))
}

/// Get auth status for OAuth providers
#[tauri::command]
pub(crate) async fn get_auth_status(
    workspace_id: String,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let sessions = state.sessions.lock().await;
    let session = sessions
        .get(&workspace_id)
        .ok_or("workspace not connected")?;
    
    let result = session.send_request("auth/status", json!({})).await;
    match result {
        Ok(response) => {
            if let Some(providers) = response.get("result").and_then(|r| r.get("providers")) {
                return Ok(json!({ "providers": providers }));
            }
            if let Some(providers) = response.get("providers") {
                return Ok(json!({ "providers": providers }));
            }
            Ok(json!({ "providers": [] }))
        }
        Err(e) => Err(e)
    }
}
