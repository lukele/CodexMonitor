//! Claude API client for the Anthropic Messages API

use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use tracing::{debug, error, info};

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_MAX_TOKENS: u32 = 16384;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(300); // 5 minutes

pub struct ClaudeClient {
    http: Client,
    api_key: String,
}

impl ClaudeClient {
    pub fn new(api_key: String) -> Self {
        let http = Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .expect("Failed to create HTTP client");

        Self { http, api_key }
    }

    pub async fn send_message(
        &self,
        model: &str,
        messages: Vec<ApiMessage>,
        tools: Vec<ToolDefinition>,
        system: &str,
    ) -> Result<ApiResponse> {
        let max_tokens = std::env::var("CLAUDE_MAX_TOKENS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_MAX_TOKENS);

        let request = CreateMessageRequest {
            model: model.to_string(),
            max_tokens,
            messages,
            tools: if tools.is_empty() { None } else { Some(tools) },
            system: Some(system.to_string()),
            stream: Some(false),
        };

        debug!("Sending request to Claude API: model={}", model);

        let response = self
            .http
            .post(ANTHROPIC_API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&request)
            .send()
            .await
            .context("Failed to send request to Claude API")?;

        let status = response.status();

        if !status.is_success() {
            let error_body = response.text().await.unwrap_or_default();
            error!("Claude API error ({}): {}", status, error_body);

            // Parse error for better messaging
            if let Ok(error_json) = serde_json::from_str::<Value>(&error_body) {
                let error_type = error_json["error"]["type"].as_str().unwrap_or("unknown");
                let error_message = error_json["error"]["message"]
                    .as_str()
                    .unwrap_or(&error_body);

                match error_type {
                    "authentication_error" => {
                        anyhow::bail!("Authentication failed. Please check your ANTHROPIC_API_KEY.");
                    }
                    "rate_limit_error" => {
                        anyhow::bail!("Rate limit exceeded. Please wait and try again.");
                    }
                    "overloaded_error" => {
                        anyhow::bail!("Claude API is overloaded. Please try again later.");
                    }
                    "invalid_request_error" => {
                        anyhow::bail!("Invalid request: {}", error_message);
                    }
                    _ => {
                        anyhow::bail!("Claude API error ({}): {}", status, error_message);
                    }
                }
            }

            anyhow::bail!("Claude API error ({}): {}", status, error_body);
        }

        let api_response: ApiResponse = response
            .json()
            .await
            .context("Failed to parse Claude API response")?;

        info!(
            "Claude response: {} content blocks, stop_reason={:?}, usage: {} in / {} out",
            api_response.content.len(),
            api_response.stop_reason,
            api_response.usage.input_tokens,
            api_response.usage.output_tokens
        );

        Ok(api_response)
    }
}

#[derive(Debug, Serialize)]
struct CreateMessageRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<ApiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ToolDefinition>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiMessage {
    pub role: String,
    pub content: Vec<ContentBlock>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Deserialize)]
pub struct ApiResponse {
    pub id: String,
    #[serde(rename = "type")]
    pub response_type: String,
    pub role: String,
    pub content: Vec<ContentBlock>,
    pub model: String,
    pub stop_reason: Option<String>,
    pub stop_sequence: Option<String>,
    pub usage: Usage,
}

#[derive(Debug, Deserialize)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_content_block_text_serialization() {
        let block = ContentBlock::Text {
            text: "Hello, world!".to_string(),
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains("\"type\":\"text\""));
        assert!(json.contains("\"text\":\"Hello, world!\""));
    }

    #[test]
    fn test_content_block_tool_use_serialization() {
        let block = ContentBlock::ToolUse {
            id: "tool_123".to_string(),
            name: "shell".to_string(),
            input: json!({"command": ["ls", "-la"]}),
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains("\"type\":\"tool_use\""));
        assert!(json.contains("\"name\":\"shell\""));
        assert!(json.contains("\"id\":\"tool_123\""));
    }

    #[test]
    fn test_content_block_tool_result_serialization() {
        let block = ContentBlock::ToolResult {
            tool_use_id: "tool_123".to_string(),
            content: "file1.txt\nfile2.txt".to_string(),
            is_error: None,
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains("\"type\":\"tool_result\""));
        assert!(json.contains("\"tool_use_id\":\"tool_123\""));
    }

    #[test]
    fn test_api_message_serialization() {
        let msg = ApiMessage {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: "Hello".to_string(),
            }],
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"role\":\"user\""));
    }

    #[test]
    fn test_tool_definition_serialization() {
        let tool = ToolDefinition {
            name: "shell".to_string(),
            description: "Execute a shell command".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "array",
                        "items": {"type": "string"}
                    }
                },
                "required": ["command"]
            }),
        };
        let json = serde_json::to_string(&tool).unwrap();
        assert!(json.contains("\"name\":\"shell\""));
        assert!(json.contains("\"input_schema\""));
    }

    #[test]
    fn test_api_response_deserialization() {
        let json = r#"{
            "id": "msg_123",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "Hello!"}],
            "model": "claude-3-5-sonnet-20241022",
            "stop_reason": "end_turn",
            "stop_sequence": null,
            "usage": {"input_tokens": 10, "output_tokens": 5}
        }"#;

        let response: ApiResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.id, "msg_123");
        assert_eq!(response.role, "assistant");
        assert_eq!(response.stop_reason, Some("end_turn".to_string()));
        assert_eq!(response.usage.input_tokens, 10);
        assert_eq!(response.usage.output_tokens, 5);
    }
}
