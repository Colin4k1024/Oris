//! Integration tests for the Evolution Server
//!
//! Tests the IPC handlers and server components.

use chrono::Utc;
use uuid::Uuid;

// Re-export types we're testing
use oris_evo_ipc_protocol::*;

/// Test ping response structure
#[test]
fn test_ping_response_format() {
    use oris_evo_ipc_protocol::response::*;

    let result = ResponseResult::Ping(PingResponse {
        timestamp: Utc::now().to_rfc3339(),
        message: Some("test".to_string()),
        version: "1.0".to_string(),
    });

    let response = JsonRpcResponse::success(Uuid::new_v4(), result);
    let json = response.to_json().expect("should serialize");

    let parsed: serde_json::Value = serde_json::from_str(&json).expect("should parse");
    assert!(parsed["result"]["version"].is_string());
    assert_eq!(parsed["result"]["version"], "1.0");
}

/// Test error response with different error codes
#[test]
fn test_error_codes() {
    use oris_evo_ipc_protocol::response::*;

    let codes = vec![
        (ErrorCode::ParseError, -32700),
        (ErrorCode::InvalidRequest, -32600),
        (ErrorCode::MethodNotFound, -32601),
        (ErrorCode::InvalidParams, -32602),
        (ErrorCode::InternalError, -32603),
        (ErrorCode::EvolutionError, -32000),
        (ErrorCode::GeneNotFound, -32001),
        (ErrorCode::ValidationFailed, -32002),
        (ErrorCode::SandboxError, -32003),
        (ErrorCode::SignatureError, -32004),
    ];

    for (code, expected) in codes {
        let response = JsonRpcResponse::error(Uuid::new_v4(), code, "test error");
        let json = response.to_json().expect("should serialize");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("should parse");
        assert_eq!(parsed["error"]["code"], expected, "code {:?} should have value {}", code, expected);
    }
}

/// Test solidify response structure
#[test]
fn test_solidify_response_format() {
    use oris_evo_ipc_protocol::response::*;

    let gene_id = Uuid::new_v4();
    let result = ResponseResult::Solidify(SolidifyResponse {
        success: true,
        gene_id,
    });

    let response = JsonRpcResponse::success(Uuid::new_v4(), result);
    let json = response.to_json().expect("should serialize");

    let parsed: serde_json::Value = serde_json::from_str(&json).expect("should parse");
    assert!(parsed["result"]["success"].as_bool().unwrap());
    assert_eq!(parsed["result"]["gene_id"].as_str().unwrap(), gene_id.to_string());
}

/// Test revert response structure
#[test]
fn test_revert_response_format() {
    use oris_evo_ipc_protocol::response::*;

    let gene_id = Uuid::new_v4();
    let result = ResponseResult::Revert(RevertResponse {
        success: true,
        gene_id,
        message: "Reverted due to confidence drop".to_string(),
    });

    let response = JsonRpcResponse::success(Uuid::new_v4(), result);
    let json = response.to_json().expect("should serialize");

    let parsed: serde_json::Value = serde_json::from_str(&json).expect("should parse");
    assert!(parsed["result"]["success"].as_bool().unwrap());
    assert_eq!(parsed["result"]["gene_id"].as_str().unwrap(), gene_id.to_string());
    assert!(parsed["result"]["message"].as_str().unwrap().contains("confidence drop"));
}

/// Test list response structure
#[test]
fn test_list_response_format() {
    use oris_evo_ipc_protocol::response::*;

    let result = ResponseResult::List(ListResponse {
        total: 42,
        genes: vec![],
    });

    let response = JsonRpcResponse::success(Uuid::new_v4(), result);
    let json = response.to_json().expect("should serialize");

    let parsed: serde_json::Value = serde_json::from_str(&json).expect("should parse");
    assert_eq!(parsed["result"]["total"], 42);
    assert!(parsed["result"]["genes"].is_array());
}

/// Test evolution context
#[test]
fn test_evolution_context_serialization() {
    let context = EvolutionContext {
        session_id: Uuid::new_v4(),
        user_id: Uuid::new_v4(),
        workspace: "/tmp/test".to_string(),
        user_confirmation: Some(true),
    };

    let json = serde_json::to_string(&context).expect("should serialize");
    let deserialized: EvolutionContext = serde_json::from_str(&json).expect("should deserialize");

    assert_eq!(deserialized.workspace, "/tmp/test");
    assert_eq!(deserialized.user_confirmation, Some(true));
}

/// Test gene query
#[test]
fn test_gene_query_serialization() {
    let query = GeneQuery {
        pattern: "cannot find value".to_string(),
        limit: Some(10),
        min_confidence: Some(0.7),
    };

    let json = serde_json::to_string(&query).expect("should serialize");
    let deserialized: GeneQuery = serde_json::from_str(&json).expect("should deserialize");

    assert_eq!(deserialized.pattern, "cannot find value");
    assert_eq!(deserialized.limit, Some(10));
    assert_eq!(deserialized.min_confidence, Some(0.7));
}

/// Test source location
#[test]
fn test_source_location_serialization() {
    let location = SourceLocation {
        file: "src/main.rs".to_string(),
        line: 42,
        column: Some(5),
    };

    let json = serde_json::to_string(&location).expect("should serialize");
    let deserialized: SourceLocation = serde_json::from_str(&json).expect("should deserialize");

    assert_eq!(deserialized.file, "src/main.rs");
    assert_eq!(deserialized.line, 42);
    assert_eq!(deserialized.column, Some(5));
}

/// Test gene metadata
#[test]
fn test_gene_metadata_serialization() {
    let metadata = GeneMetadata {
        language: Some("rust".to_string()),
        file_extensions: vec!["rs".to_string()],
        tags: vec!["compiler".to_string(), "error".to_string()],
    };

    let json = serde_json::to_string(&metadata).expect("should serialize");
    let deserialized: GeneMetadata = serde_json::from_str(&json).expect("should deserialize");

    assert_eq!(deserialized.language, Some("rust".to_string()));
    assert_eq!(deserialized.file_extensions, vec!["rs".to_string()]);
    assert_eq!(deserialized.tags.len(), 2);
}

/// Test protocol constants
#[test]
fn test_protocol_constants() {
    assert_eq!(PROTOCOL_VERSION, "1.0");
}
