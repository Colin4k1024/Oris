use chrono::Utc;
use std::sync::Arc;
use tracing::{info, warn};

use super::store::RegistryStore;
use super::types::*;
use crate::error::HubError;

const DEFAULT_TTL_SECONDS: u64 = 60;
const DEFAULT_HEARTBEAT_INTERVAL: u64 = 30;

pub struct RegistryService {
    store: Arc<dyn RegistryStore>,
}

impl RegistryService {
    pub fn new(store: Arc<dyn RegistryStore>) -> Self {
        Self { store }
    }

    pub async fn register(&self, req: RegisterRequest) -> Result<RegisterResponse, HubError> {
        // Reject if node_id is already registered (prevents key substitution attacks)
        if let Some(existing) = self.store.get_node(&req.node_id).await? {
            if existing.public_key != req.public_key {
                return Err(HubError::Conflict(format!(
                    "node '{}' is already registered with a different key",
                    req.node_id
                )));
            }
        }

        let now = Utc::now();
        let node = NodeInfo {
            node_id: req.node_id.clone(),
            endpoint: req.endpoint,
            public_key: req.public_key,
            capabilities: req.capabilities,
            region: req.region,
            version: req.version,
            status: NodeStatus::Active,
            registered_at: now,
            last_heartbeat: now,
            ttl_seconds: DEFAULT_TTL_SECONDS,
        };

        self.store.upsert_node(&node).await?;
        info!(node_id = %req.node_id, "node registered");

        Ok(RegisterResponse {
            node_id: req.node_id,
            heartbeat_interval_seconds: DEFAULT_HEARTBEAT_INTERVAL,
            ttl_seconds: DEFAULT_TTL_SECONDS,
        })
    }

    pub async fn heartbeat(&self, req: HeartbeatRequest) -> Result<HeartbeatResponse, HubError> {
        let existing = self.store.get_node(&req.node_id).await?;
        if existing.is_none() {
            return Err(HubError::NodeNotFound(req.node_id));
        }

        self.store
            .refresh_heartbeat(&req.node_id, req.status)
            .await?;

        Ok(HeartbeatResponse {
            acknowledged: true,
            next_heartbeat_seconds: DEFAULT_HEARTBEAT_INTERVAL,
        })
    }

    pub async fn deregister(&self, node_id: &str) -> Result<(), HubError> {
        self.store.remove_node(node_id).await?;
        info!(node_id = %node_id, "node deregistered");
        Ok(())
    }

    pub async fn get_node(&self, node_id: &str) -> Result<NodeInfo, HubError> {
        self.store
            .get_node(node_id)
            .await?
            .ok_or_else(|| HubError::NodeNotFound(node_id.to_string()))
    }

    pub async fn list_active_nodes(&self) -> Result<Vec<NodeInfo>, HubError> {
        let nodes = self.store.list_nodes().await?;
        let now = Utc::now();
        Ok(nodes
            .into_iter()
            .filter(|n| {
                let elapsed = (now - n.last_heartbeat).num_seconds() as u64;
                elapsed < n.ttl_seconds
            })
            .collect())
    }

    pub async fn gc(&self) -> Result<u64, HubError> {
        let removed = self.store.gc_expired_nodes().await?;
        if removed > 0 {
            warn!(removed = removed, "GC removed expired nodes");
        }
        Ok(removed)
    }
}
