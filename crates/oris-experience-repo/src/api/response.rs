//! API response types.

use serde::{Deserialize, Serialize};

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
    },
    Capsule {
        id: String,
        gene_id: String,
        confidence: f64,
        quality_score: f64,
        use_count: u64,
        success_count: u64,
        created_at: String,
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

/// Error response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: String,
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
