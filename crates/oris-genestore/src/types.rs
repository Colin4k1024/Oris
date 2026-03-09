//! oris-genestore/src/types.rs
//!
//! Domain types for Gene and Capsule storage.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Gene - a reusable evolution strategy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gene {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub template: String,
    pub preconditions: Vec<String>,
    pub validation_steps: Vec<String>,
    pub confidence: f64,
    pub use_count: u64,
    pub success_count: u64,
    pub quality_score: f64,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub last_boosted_at: Option<DateTime<Utc>>,
}

impl Gene {
    /// Confidence thresholds
    pub const STALE_THRESHOLD: f64 = 0.30;
    pub const DECAY_PER_QUERY: f64 = 0.002;
    pub const BOOST_ON_SUCCESS: f64 = 0.05;
    pub const PENALTY_ON_FAILURE: f64 = 0.08;
}

/// Capsule - a successful evolution instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capsule {
    pub id: Uuid,
    pub gene_id: Uuid,
    pub content: String,
    pub env_fingerprint: String,
    pub quality_score: f64,
    pub confidence: f64,
    pub use_count: u64,
    pub success_count: u64,
    pub last_replay_run_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}

impl Capsule {
    pub const STALE_THRESHOLD: f64 = 0.30;
    pub const DECAY_PER_QUERY: f64 = 0.002;
    pub const PENALTY_ON_FAILURE: f64 = 0.08;
    pub const BOOST_ON_SUCCESS: f64 = 0.05;
}

/// Query for searching genes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneQuery {
    /// Minimum confidence threshold.
    pub min_confidence: f64,
    /// Maximum number of results.
    pub limit: usize,
    /// Required tags (AND logic).
    pub required_tags: Vec<String>,
    /// Problem description for relevance scoring.
    pub problem_description: String,
}

impl Default for GeneQuery {
    fn default() -> Self {
        Self {
            min_confidence: 0.5,
            limit: 10,
            required_tags: vec![],
            problem_description: String::new(),
        }
    }
}

/// A gene with computed relevance score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneMatch {
    pub gene: Gene,
    pub relevance_score: f64,
}
