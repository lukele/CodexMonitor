//! JSON-RPC protocol types compatible with Codex app-server

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub type RequestId = u64;

/// JSON-RPC 2.0 message types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JSONRPCMessage {
    Request {
        id: RequestId,
        method: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        params: Option<Value>,
    },
    Response {
        id: RequestId,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<Value>,
    },
    Notification {
        method: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        params: Option<Value>,
    },
}

/// Error codes following JSON-RPC 2.0 spec
#[allow(dead_code)] // These are available for future use
pub mod error_codes {
    pub const PARSE_ERROR: i32 = -32700;
    pub const INVALID_REQUEST: i32 = -32600;
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
    pub const INTERNAL_ERROR: i32 = -32603;
    
    // Custom error codes
    pub const THREAD_NOT_FOUND: i32 = -32000;
    pub const API_ERROR: i32 = -32001;
    pub const TOOL_EXECUTION_ERROR: i32 = -32002;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_request() {
        let json_str = r#"{"id": 1, "method": "initialize", "params": {}}"#;
        let msg: JSONRPCMessage = serde_json::from_str(json_str).unwrap();
        
        match msg {
            JSONRPCMessage::Request { id, method, .. } => {
                assert_eq!(id, 1);
                assert_eq!(method, "initialize");
            }
            _ => panic!("Expected Request"),
        }
    }

    #[test]
    fn test_serialize_response() {
        let msg = JSONRPCMessage::Response {
            id: 1,
            result: Some(json!({"success": true})),
            error: None,
        };
        
        let json_str = serde_json::to_string(&msg).unwrap();
        assert!(json_str.contains("\"id\":1"));
        assert!(json_str.contains("\"success\":true"));
    }

    #[test]
    fn test_serialize_notification() {
        let msg = JSONRPCMessage::Notification {
            method: "codex/turnStarted".to_string(),
            params: Some(json!({"threadId": "test-123"})),
        };
        
        let json_str = serde_json::to_string(&msg).unwrap();
        assert!(json_str.contains("codex/turnStarted"));
        assert!(json_str.contains("test-123"));
    }
}
