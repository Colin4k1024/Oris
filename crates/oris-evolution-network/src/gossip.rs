//! Peer discovery and gossip protocol for the Evolution Network.
//!
//! This module provides:
//! - Static peer list configuration
//! - Peer health monitoring
//! - Basic gossip protocol for event propagation

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Instant;

use chrono::Utc;
use serde::{Deserialize, Serialize};

/// Configuration for peer discovery
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PeerConfig {
    /// List of peer endpoints for discovery
    pub peers: Vec<PeerEndpoint>,
    /// Heartbeat interval for peer health checks
    #[serde(default = "default_heartbeat_interval")]
    pub heartbeat_interval_secs: u64,
    /// Timeout for peer responses
    #[serde(default = "default_peer_timeout_secs")]
    pub peer_timeout_secs: u64,
    /// Gossip fanout (number of peers to spread messages to)
    #[serde(default = "default_fanout")]
    pub gossip_fanout: usize,
}

fn default_heartbeat_interval() -> u64 {
    30
}
fn default_peer_timeout_secs() -> u64 {
    10
}
fn default_fanout() -> usize {
    3
}

/// A peer endpoint in the network
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PeerEndpoint {
    /// Unique identifier for the peer
    pub peer_id: String,
    /// HTTP endpoint for the peer
    pub endpoint: String,
    /// Optional public key for authentication
    pub public_key: Option<String>,
}

/// Status of a peer
#[derive(Clone, Debug, PartialEq)]
pub enum PeerStatus {
    /// Peer is active and responding
    Active,
    /// Peer is suspected to be offline
    Suspected,
    /// Peer is confirmed offline
    Offline,
}

/// Information about a known peer
#[derive(Clone, Debug)]
pub struct PeerInfo {
    pub endpoint: PeerEndpoint,
    pub status: PeerStatus,
    pub last_seen: Instant,
    pub last_heartbeat: Option<Instant>,
    pub failure_count: u32,
}

impl PeerInfo {
    pub fn new(endpoint: PeerEndpoint) -> Self {
        Self {
            endpoint,
            status: PeerStatus::Active,
            last_seen: Instant::now(),
            last_heartbeat: None,
            failure_count: 0,
        }
    }

    pub fn mark_failure(&mut self) {
        self.failure_count += 1;
        if self.failure_count >= 3 {
            self.status = PeerStatus::Offline;
        } else {
            self.status = PeerStatus::Suspected;
        }
    }

    pub fn mark_success(&mut self) {
        self.failure_count = 0;
        self.status = PeerStatus::Active;
        self.last_seen = Instant::now();
    }
}

/// Gossip message for peer-to-peer communication
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GossipMessage {
    /// Unique message identifier
    pub message_id: String,
    /// Origin peer ID
    pub origin_peer: String,
    /// Sequence number for ordering
    pub sequence: u64,
    /// Message type
    pub kind: GossipKind,
    /// Timestamp
    pub timestamp: String,
    /// Message payload (JSON)
    pub payload: String,
}

/// Types of gossip messages
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GossipKind {
    /// Peer advertisement
    Advertisement { peer_id: String, endpoint: String },
    /// Asset update (gene, capsule, event)
    AssetUpdate {
        asset_id: String,
        asset_type: String,
    },
    /// State synchronization request
    SyncRequest { since_sequence: u64 },
    /// State synchronization response
    SyncResponse { assets: Vec<String> },
    /// Peer leave notification
    Leave { peer_id: String },
}

/// Peer registry for managing known peers
#[derive(Clone)]
pub struct PeerRegistry {
    peers: Arc<RwLock<HashMap<String, PeerInfo>>>,
    config: PeerConfig,
    local_peer_id: String,
}

impl PeerRegistry {
    /// Create a new peer registry from config
    pub fn new(config: PeerConfig, local_peer_id: String) -> Self {
        let peers: HashMap<String, PeerInfo> = config
            .peers
            .iter()
            .map(|e| (e.peer_id.clone(), PeerInfo::new(e.clone())))
            .collect();

        Self {
            peers: Arc::new(RwLock::new(peers)),
            config,
            local_peer_id,
        }
    }

    /// Get all active peers
    pub fn get_active_peers(&self) -> Vec<PeerEndpoint> {
        self.peers
            .read()
            .unwrap()
            .values()
            .filter(|p| p.status == PeerStatus::Active)
            .map(|p| p.endpoint.clone())
            .collect()
    }

    /// Get a random sample of peers for gossip
    pub fn get_gossip_peers(&self, count: usize) -> Vec<PeerEndpoint> {
        let peers = self.peers.read().unwrap();
        let active: Vec<_> = peers
            .values()
            .filter(|p| p.status == PeerStatus::Active)
            .filter(|p| p.endpoint.peer_id != self.local_peer_id)
            .map(|p| p.endpoint.clone())
            .collect();

        if active.is_empty() {
            return vec![];
        }

        // Simple round-robin selection
        let count = count.min(active.len());
        active.into_iter().take(count).collect()
    }

    /// Update peer status based on heartbeat
    pub fn update_peer_status(&self, peer_id: &str, is_alive: bool) {
        let mut peers = self.peers.write().unwrap();
        if let Some(peer) = peers.get_mut(peer_id) {
            if is_alive {
                peer.mark_success();
                peer.last_heartbeat = Some(Instant::now());
            } else {
                peer.mark_failure();
            }
        }
    }

    /// Add a new peer discovered via gossip
    pub fn add_peer(&self, endpoint: PeerEndpoint) {
        let mut peers = self.peers.write().unwrap();
        if !peers.contains_key(&endpoint.peer_id) {
            peers.insert(endpoint.peer_id.clone(), PeerInfo::new(endpoint));
        }
    }

    /// Remove a peer
    pub fn remove_peer(&self, peer_id: &str) {
        let mut peers = self.peers.write().unwrap();
        peers.remove(peer_id);
    }

    /// Get local peer ID
    pub fn local_peer_id(&self) -> &str {
        &self.local_peer_id
    }

    /// Get config
    pub fn config(&self) -> &PeerConfig {
        &self.config
    }
}

/// Builder for creating gossip messages
pub struct GossipBuilder {
    origin_peer: String,
    sequence: u64,
    kind: Option<GossipKind>,
    payload: Option<String>,
}

impl GossipBuilder {
    pub fn new(origin_peer: String, sequence: u64) -> Self {
        Self {
            origin_peer,
            sequence,
            kind: None,
            payload: None,
        }
    }

    pub fn advertisement(mut self, peer_id: String, endpoint: String) -> Self {
        self.kind = Some(GossipKind::Advertisement { peer_id, endpoint });
        self
    }

    pub fn asset_update(mut self, asset_id: String, asset_type: String) -> Self {
        self.kind = Some(GossipKind::AssetUpdate {
            asset_id,
            asset_type,
        });
        self
    }

    pub fn sync_request(mut self, since_sequence: u64) -> Self {
        self.kind = Some(GossipKind::SyncRequest { since_sequence });
        self
    }

    pub fn sync_response(mut self, assets: Vec<String>) -> Self {
        self.kind = Some(GossipKind::SyncResponse { assets });
        self
    }

    pub fn leave(mut self, peer_id: String) -> Self {
        self.kind = Some(GossipKind::Leave { peer_id });
        self
    }

    pub fn payload(mut self, payload: String) -> Self {
        self.payload = Some(payload);
        self
    }

    pub fn build(self) -> Option<GossipMessage> {
        let kind = self.kind?;
        let payload = self
            .payload
            .unwrap_or_else(|| serde_json::to_string(&kind).unwrap_or_default());

        Some(GossipMessage {
            message_id: format!(
                "gossip-{:x}",
                Utc::now().timestamp_nanos_opt().unwrap_or_default()
            ),
            origin_peer: self.origin_peer,
            sequence: self.sequence,
            kind,
            timestamp: Utc::now().to_rfc3339(),
            payload,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_peer_registry_creation() {
        let config = PeerConfig {
            peers: vec![
                PeerEndpoint {
                    peer_id: "peer1".into(),
                    endpoint: "http://peer1:8080".into(),
                    public_key: None,
                },
                PeerEndpoint {
                    peer_id: "peer2".into(),
                    endpoint: "http://peer2:8080".into(),
                    public_key: None,
                },
            ],
            heartbeat_interval_secs: 30,
            peer_timeout_secs: 10,
            gossip_fanout: 3,
        };

        let registry = PeerRegistry::new(config, "local-peer".to_string());
        let active = registry.get_active_peers();
        assert_eq!(active.len(), 2);
    }

    #[test]
    fn test_peer_failure_tracking() {
        let config = PeerConfig {
            peers: vec![PeerEndpoint {
                peer_id: "peer1".into(),
                endpoint: "http://peer1:8080".into(),
                public_key: None,
            }],
            heartbeat_interval_secs: 30,
            peer_timeout_secs: 10,
            gossip_fanout: 3,
        };

        let registry = PeerRegistry::new(config, "local-peer".into());

        // Simulate failures
        registry.update_peer_status("peer1", false);
        registry.update_peer_status("peer1", false);

        let peers = registry.get_active_peers();
        assert!(peers.is_empty()); // Status should be Suspected

        // Recover
        registry.update_peer_status("peer1", true);
        let peers = registry.get_active_peers();
        assert_eq!(peers.len(), 1);
    }

    #[test]
    fn test_gossip_builder() {
        let msg = GossipBuilder::new("peer1".to_string(), 1)
            .asset_update("asset-123".to_string(), "gene".to_string())
            .build();

        assert!(msg.is_some());
        let msg = msg.unwrap();
        assert_eq!(msg.origin_peer, "peer1");
        assert_eq!(msg.sequence, 1);
    }
}
