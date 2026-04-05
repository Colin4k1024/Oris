//! IPC Response types (JSON-RPC 2.0 style)

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{EvolutionResult, Gene};

/// JSON-RPC 2.0 Response envelope
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    /// JSON-RPC version (always "2.0")
    pub jsonrpc: String,
    /// Request ID for correlation
    pub id: Uuid,
    /// Response result (null if error)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<ResponseResult>,
    /// Error (null if success)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ResponseError>,
}

/// Response result variants
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ResponseResult {
    /// evolve response
    Evolve(EvolutionResult),
    /// solidify response
    Solidify(SolidifyResponse),
    /// revert response
    Revert(RevertResponse),
    /// query response
    Query(Vec<Gene>),
    /// list response
    List(ListResponse),
    /// ping response
    Ping(PingResponse),
}

/// Solidify response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolidifyResponse {
    /// Whether solidification succeeded
    pub success: bool,
    /// Gene ID that was solidified
    pub gene_id: Uuid,
}

/// Revert response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevertResponse {
    /// Whether revert succeeded
    pub success: bool,
    /// Gene ID that was reverted
    pub gene_id: Uuid,
    /// Revert message
    pub message: String,
}

/// List response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListResponse {
    /// Total genes in pool
    pub total: usize,
    /// Genes in this page
    pub genes: Vec<Gene>,
}

/// Ping response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PingResponse {
    /// Server timestamp
    pub timestamp: String,
    /// Echoed message (if any)
    pub message: Option<String>,
    /// Protocol version
    pub version: String,
}

/// Response error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseError {
    /// Error code
    pub code: i32,
    /// Error message
    pub message: String,
    /// Optional error data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// Standard error codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    /// Parse error
    ParseError = -32700,
    /// Invalid request
    InvalidRequest = -32600,
    /// Method not found
    MethodNotFound = -32601,
    /// Invalid params
    InvalidParams = -32602,
    /// Internal error
    InternalError = -32603,
    /// Evolution error (custom)
    EvolutionError = -32000,
    /// Gene not found (custom)
    GeneNotFound = -32001,
    /// Validation failed (custom)
    ValidationFailed = -32002,
    /// Sandbox error (custom)
    SandboxError = -32003,
    /// Signature verification failed (custom)
    SignatureError = -32004,
}

impl JsonRpcResponse {
    /// Create a success response
    pub fn success(id: Uuid, result: ResponseResult) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    /// Create an error response
    pub fn error(id: Uuid, code: ErrorCode, message: &str) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(ResponseError {
                code: code as i32,
                message: message.to_string(),
                data: None,
            }),
        }
    }

    /// Create an error response with data
    pub fn error_with_data(id: Uuid, code: ErrorCode, message: &str, data: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(ResponseError {
                code: code as i32,
                message: message.to_string(),
                data: Some(data),
            }),
        }
    }

    /// Parse a JSON string into a response
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Serialize this response to JSON
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}

impl ResponseError {
    /// Create a method not found error
    pub fn method_not_found(method: &str) -> Self {
        Self {
            code: ErrorCode::MethodNotFound as i32,
            message: format!("Method '{}' not found", method),
            data: None,
        }
    }

    /// Create an invalid params error
    pub fn invalid_params(details: &str) -> Self {
        Self {
            code: ErrorCode::InvalidParams as i32,
            message: details.to_string(),
            data: None,
        }
    }

    /// Create an internal error
    pub fn internal(message: &str) -> Self {
        Self {
            code: ErrorCode::InternalError as i32,
            message: message.to_string(),
            data: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_success_response() {
        let result = ResponseResult::Ping(PingResponse {
            timestamp: chrono::Utc::now().to_rfc3339(),
            message: Some("hello".to_string()),
            version: "1.0".to_string(),
        });

        let response = JsonRpcResponse::success(Uuid::new_v4(), result);
        let json = response.to_json().unwrap();
        let parsed = JsonRpcResponse::from_json(&json).unwrap();

        assert!(parsed.error.is_none());
        assert!(parsed.result.is_some());
    }

    #[test]
    fn test_error_response() {
        let response = JsonRpcResponse::error(
            Uuid::new_v4(),
            ErrorCode::MethodNotFound,
            "Method 'unknown' not found",
        );

        let json = response.to_json().unwrap();
        let parsed = JsonRpcResponse::from_json(&json).unwrap();

        assert!(parsed.result.is_none());
        assert!(parsed.error.is_some());
        assert_eq!(parsed.error.unwrap().code, -32601);
    }
}
