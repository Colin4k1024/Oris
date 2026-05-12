use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    pub node_id: String,
    pub endpoint: String,
    pub public_key: String,
    pub capabilities: Vec<String>,
    pub region: Option<String>,
    pub version: String,
    pub status: NodeStatus,
    pub registered_at: DateTime<Utc>,
    pub last_heartbeat: DateTime<Utc>,
    pub ttl_seconds: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeStatus {
    Active,
    Degraded,
    Offline,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterRequest {
    pub node_id: String,
    pub endpoint: String,
    pub public_key: String,
    pub capabilities: Vec<String>,
    pub region: Option<String>,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterResponse {
    pub node_id: String,
    pub heartbeat_interval_seconds: u64,
    pub ttl_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatRequest {
    pub node_id: String,
    pub status: Option<NodeStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatResponse {
    pub acknowledged: bool,
    pub next_heartbeat_seconds: u64,
}
