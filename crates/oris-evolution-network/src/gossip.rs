//! Peer discovery and gossip protocol for the Evolution Network.
//!
//! This module provides:
//! - Static peer list configuration
//! - Peer health monitoring
//! - Basic gossip protocol for event propagation

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

use crate::{EvolutionEnvelope, FetchQuery, FetchResponse, NetworkAsset, SyncAudit};
use chrono::Utc;
use oris_evolution::Gene;
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

pub type PeerAddress = String;

fn default_sync_interval_secs() -> u64 {
    30
}

fn default_broadcast_threshold() -> f32 {
    0.8
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GossipConfig {
    #[serde(default)]
    pub peers: Vec<PeerAddress>,
    #[serde(default = "default_sync_interval_secs")]
    pub sync_interval_secs: u64,
    #[serde(default = "default_broadcast_threshold")]
    pub broadcast_threshold: f32,
}

impl Default for GossipConfig {
    fn default() -> Self {
        Self {
            peers: Vec::new(),
            sync_interval_secs: default_sync_interval_secs(),
            broadcast_threshold: default_broadcast_threshold(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GossipDigestEntry {
    pub gene_id: String,
    pub confidence: f32,
    pub version: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GossipDigest {
    pub sender_id: String,
    #[serde(default)]
    pub genes: Vec<GossipDigestEntry>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GossipSyncReport {
    pub requested_gene_ids: Vec<String>,
    pub imported_gene_ids: Vec<String>,
}

#[derive(Clone, Debug)]
struct LocalGeneRecord {
    envelope: EvolutionEnvelope,
    confidence: f32,
    version: u64,
}

/// Push-pull gossip engine for in-process synchronization tests and example flows.
pub struct GossipSyncEngine {
    local_peer_id: String,
    peers: Vec<PeerAddress>,
    config: GossipConfig,
    records: Arc<RwLock<HashMap<String, LocalGeneRecord>>>,
    next_version: Mutex<u64>,
}

impl GossipSyncEngine {
    pub fn new(local_peer_id: impl Into<String>, config: GossipConfig) -> Self {
        Self {
            local_peer_id: local_peer_id.into(),
            peers: config.peers.clone(),
            config,
            records: Arc::new(RwLock::new(HashMap::new())),
            next_version: Mutex::new(0),
        }
    }

    pub fn peers(&self) -> &[PeerAddress] {
        &self.peers
    }

    pub fn config(&self) -> &GossipConfig {
        &self.config
    }

    pub fn local_peer_id(&self) -> &str {
        &self.local_peer_id
    }

    pub fn has_gene(&self, gene_id: &str) -> bool {
        self.records.read().unwrap().contains_key(gene_id)
    }

    pub fn gene_version(&self, gene_id: &str) -> Option<u64> {
        self.records.read().unwrap().get(gene_id).map(|r| r.version)
    }

    pub fn register_envelope(&self, envelope: EvolutionEnvelope) -> usize {
        let genes: Vec<Gene> = envelope
            .assets
            .iter()
            .filter_map(|asset| match asset {
                NetworkAsset::Gene { gene } => Some(gene.clone()),
                _ => None,
            })
            .collect();

        let mut version_counter = self.next_version.lock().unwrap();
        let mut records = self.records.write().unwrap();
        let mut inserted = 0;
        for gene in genes {
            *version_counter += 1;
            let confidence = confidence_for_gene(&envelope.assets, &gene.id);
            records.insert(
                gene.id.clone(),
                LocalGeneRecord {
                    envelope: envelope.clone(),
                    confidence,
                    version: *version_counter,
                },
            );
            inserted += 1;
        }
        inserted
    }

    pub fn build_digest(&self) -> GossipDigest {
        let genes = self
            .records
            .read()
            .unwrap()
            .iter()
            .filter(|(_, record)| record.confidence >= self.config.broadcast_threshold)
            .map(|(gene_id, record)| GossipDigestEntry {
                gene_id: gene_id.clone(),
                confidence: record.confidence,
                version: record.version,
            })
            .collect();

        GossipDigest {
            sender_id: self.local_peer_id.clone(),
            genes,
        }
    }

    pub fn build_fetch_query_for_digest(&self, digest: &GossipDigest) -> FetchQuery {
        let local = self.records.read().unwrap();
        let requested_gene_ids = digest
            .genes
            .iter()
            .filter(|entry| {
                local
                    .get(&entry.gene_id)
                    .map(|record| record.version < entry.version)
                    .unwrap_or(true)
            })
            .map(|entry| entry.gene_id.clone())
            .collect::<Vec<_>>();

        FetchQuery {
            sender_id: self.local_peer_id.clone(),
            signals: requested_gene_ids,
            since_cursor: None,
            resume_token: None,
        }
    }

    pub fn respond_to_fetch(&self, query: &FetchQuery) -> FetchResponse {
        let records = self.records.read().unwrap();
        let mut assets = Vec::new();
        let mut applied = 0usize;
        for gene_id in &query.signals {
            if let Some(record) = records.get(gene_id) {
                assets.extend(record.envelope.assets.clone());
                applied += 1;
            }
        }

        FetchResponse {
            sender_id: self.local_peer_id.clone(),
            assets,
            next_cursor: None,
            resume_token: None,
            sync_audit: SyncAudit {
                batch_id: format!("gossip-fetch-{}-{}", self.local_peer_id, Utc::now().timestamp()),
                requested_cursor: None,
                scanned_count: query.signals.len(),
                applied_count: applied,
                skipped_count: query.signals.len().saturating_sub(applied),
                failed_count: 0,
                failure_reasons: vec![],
            },
        }
    }

    pub fn apply_fetch_response(&self, response: &FetchResponse) -> Vec<String> {
        if response.assets.is_empty() {
            return vec![];
        }
        let envelope = EvolutionEnvelope::publish(response.sender_id.clone(), response.assets.clone());
        let imported = envelope
            .assets
            .iter()
            .filter_map(|asset| match asset {
                NetworkAsset::Gene { gene } => Some(gene.id.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let _ = self.register_envelope(envelope);
        imported
    }

    pub async fn sync_once_with(&self, remote: &GossipSyncEngine) -> GossipSyncReport {
        let digest = remote.build_digest();
        let query = self.build_fetch_query_for_digest(&digest);
        let requested_gene_ids = query.signals.clone();
        let response = remote.respond_to_fetch(&query);
        let imported_gene_ids = self.apply_fetch_response(&response);
        GossipSyncReport {
            requested_gene_ids,
            imported_gene_ids,
        }
    }

    pub async fn start_sync_loop(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let interval = std::time::Duration::from_secs(self.config.sync_interval_secs.max(1));
            loop {
                tokio::time::sleep(interval).await;
            }
        })
    }

    pub fn serialize_digest_json(digest: &GossipDigest) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(digest)
    }

    #[cfg(feature = "gossip-msgpack")]
    pub fn serialize_digest_msgpack(
        digest: &GossipDigest,
    ) -> Result<Vec<u8>, rmp_serde::encode::Error> {
        rmp_serde::to_vec_named(digest)
    }

    #[cfg(feature = "gossip-msgpack")]
    pub fn deserialize_digest_msgpack(
        bytes: &[u8],
    ) -> Result<GossipDigest, rmp_serde::decode::Error> {
        rmp_serde::from_slice(bytes)
    }
}

fn confidence_for_gene(assets: &[NetworkAsset], gene_id: &str) -> f32 {
    assets
        .iter()
        .filter_map(|asset| match asset {
            NetworkAsset::Capsule { capsule } if capsule.gene_id == gene_id => Some(capsule.confidence),
            _ => None,
        })
        .fold(0.0_f32, f32::max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EvolutionEnvelope;
    use oris_evolution::{AssetState, Capsule, EnvFingerprint, Outcome};

    fn sample_gene_asset(id: &str) -> NetworkAsset {
        NetworkAsset::Gene {
            gene: Gene {
                id: id.to_string(),
                signals: vec!["compiler:error[E0308]".to_string()],
                strategy: vec!["fix type mismatch".to_string()],
                validation: vec!["cargo test".to_string()],
                state: AssetState::Promoted,
                task_class_id: None,
            },
        }
    }

    fn sample_capsule_asset(id: &str, gene_id: &str, confidence: f32) -> NetworkAsset {
        NetworkAsset::Capsule {
            capsule: Capsule {
                id: id.to_string(),
                gene_id: gene_id.to_string(),
                mutation_id: format!("mut-{id}"),
                run_id: format!("run-{id}"),
                diff_hash: format!("diff-{id}"),
                confidence,
                env: EnvFingerprint {
                    rustc_version: "rustc 1.80.0".to_string(),
                    cargo_lock_hash: "cargo-lock".to_string(),
                    target_triple: "aarch64-apple-darwin".to_string(),
                    os: "macos".to_string(),
                },
                outcome: Outcome {
                    success: true,
                    validation_profile: "default".to_string(),
                    validation_duration_ms: 100,
                    changed_files: vec!["src/lib.rs".to_string()],
                    validator_hash: "validator".to_string(),
                    lines_changed: 3,
                    replay_verified: true,
                },
                state: AssetState::Promoted,
            },
        }
    }

    fn sample_envelope(gene_id: &str, confidence: f32) -> EvolutionEnvelope {
        EvolutionEnvelope::publish(
            "node-a",
            vec![
                sample_gene_asset(gene_id),
                sample_capsule_asset(&format!("capsule-{gene_id}"), gene_id, confidence),
            ],
        )
    }

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

    #[test]
    fn gossip_digest_only_includes_genes_above_threshold() {
        let engine = GossipSyncEngine::new(
            "node-a",
            GossipConfig {
                peers: vec!["node-b".into()],
                sync_interval_secs: 30,
                broadcast_threshold: 0.8,
            },
        );
        engine.register_envelope(sample_envelope("gene-high", 0.92));
        engine.register_envelope(sample_envelope("gene-low", 0.42));

        let digest = engine.build_digest();
        assert_eq!(digest.genes.len(), 1);
        assert_eq!(digest.genes[0].gene_id, "gene-high");
        assert!(digest.genes[0].confidence >= 0.8);
    }

    #[test]
    fn fetch_query_response_round_trip_returns_requested_gene() {
        let producer = GossipSyncEngine::new("node-a", GossipConfig::default());
        producer.register_envelope(sample_envelope("gene-1", 0.95));

        let query = FetchQuery {
            sender_id: "node-b".into(),
            signals: vec!["gene-1".into()],
            since_cursor: None,
            resume_token: None,
        };
        let response = producer.respond_to_fetch(&query);

        assert_eq!(response.sender_id, "node-a");
        assert!(!response.assets.is_empty());
        assert_eq!(response.sync_audit.applied_count, 1);
        assert!(response.assets.iter().any(|asset| matches!(
            asset,
            NetworkAsset::Gene { gene } if gene.id == "gene-1"
        )));
    }

    #[tokio::test]
    async fn two_in_process_gossip_sync_engines_exchange_gene_within_one_cycle() {
        let producer = GossipSyncEngine::new(
            "node-a",
            GossipConfig {
                peers: vec!["node-b".into()],
                sync_interval_secs: 30,
                broadcast_threshold: 0.8,
            },
        );
        let consumer = GossipSyncEngine::new(
            "node-b",
            GossipConfig {
                peers: vec!["node-a".into()],
                sync_interval_secs: 30,
                broadcast_threshold: 0.8,
            },
        );

        producer.register_envelope(sample_envelope("gene-sync", 0.91));
        assert!(!consumer.has_gene("gene-sync"));

        let report = consumer.sync_once_with(&producer).await;
        assert_eq!(report.requested_gene_ids, vec!["gene-sync".to_string()]);
        assert_eq!(report.imported_gene_ids, vec!["gene-sync".to_string()]);
        assert!(consumer.has_gene("gene-sync"));
    }
}
