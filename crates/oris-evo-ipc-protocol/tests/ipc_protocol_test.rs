//! Integration tests for the IPC Protocol
//!
//! Tests the JSON-RPC 2.0 protocol implementation including
//! request/response serialization and deserialization.

use chrono::Utc;
use uuid::Uuid;

// Re-export the types we're testing
use oris_evo_ipc_protocol::*;

/// Test that we can create and serialize an evolve request
#[test]
fn test_evolve_request_serialization() {
    let signal = RuntimeSignal {
        id: Uuid::new_v4(),
        signal_type: RuntimeSignalType::CompilerError,
        content: "error[E0425]: cannot find value `foo` in this scope".to_string(),
        location: Some(SourceLocation {
            file: "src/main.rs".to_string(),
            line: 42,
            column: Some(5),
        }),
        severity: 0.85,
        timestamp: Utc::now(),
    };

    let context = EvolutionContext {
        session_id: Uuid::new_v4(),
        user_id: Uuid::new_v4(),
        workspace: "/tmp/test-workspace".to_string(),
        user_confirmation: None,
    };

    let request = JsonRpcRequest::evolve(signal.clone(), context.clone());
    let json = request.to_json().expect("should serialize to JSON");

    // Verify it's valid JSON
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("should parse as JSON");
    assert_eq!(parsed["jsonrpc"], "2.0");
    assert_eq!(parsed["method"], "evolve");

    // Deserialize back
    let deserialized = JsonRpcRequest::from_json(&json).expect("should deserialize");
    assert_eq!(deserialized.method, "evolve");
}

/// Test that we can create and serialize a ping request
#[test]
fn test_ping_request_serialization() {
    let request = JsonRpcRequest::ping(Some("hello".to_string()));
    let json = request.to_json().expect("should serialize to JSON");

    let parsed: serde_json::Value = serde_json::from_str(&json).expect("should parse as JSON");
    assert_eq!(parsed["jsonrpc"], "2.0");
    assert_eq!(parsed["method"], "ping");

    let deserialized = JsonRpcRequest::from_json(&json).expect("should deserialize");
    assert_eq!(deserialized.method, "ping");
}

/// Test that we can create and serialize a list request
#[test]
fn test_list_request_serialization() {
    let request = JsonRpcRequest::list(Some(100), Some(0));
    let json = request.to_json().expect("should serialize to JSON");

    let parsed: serde_json::Value = serde_json::from_str(&json).expect("should parse as JSON");
    assert_eq!(parsed["method"], "list");

    let deserialized = JsonRpcRequest::from_json(&json).expect("should deserialize");
    assert_eq!(deserialized.method, "list");
}

/// Test that we can create and serialize a revert request
#[test]
fn test_revert_request_serialization() {
    let reason = RevertReason {
        gene_id: Uuid::new_v4(),
        reason: "Confidence drop detected".to_string(),
        confidence_drop: Some(0.25),
    };

    let request = JsonRpcRequest::revert(reason.clone());
    let json = request.to_json().expect("should serialize to JSON");

    let parsed: serde_json::Value = serde_json::from_str(&json).expect("should parse as JSON");
    assert_eq!(parsed["method"], "revert");

    let deserialized = JsonRpcRequest::from_json(&json).expect("should deserialize");
    assert_eq!(deserialized.method, "revert");
}

/// Test that we can create and serialize a query request
#[test]
fn test_query_request_serialization() {
    let query = GeneQuery {
        pattern: "cannot find value".to_string(),
        limit: Some(10),
        min_confidence: Some(0.5),
    };

    let request = JsonRpcRequest::query(query);
    let json = request.to_json().expect("should serialize to JSON");

    let parsed: serde_json::Value = serde_json::from_str(&json).expect("should parse as JSON");
    assert_eq!(parsed["method"], "query");

    let deserialized = JsonRpcRequest::from_json(&json).expect("should deserialize");
    assert_eq!(deserialized.method, "query");
}

/// Test that we can create and serialize a success response
#[test]
fn test_success_response_serialization() {
    use oris_evo_ipc_protocol::response::{JsonRpcResponse, ResponseResult};

    let result = ResponseResult::Ping(response::PingResponse {
        timestamp: Utc::now().to_rfc3339(),
        message: Some("pong".to_string()),
        version: "1.0".to_string(),
    });

    let response = JsonRpcResponse::success(Uuid::new_v4(), result);
    let json = response.to_json().expect("should serialize to JSON");

    let parsed: serde_json::Value = serde_json::from_str(&json).expect("should parse as JSON");
    assert_eq!(parsed["jsonrpc"], "2.0");
    assert!(parsed["result"].is_object());
    assert!(parsed["error"].is_null());
}

/// Test that we can create and serialize an error response
#[test]
fn test_error_response_serialization() {
    use oris_evo_ipc_protocol::response::{ErrorCode, JsonRpcResponse};

    let response = JsonRpcResponse::error(
        Uuid::new_v4(),
        ErrorCode::MethodNotFound,
        "Method 'unknown' not found",
    );
    let json = response.to_json().expect("should serialize to JSON");

    let parsed: serde_json::Value = serde_json::from_str(&json).expect("should parse as JSON");
    assert_eq!(parsed["jsonrpc"], "2.0");
    assert!(parsed["result"].is_null());
    assert!(parsed["error"].is_object());
    assert_eq!(parsed["error"]["code"], -32601); // Method not found
}

/// Test signal types
#[test]
fn test_signal_types() {
    let compiler_error = RuntimeSignal {
        id: Uuid::new_v4(),
        signal_type: RuntimeSignalType::CompilerError,
        content: "error".to_string(),
        location: None,
        severity: 0.5,
        timestamp: Utc::now(),
    };

    let panic = RuntimeSignal {
        id: Uuid::new_v4(),
        signal_type: RuntimeSignalType::Panic,
        content: "thread panicked".to_string(),
        location: None,
        severity: 0.9,
        timestamp: Utc::now(),
    };

    // Serialize and deserialize to verify all types work
    let compiler_json = serde_json::to_string(&compiler_error).expect("should serialize");
    let panic_json = serde_json::to_string(&panic).expect("should serialize");

    let _: RuntimeSignal = serde_json::from_str(&compiler_json).expect("should deserialize");
    let _: RuntimeSignal = serde_json::from_str(&panic_json).expect("should deserialize");
}

/// Test evolution action types
#[test]
fn test_evolution_action_serialization() {
    let solidify = EvolutionAction::Solidify;
    let apply_once = EvolutionAction::ApplyOnce;
    let reject = EvolutionAction::Reject;

    let solidify_json = serde_json::to_string(&solidify).expect("should serialize");
    let apply_once_json = serde_json::to_string(&apply_once).expect("should serialize");
    let reject_json = serde_json::to_string(&reject).expect("should serialize");

    assert!(solidify_json.contains("solidify"));
    assert!(apply_once_json.contains("apply_once"));
    assert!(reject_json.contains("reject"));
}

/// Test evolution result
#[test]
fn test_evolution_result_serialization() {
    let result = EvolutionResult {
        gene_id: Some(Uuid::new_v4()),
        confidence: 0.85,
        action: EvolutionAction::Solidify,
        revert_triggered: false,
        evaluation_summary: "Test evaluation".to_string(),
    };

    let json = serde_json::to_string(&result).expect("should serialize");
    let deserialized: EvolutionResult = serde_json::from_str(&json).expect("should deserialize");

    assert!(deserialized.gene_id.is_some());
    assert_eq!(deserialized.confidence, 0.85);
    assert_eq!(deserialized.action, EvolutionAction::Solidify);
    assert!(!deserialized.revert_triggered);
}

/// Test source tag
#[test]
fn test_source_tag_serialization() {
    let tag = SourceTag {
        error_type: "compiler_error".to_string(),
        user_id: Uuid::new_v4(),
        session_id: Uuid::new_v4(),
        timestamp: Utc::now(),
    };

    let json = serde_json::to_string(&tag).expect("should serialize");
    let deserialized: SourceTag = serde_json::from_str(&json).expect("should deserialize");

    assert_eq!(deserialized.error_type, "compiler_error");
}

/// Test protocol version constant
#[test]
fn test_protocol_version() {
    assert_eq!(PROTOCOL_VERSION, "1.0");
}
