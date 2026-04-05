//! IPC Request types (JSON-RPC 2.0 style)

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{EvolutionContext, GeneQuery, RevertReason, RuntimeSignal};

/// JSON-RPC 2.0 Request envelope
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    /// JSON-RPC version (always "2.0")
    pub jsonrpc: String,
    /// Request ID for correlation
    pub id: Uuid,
    /// Method name
    pub method: String,
    /// Request parameters
    pub params: RequestParams,
}

/// Request parameters (method-specific)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequestParams {
    /// evolve method params
    Evolve(EvolvParams),
    /// solidify method params
    Solidify(SolidifyParams),
    /// revert method params
    Revert(RevertParams),
    /// query method params
    Query(QueryParams),
    /// list method params
    List(ListParams),
    /// ping method (no params)
    Ping(Option<PingParams>),
}

/// Parameters for evolve method
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvolvParams {
    /// The signal to evolve from
    pub signal: RuntimeSignal,
    /// Evolution context
    pub context: EvolutionContext,
}

/// Parameters for solidify method
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolidifyParams {
    /// Gene ID to solidify
    pub gene_id: Uuid,
}

/// Parameters for revert method
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevertParams {
    /// Revert details
    pub reason: RevertReason,
}

/// Parameters for query method
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryParams {
    /// Query details
    pub query: GeneQuery,
}

/// Parameters for list method
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListParams {
    /// Optional limit
    pub limit: Option<usize>,
    /// Optional offset
    pub offset: Option<usize>,
}

/// Parameters for ping method
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PingParams {
    /// Optional message
    pub message: Option<String>,
}

impl JsonRpcRequest {
    /// Create a new evolve request
    pub fn evolve(signal: RuntimeSignal, context: EvolutionContext) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: Uuid::new_v4(),
            method: "evolve".to_string(),
            params: RequestParams::Evolve(EvolvParams { signal, context }),
        }
    }

    /// Create a new solidify request
    pub fn solidify(gene_id: Uuid) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: Uuid::new_v4(),
            method: "solidify".to_string(),
            params: RequestParams::Solidify(SolidifyParams { gene_id }),
        }
    }

    /// Create a new revert request
    pub fn revert(reason: RevertReason) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: Uuid::new_v4(),
            method: "revert".to_string(),
            params: RequestParams::Revert(RevertParams { reason }),
        }
    }

    /// Create a new query request
    pub fn query(query: GeneQuery) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: Uuid::new_v4(),
            method: "query".to_string(),
            params: RequestParams::Query(QueryParams { query }),
        }
    }

    /// Create a new list request
    pub fn list(limit: Option<usize>, offset: Option<usize>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: Uuid::new_v4(),
            method: "list".to_string(),
            params: RequestParams::List(ListParams { limit, offset }),
        }
    }

    /// Create a new ping request
    pub fn ping(message: Option<String>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: Uuid::new_v4(),
            method: "ping".to_string(),
            params: RequestParams::Ping(Some(PingParams { message })),
        }
    }

    /// Parse a JSON string into a request
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Serialize this request to JSON
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RuntimeSignalType;

    #[test]
    fn test_evolve_request() {
        let signal = RuntimeSignal {
            id: Uuid::new_v4(),
            signal_type: RuntimeSignalType::CompilerError,
            content: "test error".to_string(),
            location: None,
            severity: 0.5,
            timestamp: chrono::Utc::now(),
        };

        let context = EvolutionContext {
            session_id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            workspace: "/tmp/test".to_string(),
            user_confirmation: None,
        };

        let request = JsonRpcRequest::evolve(signal, context);
        let json = request.to_json().unwrap();
        let parsed = JsonRpcRequest::from_json(&json).unwrap();

        assert_eq!(parsed.method, "evolve");
        assert_eq!(parsed.jsonrpc, "2.0");
    }
}
