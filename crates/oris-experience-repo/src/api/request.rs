//! API request types.

use serde::Deserialize;

use crate::oen::OenEnvelope;

/// Query parameters for fetching experiences.
#[derive(Debug, Clone, Deserialize)]
pub struct FetchQuery {
    /// Comma-separated problem signals (e.g., "timeout,error")
    #[serde(default)]
    pub q: Option<String>,

    /// Minimum confidence threshold (default: 0.5)
    #[serde(default = "default_min_confidence")]
    pub min_confidence: f64,

    /// Maximum number of results (default: 10)
    #[serde(default = "default_limit")]
    pub limit: usize,

    /// Pagination cursor
    #[serde(default)]
    pub cursor: Option<String>,
}

fn default_min_confidence() -> f64 {
    0.5
}

fn default_limit() -> usize {
    10
}

impl FetchQuery {
    /// Parse the query string into signals.
    pub fn signals(&self) -> Vec<String> {
        self.q
            .as_ref()
            .map(|q| {
                q.split(',')
                    .map(|s| s.trim().to_lowercase())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Request body for sharing experiences.
#[derive(Debug, Clone, Deserialize)]
pub struct ShareRequest {
    /// OEN Envelope containing the experience to share.
    pub envelope: OenEnvelope,
}

/// Request body for creating an API key.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateKeyRequest {
    /// Agent ID this key belongs to.
    pub agent_id: String,
    /// Optional TTL in days.
    pub ttl_days: Option<i64>,
    /// Human-readable description.
    pub description: Option<String>,
}

/// Request body for rotating an API key.
#[derive(Debug, Clone, Deserialize)]
pub struct RotateKeyRequest {
    /// Optional TTL in days for the new key.
    pub ttl_days: Option<i64>,
}

/// Request body for registering a public key.
#[derive(Debug, Clone, Deserialize)]
pub struct RegisterPublicKeyRequest {
    /// Sender ID this public key belongs to.
    pub sender_id: String,
    /// 32-byte hex-encoded Ed25519 public key.
    pub public_key_hex: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fetch_query_signals() {
        let query = FetchQuery {
            q: Some("timeout,error".to_string()),
            min_confidence: 0.5,
            limit: 10,
            cursor: None,
        };

        let signals = query.signals();
        assert_eq!(signals, vec!["timeout", "error"]);
    }

    #[test]
    fn test_fetch_query_signals_with_spaces() {
        let query = FetchQuery {
            q: Some(" timeout , error , memory ".to_string()),
            min_confidence: 0.5,
            limit: 10,
            cursor: None,
        };

        let signals = query.signals();
        assert_eq!(signals, vec!["timeout", "error", "memory"]);
    }

    #[test]
    fn test_fetch_query_signals_empty() {
        let query = FetchQuery {
            q: None,
            min_confidence: 0.5,
            limit: 10,
            cursor: None,
        };

        let signals = query.signals();
        assert!(signals.is_empty());
    }
}
