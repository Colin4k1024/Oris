use super::super::registry::types::NodeInfo;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryQuery {
    pub capabilities: Option<Vec<String>>,
    pub region: Option<String>,
    pub version: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryResult {
    pub nodes: Vec<NodeInfo>,
    pub total: usize,
}
