use std::sync::Arc;

use super::types::*;
use crate::error::HubError;
use crate::registry::RegistryService;

pub struct DiscoveryService {
    registry: Arc<RegistryService>,
}

impl DiscoveryService {
    pub fn new(registry: Arc<RegistryService>) -> Self {
        Self { registry }
    }

    pub async fn discover(&self, query: DiscoveryQuery) -> Result<DiscoveryResult, HubError> {
        let mut nodes = self.registry.list_active_nodes().await?;

        if let Some(ref caps) = query.capabilities {
            nodes.retain(|n| caps.iter().all(|c| n.capabilities.contains(c)));
        }

        if let Some(ref region) = query.region {
            nodes.retain(|n| n.region.as_deref() == Some(region.as_str()));
        }

        if let Some(ref version) = query.version {
            nodes.retain(|n| &n.version == version);
        }

        let total = nodes.len();

        if let Some(limit) = query.limit {
            nodes.truncate(limit);
        }

        Ok(DiscoveryResult { nodes, total })
    }
}
