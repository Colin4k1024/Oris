use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FederatedQuery {
    pub query: String,
    pub task_class: Option<String>,
    pub min_confidence: Option<f64>,
    pub timeout_ms: Option<u64>,
    pub target_nodes: Option<Vec<String>>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FederatedResult {
    pub results: Vec<GeneResult>,
    pub meta: FederationMeta,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneResult {
    pub gene_id: String,
    pub name: String,
    pub task_class: String,
    pub confidence: f64,
    pub source_node: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FederationMeta {
    pub nodes_queried: usize,
    pub nodes_responded: usize,
    pub coverage: f64,
    pub freshness: DateTime<Utc>,
    pub timeout_nodes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeSearchResponse {
    pub genes: Vec<GeneResult>,
}
