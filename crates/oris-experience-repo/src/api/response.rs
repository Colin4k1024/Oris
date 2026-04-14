//! API response types.

use serde::{Deserialize, Serialize};

use crate::key_service::{ApiKeyInfo, PublicKey};

/// Sync audit information.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncAudit {
    #[serde(default)]
    pub scanned_count: usize,
    #[serde(default)]
    pub applied_count: usize,
    #[serde(default)]
    pub skipped_count: usize,
    #[serde(default)]
    pub failed_count: usize,
}

/// A network asset wrapper for Gene or Capsule.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NetworkAsset {
    Gene {
        id: String,
        signals: Vec<String>,
        strategy: Vec<String>,
        validation: Vec<String>,
        confidence: f64,
        quality_score: f64,
        use_count: u64,
        success_count: u64,
        created_at: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        contributor_id: Option<String>,
    },
    Capsule {
        id: String,
        gene_id: String,
        confidence: f64,
        quality_score: f64,
        use_count: u64,
        success_count: u64,
        created_at: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        contributor_id: Option<String>,
    },
}

/// Response for fetching experiences.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchResponse {
    pub assets: Vec<NetworkAsset>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    pub sync_audit: SyncAudit,
}

/// Response for sharing experiences.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareResponse {
    /// The stored gene ID.
    pub gene_id: String,
    /// Publication status.
    pub status: String,
    /// When the gene was published.
    pub published_at: String,
}

/// Error response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
}

/// Health check response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

impl HealthResponse {
    pub fn ok() -> Self {
        Self {
            status: "ok".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

/// Response for creating an API key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateKeyResponse {
    /// The new key ID.
    pub key_id: String,
    /// The raw API key (only shown once).
    pub api_key: String,
    /// The agent ID this key belongs to.
    pub agent_id: String,
    /// When the key was created.
    pub created_at: String,
    /// When the key expires.
    pub expires_at: Option<String>,
}

/// Response for listing API keys.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListKeysResponse {
    pub keys: Vec<ApiKeyInfo>,
}

/// Response for rotating an API key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RotateKeyResponse {
    /// The key ID (unchanged).
    pub key_id: String,
    /// The new raw API key (only shown once).
    pub api_key: String,
    /// When the key was rotated.
    pub rotated_at: String,
}

/// Response for registering a public key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterPublicKeyResponse {
    /// The sender ID this public key belongs to.
    pub sender_id: String,
    /// The version of the registered public key.
    pub version: i32,
    /// When the public key was registered.
    pub created_at: String,
}

/// Response for listing public keys.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListPublicKeysResponse {
    pub keys: Vec<PublicKeyInfo>,
}

/// Public key info for API responses (without the actual key).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicKeyInfo {
    pub sender_id: String,
    pub version: i32,
    pub status: String,
    pub created_at: String,
}

impl From<&PublicKey> for PublicKeyInfo {
    fn from(pk: &PublicKey) -> Self {
        Self {
            sender_id: pk.sender_id.clone(),
            version: pk.version,
            status: pk.status.to_string(),
            created_at: pk.created_at.to_rfc3339(),
        }
    }
}
