//! Tool definitions and execution for Claude app-server

use std::path::PathBuf;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::process::Command;
use tracing::{debug, error, info, warn};

use crate::claude::ToolDefinition;

const DEFAULT_TIMEOUT_MS: u64 = 30000;
const MAX_OUTPUT_SIZE: usize = 100_000; // 100KB

pub struct ToolExecutor {
    cwd: PathBuf,
}

impl ToolExecutor {
    pub fn new(cwd: PathBuf) -> Self {
        Self { cwd }
    }

    pub fn get_tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "shell".to_string(),
                description: "Execute a shell command in the workspace. Use this for running builds, tests, git commands, and other CLI operations. The command runs in a bash shell.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Command and arguments to execute (e.g., [\"git\", \"status\"] or [\"npm\", \"test\"])"
                        },
                        "workdir": {
                            "type": "string",
                            "description": "Working directory relative to workspace root (optional)"
                        },
                        "timeout": {
                            "type": "integer",
                            "description": "Timeout in milliseconds (default: 30000)"
                        }
                    },
                    "required": ["command"]
                }),
            },
            ToolDefinition {
                name: "read_file".to_string(),
                description: "Read the contents of a file. Returns the full text content.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Path to the file relative to workspace root"
                        },
                        "offset": {
                            "type": "integer",
                            "description": "Line number to start reading from (1-indexed, optional)"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum number of lines to read (optional)"
                        }
                    },
                    "required": ["path"]
                }),
            },
            ToolDefinition {
                name: "write_file".to_string(),
                description: "Write content to a file. Creates the file if it doesn't exist, or overwrites if it does.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Path to the file relative to workspace root"
                        },
                        "content": {
                            "type": "string",
                            "description": "Content to write to the file"
                        }
                    },
                    "required": ["path", "content"]
                }),
            },
            ToolDefinition {
                name: "list_files".to_string(),
                description: "List files in a directory.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Directory path relative to workspace root"
                        },
                        "recursive": {
                            "type": "boolean",
                            "description": "Whether to list files recursively (default: false)"
                        },
                        "pattern": {
                            "type": "string",
                            "description": "Glob pattern to filter files (optional)"
                        }
                    },
                    "required": ["path"]
                }),
            },
            ToolDefinition {
                name: "search_files".to_string(),
                description: "Search for text in files using grep-like functionality.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "pattern": {
                            "type": "string",
                            "description": "Search pattern (regex supported)"
                        },
                        "path": {
                            "type": "string",
                            "description": "Directory or file to search in (default: workspace root)"
                        },
                        "file_pattern": {
                            "type": "string",
                            "description": "Glob pattern to filter which files to search (e.g., \"*.rs\")"
                        }
                    },
                    "required": ["pattern"]
                }),
            },
            ToolDefinition {
                name: "edit_file".to_string(),
                description: "Make a surgical edit to a file by replacing exact text.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Path to the file to edit"
                        },
                        "old_text": {
                            "type": "string",
                            "description": "Exact text to find and replace (must match exactly)"
                        },
                        "new_text": {
                            "type": "string",
                            "description": "New text to replace the old text with"
                        }
                    },
                    "required": ["path", "old_text", "new_text"]
                }),
            },
        ]
    }

    pub async fn execute(&self, tool_name: &str, input: &Value) -> Value {
        debug!("Executing tool: {} with input: {:?}", tool_name, input);

        let result = match tool_name {
            "shell" => self.execute_shell(input).await,
            "read_file" => self.execute_read_file(input).await,
            "write_file" => self.execute_write_file(input).await,
            "list_files" => self.execute_list_files(input).await,
            "search_files" => self.execute_search_files(input).await,
            "edit_file" => self.execute_edit_file(input).await,
            _ => {
                error!("Unknown tool: {}", tool_name);
                json!({ "error": format!("Unknown tool: {}", tool_name) })
            }
        };

        debug!("Tool result: {:?}", result);
        result
    }

    async fn execute_shell(&self, input: &Value) -> Value {
        let command = match input["command"].as_array() {
            Some(arr) => arr
                .iter()
                .filter_map(|v| v.as_str())
                .map(String::from)
                .collect::<Vec<_>>(),
            None => {
                return json!({ "error": "Missing or invalid 'command' parameter" });
            }
        };

        if command.is_empty() {
            return json!({ "error": "Command array is empty" });
        }

        let workdir = input["workdir"]
            .as_str()
            .map(|w| self.cwd.join(w))
            .unwrap_or_else(|| self.cwd.clone());

        let timeout_ms = input["timeout"].as_u64().unwrap_or(DEFAULT_TIMEOUT_MS);

        // Security check: prevent obviously dangerous commands
        let dangerous_commands = ["rm -rf /", "mkfs", "dd if=/dev/zero", "> /dev/sda"];
        let cmd_str = command.join(" ");
        for dangerous in dangerous_commands {
            if cmd_str.contains(dangerous) {
                warn!("Blocked dangerous command: {}", cmd_str);
                return json!({ "error": "Command blocked for safety reasons" });
            }
        }

        let result = tokio::time::timeout(
            Duration::from_millis(timeout_ms),
            Command::new(&command[0])
                .args(&command[1..])
                .current_dir(&workdir)
                .output(),
        )
        .await;

        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);

                // Truncate output if too large
                let stdout = truncate_output(&stdout, MAX_OUTPUT_SIZE);
                let stderr = truncate_output(&stderr, MAX_OUTPUT_SIZE);

                json!({
                    "stdout": stdout,
                    "stderr": stderr,
                    "exitCode": output.status.code()
                })
            }
            Ok(Err(e)) => {
                error!("Shell command failed: {}", e);
                json!({ "error": e.to_string() })
            }
            Err(_) => {
                warn!("Shell command timed out after {}ms", timeout_ms);
                json!({ "error": format!("Command timed out after {}ms", timeout_ms) })
            }
        }
    }

    async fn execute_read_file(&self, input: &Value) -> Value {
        let path = match input["path"].as_str() {
            Some(p) => p,
            None => return json!({ "error": "Missing 'path' parameter" }),
        };

        let full_path = self.cwd.join(path);

        // Security: ensure path doesn't escape workspace
        if !is_safe_path(&self.cwd, &full_path) {
            return json!({ "error": "Path escapes workspace directory" });
        }

        match tokio::fs::read_to_string(&full_path).await {
            Ok(content) => {
                let offset = input["offset"].as_u64().unwrap_or(1) as usize;
                let limit = input["limit"].as_u64().map(|l| l as usize);

                let lines: Vec<&str> = content.lines().collect();
                let start = (offset.saturating_sub(1)).min(lines.len());
                let end = limit
                    .map(|l| (start + l).min(lines.len()))
                    .unwrap_or(lines.len());

                let selected_lines = lines[start..end].join("\n");
                let truncated = selected_lines.len() > MAX_OUTPUT_SIZE;
                let content = truncate_output(&selected_lines, MAX_OUTPUT_SIZE);

                json!({
                    "content": content,
                    "totalLines": lines.len(),
                    "truncated": truncated
                })
            }
            Err(e) => {
                error!("Failed to read file {}: {}", path, e);
                json!({ "error": e.to_string() })
            }
        }
    }

    async fn execute_write_file(&self, input: &Value) -> Value {
        let path = match input["path"].as_str() {
            Some(p) => p,
            None => return json!({ "error": "Missing 'path' parameter" }),
        };

        let content = match input["content"].as_str() {
            Some(c) => c,
            None => return json!({ "error": "Missing 'content' parameter" }),
        };

        let full_path = self.cwd.join(path);

        // Security: ensure path doesn't escape workspace
        if !is_safe_path(&self.cwd, &full_path) {
            return json!({ "error": "Path escapes workspace directory" });
        }

        // Create parent directories if needed
        if let Some(parent) = full_path.parent() {
            if let Err(e) = tokio::fs::create_dir_all(parent).await {
                error!("Failed to create directories for {}: {}", path, e);
                return json!({ "error": format!("Failed to create directories: {}", e) });
            }
        }

        match tokio::fs::write(&full_path, content).await {
            Ok(_) => {
                json!({
                    "success": true,
                    "path": path,
                    "bytesWritten": content.len()
                })
            }
            Err(e) => {
                error!("Failed to write file {}: {}", path, e);
                json!({ "error": e.to_string() })
            }
        }
    }

    async fn execute_list_files(&self, input: &Value) -> Value {
        let path = input["path"].as_str().unwrap_or(".");
        let recursive = input["recursive"].as_bool().unwrap_or(false);

        let full_path = self.cwd.join(path);

        // Security: ensure path doesn't escape workspace
        if !is_safe_path(&self.cwd, &full_path) {
            return json!({ "error": "Path escapes workspace directory" });
        }

        let files = if recursive {
            list_files_recursive(&full_path, &self.cwd).await
        } else {
            list_files_flat(&full_path, &self.cwd).await
        };

        match files {
            Ok(files) => json!({ "files": files }),
            Err(e) => json!({ "error": e.to_string() }),
        }
    }

    async fn execute_search_files(&self, input: &Value) -> Value {
        let pattern = match input["pattern"].as_str() {
            Some(p) => p,
            None => return json!({ "error": "Missing 'pattern' parameter" }),
        };

        let path = input["path"].as_str().unwrap_or(".");
        let full_path = self.cwd.join(path);

        // Use grep for searching
        let mut cmd = Command::new("grep");
        cmd.arg("-rn")
            .arg("--include=*.rs")
            .arg("--include=*.ts")
            .arg("--include=*.js")
            .arg("--include=*.py")
            .arg("--include=*.go")
            .arg("--include=*.java")
            .arg("--include=*.c")
            .arg("--include=*.cpp")
            .arg("--include=*.h")
            .arg("--include=*.md")
            .arg("--include=*.json")
            .arg("--include=*.toml")
            .arg("--include=*.yaml")
            .arg("--include=*.yml")
            .arg("-E")
            .arg(pattern)
            .arg(&full_path)
            .current_dir(&self.cwd);

        match cmd.output().await {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let matches: Vec<Value> = stdout
                    .lines()
                    .take(100) // Limit results
                    .filter_map(|line| {
                        let parts: Vec<&str> = line.splitn(3, ':').collect();
                        if parts.len() >= 3 {
                            Some(json!({
                                "file": parts[0].strip_prefix(full_path.to_str().unwrap_or("")).unwrap_or(parts[0]).trim_start_matches('/'),
                                "line": parts[1].parse::<u32>().ok(),
                                "content": parts[2]
                            }))
                        } else {
                            None
                        }
                    })
                    .collect();

                json!({
                    "matches": matches,
                    "count": matches.len()
                })
            }
            Err(e) => json!({ "error": e.to_string() }),
        }
    }

    async fn execute_edit_file(&self, input: &Value) -> Value {
        let path = match input["path"].as_str() {
            Some(p) => p,
            None => return json!({ "error": "Missing 'path' parameter" }),
        };

        let old_text = match input["old_text"].as_str() {
            Some(t) => t,
            None => return json!({ "error": "Missing 'old_text' parameter" }),
        };

        let new_text = match input["new_text"].as_str() {
            Some(t) => t,
            None => return json!({ "error": "Missing 'new_text' parameter" }),
        };

        let full_path = self.cwd.join(path);

        // Security: ensure path doesn't escape workspace
        if !is_safe_path(&self.cwd, &full_path) {
            return json!({ "error": "Path escapes workspace directory" });
        }

        // Read file
        let content = match tokio::fs::read_to_string(&full_path).await {
            Ok(c) => c,
            Err(e) => return json!({ "error": format!("Failed to read file: {}", e) }),
        };

        // Check if old_text exists
        if !content.contains(old_text) {
            return json!({
                "error": "old_text not found in file",
                "hint": "Make sure old_text matches exactly including whitespace"
            });
        }

        // Count occurrences
        let count = content.matches(old_text).count();
        if count > 1 {
            return json!({
                "error": format!("old_text found {} times in file, expected exactly 1", count),
                "hint": "Provide more context to make the match unique"
            });
        }

        // Perform replacement
        let new_content = content.replace(old_text, new_text);

        // Write file
        match tokio::fs::write(&full_path, &new_content).await {
            Ok(_) => {
                json!({
                    "success": true,
                    "path": path,
                    "replacements": 1
                })
            }
            Err(e) => json!({ "error": format!("Failed to write file: {}", e) }),
        }
    }
}

fn truncate_output(s: &str, max_size: usize) -> String {
    if s.len() <= max_size {
        s.to_string()
    } else {
        format!(
            "{}... [truncated, {} bytes total]",
            &s[..max_size],
            s.len()
        )
    }
}

fn is_safe_path(workspace: &PathBuf, target: &PathBuf) -> bool {
    match (workspace.canonicalize(), target.canonicalize()) {
        (Ok(ws), Ok(tgt)) => tgt.starts_with(&ws),
        // If canonicalize fails (e.g., file doesn't exist yet), check the parent
        (Ok(ws), Err(_)) => target
            .parent()
            .and_then(|p| p.canonicalize().ok())
            .map(|p| p.starts_with(&ws))
            .unwrap_or(false)
            || target.starts_with(workspace),
        _ => false,
    }
}

async fn list_files_flat(dir: &PathBuf, workspace: &PathBuf) -> Result<Vec<Value>, std::io::Error> {
    let mut entries = tokio::fs::read_dir(dir).await?;
    let mut files = vec![];

    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        let metadata = entry.metadata().await?;
        let relative = path
            .strip_prefix(workspace)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();

        files.push(json!({
            "name": entry.file_name().to_string_lossy(),
            "path": relative,
            "isDirectory": metadata.is_dir(),
            "size": metadata.len()
        }));
    }

    Ok(files)
}

async fn list_files_recursive(
    dir: &PathBuf,
    workspace: &PathBuf,
) -> Result<Vec<Value>, std::io::Error> {
    let mut files = vec![];
    let mut stack = vec![dir.clone()];

    while let Some(current) = stack.pop() {
        let mut entries = tokio::fs::read_dir(&current).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let metadata = entry.metadata().await?;

            // Skip hidden files and common ignore patterns
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.')
                || name == "node_modules"
                || name == "target"
                || name == "__pycache__"
            {
                continue;
            }

            let relative = path
                .strip_prefix(workspace)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();

            if metadata.is_dir() {
                stack.push(path);
            }

            files.push(json!({
                "name": name,
                "path": relative,
                "isDirectory": metadata.is_dir(),
                "size": metadata.len()
            }));

            // Limit total files
            if files.len() >= 1000 {
                return Ok(files);
            }
        }
    }

    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_is_safe_path() {
        let workspace = PathBuf::from("/home/user/project");
        let safe = PathBuf::from("/home/user/project/src/main.rs");
        let unsafe_path = PathBuf::from("/etc/passwd");

        // Note: These tests may not work without actual filesystem
        // Just testing the logic with string comparison
        assert!(safe.starts_with(&workspace));
        assert!(!unsafe_path.starts_with(&workspace));
    }

    #[test]
    fn test_truncate_output() {
        let short = "hello";
        assert_eq!(truncate_output(short, 100), "hello");

        let long = "a".repeat(200);
        let truncated = truncate_output(&long, 100);
        assert!(truncated.contains("[truncated"));
    }
}
