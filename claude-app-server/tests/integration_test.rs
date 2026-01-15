//! Integration tests for claude-app-server
//!
//! These tests verify the JSON-RPC protocol handling without making actual API calls.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

/// Helper to parse JSON-RPC responses
fn parse_response(line: &str) -> Option<Value> {
    serde_json::from_str(line).ok()
}

/// Test that the server starts and responds to initialize
#[test]
#[ignore] // Requires built binary
fn test_initialize() {
    let mut child = Command::new("cargo")
        .args(["run", "--release", "-q"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to start server");

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let reader = BufReader::new(stdout);

    // Send initialize request
    writeln!(stdin, r#"{{"id": 1, "method": "initialize", "params": {{}}}}"#).unwrap();
    stdin.flush().unwrap();

    // Read response with timeout
    let mut lines = reader.lines();
    let response_line = lines.next().expect("No response").expect("Read error");
    let response = parse_response(&response_line).expect("Invalid JSON");

    // Verify response
    assert_eq!(response["id"], 1);
    assert!(response["result"]["protocolVersion"].is_string());
    assert!(response["result"]["capabilities"]["tools"].as_bool().unwrap_or(false));

    // Cleanup
    drop(stdin);
    child.kill().ok();
}

/// Test model list endpoint
#[test]
#[ignore] // Requires built binary
fn test_model_list() {
    let mut child = Command::new("cargo")
        .args(["run", "--release", "-q"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to start server");

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let reader = BufReader::new(stdout);

    // Initialize first
    writeln!(stdin, r#"{{"id": 1, "method": "initialize", "params": {{}}}}"#).unwrap();
    // Then request models
    writeln!(stdin, r#"{{"id": 2, "method": "model/list", "params": {{}}}}"#).unwrap();
    stdin.flush().unwrap();

    // Skip initialize response, get model list
    let mut lines = reader.lines();
    lines.next(); // skip init response
    let response_line = lines.next().expect("No response").expect("Read error");
    let response = parse_response(&response_line).expect("Invalid JSON");

    // Verify response
    assert_eq!(response["id"], 2);
    let models = response["result"]["data"].as_array().expect("No models array");
    assert!(!models.is_empty(), "No models returned");

    // Check first model has required fields
    let first_model = &models[0];
    assert!(first_model["id"].is_string());
    assert!(first_model["model"].is_string());
    assert!(first_model["displayName"].is_string());

    // Cleanup
    drop(stdin);
    child.kill().ok();
}

/// Test thread lifecycle
#[test]
#[ignore] // Requires built binary
fn test_thread_lifecycle() {
    let mut child = Command::new("cargo")
        .args(["run", "--release", "-q"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to start server");

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let reader = BufReader::new(stdout);

    // Initialize
    writeln!(stdin, r#"{{"id": 1, "method": "initialize", "params": {{}}}}"#).unwrap();
    // Start thread
    writeln!(
        stdin,
        r#"{{"id": 2, "method": "thread/start", "params": {{"name": "Test Thread"}}}}"#
    )
    .unwrap();
    // List threads
    writeln!(stdin, r#"{{"id": 3, "method": "thread/list", "params": {{}}}}"#).unwrap();
    stdin.flush().unwrap();

    let mut lines = reader.lines();
    lines.next(); // skip init

    // Check thread start response
    let start_response = parse_response(&lines.next().unwrap().unwrap()).unwrap();
    assert_eq!(start_response["id"], 2);
    let thread_id = start_response["result"]["threadId"]
        .as_str()
        .expect("No threadId");
    assert!(!thread_id.is_empty());

    // Check thread list response
    let list_response = parse_response(&lines.next().unwrap().unwrap()).unwrap();
    assert_eq!(list_response["id"], 3);
    let threads = list_response["result"]["threads"]
        .as_array()
        .expect("No threads");
    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0]["id"].as_str().unwrap(), thread_id);

    // Cleanup
    drop(stdin);
    child.kill().ok();
}

/// Unit test for protocol types
#[test]
fn test_protocol_types() {
    // Test request parsing
    let request_json = r#"{"id": 1, "method": "test", "params": {"foo": "bar"}}"#;
    let value: Value = serde_json::from_str(request_json).unwrap();
    assert_eq!(value["id"], 1);
    assert_eq!(value["method"], "test");
    assert_eq!(value["params"]["foo"], "bar");

    // Test response format
    let response = json!({
        "id": 1,
        "result": {"success": true},
    });
    assert!(response["error"].is_null());

    // Test notification format
    let notification = json!({
        "method": "codex/turnStarted",
        "params": {"threadId": "abc-123"}
    });
    assert!(notification["id"].is_null());
}

/// Unit test for model list format
#[test]
fn test_model_format() {
    let model = json!({
        "id": "claude-sonnet-4-20250514",
        "model": "claude-sonnet-4-20250514",
        "displayName": "Claude Sonnet 4",
        "description": "Most intelligent model",
        "supportedReasoningEfforts": [
            {"reasoningEffort": "default", "description": "Standard reasoning"}
        ],
        "defaultReasoningEffort": "default",
        "isDefault": true
    });

    assert_eq!(model["id"], model["model"]);
    assert!(model["isDefault"].as_bool().unwrap());
    assert!(model["supportedReasoningEfforts"].is_array());
}
