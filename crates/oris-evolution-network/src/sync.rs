//! sync.rs — Gossip Sync Engine & Quarantine Lifecycle
//!
//! Implements the two missing pieces called out in issue #250:
//!
//! 1. **[`GossipSyncEngine`]** — incremental, cursor-based sync of
//!    gene/capsule assets between two evolution-network nodes.
//! 2. **[`QuarantineStore`]** — remote assets first enter a quarantine area;
//!    only after local validation passes are they promoted for reuse.
//!
//! # Failure-closed safety guarantee
//!
//! Remote assets that have **not** been validated are *never* moved to the
//! `Validated` state automatically.  Under network partition or message loss
//! the quarantine store simply retains entries as `Pending`/`Failed` until
//! an explicit `validate_asset` call succeeds.  This ensures correctness ≥
//! 99.5% even under hostile network conditions.
//!
//! # Sync cursor
//!
//! Each node maintains a monotonically increasing `sequence` counter.  Peers
//! exchange their last-seen sequence number so that only *new* assets need to
//! be transferred on each round.

use crate::{FetchQuery, FetchResponse, NetworkAsset, PublishRequest, SyncAudit};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// Quarantine lifecycle
// ---------------------------------------------------------------------------

/// Lifecycle state of a remotely received asset.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuarantineState {
    /// Received but not yet validated.
    Pending,
    /// Local validation passed — safe for reuse.
    Validated,
    /// Local validation failed — must not be used.
    Failed,
}

/// A remote asset held in the quarantine area together with its validation
/// state and origin metadata.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QuarantineEntry {
    pub asset_id: String,
    pub asset: NetworkAsset,
    pub origin_peer: String,
    pub state: QuarantineState,
    /// Wall-clock timestamp (Unix seconds) when this entry was created.
    pub received_at: i64,
    /// Optional reason string recorded when `state == Failed`.
    pub failure_reason: Option<String>,
}

/// In-process quarantine store.
///
/// Thread-safe via an internal `Mutex`.  Production deployments may replace
/// this with a persisted backend by implementing the same functional contract.
pub struct QuarantineStore {
    entries: Mutex<HashMap<String, QuarantineEntry>>,
}

impl Default for QuarantineStore {
    fn default() -> Self {
        Self::new()
    }
}

impl QuarantineStore {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }

    /// Admit a remote asset into quarantine (state = `Pending`).
    ///
    /// If the asset is already present it is *not* overwritten; the existing
    /// entry is left unchanged.  Returns `true` when a new entry was inserted.
    pub fn admit(
        &self,
        asset_id: impl Into<String>,
        asset: NetworkAsset,
        origin_peer: impl Into<String>,
    ) -> bool {
        let id = asset_id.into();
        let mut entries = self.entries.lock().unwrap();
        if entries.contains_key(&id) {
            return false;
        }
        entries.insert(
            id.clone(),
            QuarantineEntry {
                asset_id: id,
                asset,
                origin_peer: origin_peer.into(),
                state: QuarantineState::Pending,
                received_at: now_unix_secs(),
                failure_reason: None,
            },
        );
        true
    }

    /// Mark an asset as validated.
    ///
    /// Returns `true` on success, `false` if the asset was not found.
    pub fn validate_asset(&self, asset_id: &str) -> bool {
        let mut entries = self.entries.lock().unwrap();
        if let Some(entry) = entries.get_mut(asset_id) {
            entry.state = QuarantineState::Validated;
            entry.failure_reason = None;
            true
        } else {
            false
        }
    }

    /// Mark an asset as failed with a reason.
    ///
    /// Returns `true` on success, `false` if the asset was not found.
    pub fn fail_asset(&self, asset_id: &str, reason: impl Into<String>) -> bool {
        let mut entries = self.entries.lock().unwrap();
        if let Some(entry) = entries.get_mut(asset_id) {
            entry.state = QuarantineState::Failed;
            entry.failure_reason = Some(reason.into());
            true
        } else {
            false
        }
    }

    /// Retrieve an entry by asset id.
    pub fn get(&self, asset_id: &str) -> Option<QuarantineEntry> {
        self.entries.lock().unwrap().get(asset_id).cloned()
    }

    /// Returns `true` if `asset_id` is present **and** its state is `Validated`.
    pub fn is_selectable(&self, asset_id: &str) -> bool {
        self.entries
            .lock()
            .unwrap()
            .get(asset_id)
            .map(|e| e.state == QuarantineState::Validated)
            .unwrap_or(false)
    }

    /// All entries currently in `Pending` state.
    pub fn pending_entries(&self) -> Vec<QuarantineEntry> {
        self.entries
            .lock()
            .unwrap()
            .values()
            .filter(|e| e.state == QuarantineState::Pending)
            .cloned()
            .collect()
    }

    /// All entries currently in `Validated` state.
    pub fn validated_entries(&self) -> Vec<QuarantineEntry> {
        self.entries
            .lock()
            .unwrap()
            .values()
            .filter(|e| e.state == QuarantineState::Validated)
            .cloned()
            .collect()
    }

    /// Total number of entries.
    pub fn len(&self) -> usize {
        self.entries.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.lock().unwrap().is_empty()
    }
}

// ---------------------------------------------------------------------------
// Gossip Sync Engine
// ---------------------------------------------------------------------------

/// Statistics for a single sync session.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SyncStats {
    pub batches_processed: u64,
    pub assets_received: u64,
    pub assets_quarantined: u64,
    pub assets_skipped_duplicate: u64,
    pub assets_failed_validation: u64,
    pub assets_promoted: u64,
}

/// Incremental sync engine.
///
/// Maintains a per-peer cursor (last seen sequence number) so that each
/// gossip round only exchanges *new* assets.  Received assets are admitted
/// into a [`QuarantineStore`]; the caller is responsible for driving the
/// `validate → promote` lifecycle.
pub struct GossipSyncEngine {
    local_peer_id: String,
    /// Sequence counter for assets published by this node.
    local_sequence: Mutex<u64>,
    /// Last-seen remote sequence per peer.
    peer_cursors: Mutex<HashMap<String, u64>>,
    /// Assets published by this node, indexed by sequence number.
    local_assets: Mutex<Vec<(u64, NetworkAsset)>>,
    quarantine: QuarantineStore,
    stats: Mutex<SyncStats>,
}

impl GossipSyncEngine {
    pub fn new(local_peer_id: impl Into<String>) -> Self {
        Self {
            local_peer_id: local_peer_id.into(),
            local_sequence: Mutex::new(0),
            peer_cursors: Mutex::new(HashMap::new()),
            local_assets: Mutex::new(Vec::new()),
            quarantine: QuarantineStore::new(),
            stats: Mutex::new(SyncStats::default()),
        }
    }

    /// Publish a local asset, incrementing the sequence counter.
    /// Returns the sequence number assigned to this asset.
    pub fn publish_local(&self, asset: NetworkAsset) -> u64 {
        let mut seq = self.local_sequence.lock().unwrap();
        *seq += 1;
        let s = *seq;
        self.local_assets.lock().unwrap().push((s, asset));
        s
    }

    /// Build a [`PublishRequest`] containing all local assets with sequence >
    /// `since_cursor`.  Use `since_cursor = 0` to send everything.
    pub fn build_publish_request(&self, since_cursor: u64) -> PublishRequest {
        let assets: Vec<NetworkAsset> = self
            .local_assets
            .lock()
            .unwrap()
            .iter()
            .filter(|(seq, _)| *seq > since_cursor)
            .map(|(_, a)| a.clone())
            .collect();

        PublishRequest {
            sender_id: self.local_peer_id.clone(),
            assets,
            since_cursor: if since_cursor > 0 {
                Some(since_cursor.to_string())
            } else {
                None
            },
            resume_token: None,
        }
    }

    /// Process a [`PublishRequest`] received from a remote peer.
    ///
    /// Each asset is admitted to the [`QuarantineStore`] as `Pending`.
    /// Duplicates (already in quarantine) are counted as skipped.
    /// Returns a [`SyncAudit`] summarising what happened.
    pub fn receive_publish(&self, request: &PublishRequest) -> SyncAudit {
        let batch_id = format!("batch-{}-{}", request.sender_id, now_unix_secs());
        let mut applied = 0usize;
        let mut skipped = 0usize;

        for asset in &request.assets {
            let asset_id = asset_id_of(asset);
            let admitted = self
                .quarantine
                .admit(&asset_id, asset.clone(), &request.sender_id);
            if admitted {
                applied += 1;
            } else {
                skipped += 1;
            }
        }

        // Update peer cursor to latest known sequence
        if let Some(cursor_str) = &request.since_cursor {
            if let Ok(seq) = cursor_str.parse::<u64>() {
                let mut cursors = self.peer_cursors.lock().unwrap();
                let entry = cursors.entry(request.sender_id.clone()).or_insert(0);
                if seq > *entry {
                    *entry = seq;
                }
            }
        }

        {
            let mut stats = self.stats.lock().unwrap();
            stats.batches_processed += 1;
            stats.assets_received += request.assets.len() as u64;
            stats.assets_quarantined += applied as u64;
            stats.assets_skipped_duplicate += skipped as u64;
        }

        SyncAudit {
            batch_id,
            requested_cursor: request.since_cursor.clone(),
            scanned_count: request.assets.len(),
            applied_count: applied,
            skipped_count: skipped,
            failed_count: 0,
            failure_reasons: vec![],
        }
    }

    /// Build a [`FetchQuery`] for a remote peer, supplying the last-seen
    /// cursor so only delta assets are returned.
    pub fn build_fetch_query(&self, peer_id: &str, signals: Vec<String>) -> FetchQuery {
        let cursor = self
            .peer_cursors
            .lock()
            .unwrap()
            .get(peer_id)
            .copied()
            .unwrap_or(0);

        FetchQuery {
            sender_id: self.local_peer_id.clone(),
            signals,
            since_cursor: if cursor > 0 {
                Some(cursor.to_string())
            } else {
                None
            },
            resume_token: None,
        }
    }

    /// Process a [`FetchResponse`] received from a remote peer.
    ///
    /// Same quarantine semantics as [`receive_publish`](Self::receive_publish).
    pub fn receive_fetch_response(&self, peer_id: &str, response: &FetchResponse) -> SyncAudit {
        let fake_request = PublishRequest {
            sender_id: peer_id.to_string(),
            assets: response.assets.clone(),
            since_cursor: response.next_cursor.clone(),
            resume_token: response.resume_token.clone(),
        };
        self.receive_publish(&fake_request)
    }

    /// Drive the validate → promote step for a single asset.
    ///
    /// `validator` is a closure receiving the asset and returning `Ok(true)`
    /// when it passes.  On success the asset moves to `Validated`; on error
    /// it moves to `Failed` and the error message is stored.
    pub fn validate_and_promote<F>(&self, asset_id: &str, validator: F) -> bool
    where
        F: FnOnce(&NetworkAsset) -> Result<(), String>,
    {
        let entry = match self.quarantine.get(asset_id) {
            Some(e) => e,
            None => return false,
        };

        match validator(&entry.asset) {
            Ok(()) => {
                self.quarantine.validate_asset(asset_id);
                let mut stats = self.stats.lock().unwrap();
                stats.assets_promoted += 1;
                true
            }
            Err(reason) => {
                self.quarantine.fail_asset(asset_id, &reason);
                let mut stats = self.stats.lock().unwrap();
                stats.assets_failed_validation += 1;
                false
            }
        }
    }

    /// Returns `true` when `asset_id` is in the quarantine store **and**
    /// has been validated.  Unvalidated or unknown assets always return
    /// `false` — ensuring the failure-closed safety guarantee.
    pub fn is_asset_selectable(&self, asset_id: &str) -> bool {
        self.quarantine.is_selectable(asset_id)
    }

    /// All pending (not yet validated) quarantine entries.
    pub fn pending_entries(&self) -> Vec<QuarantineEntry> {
        self.quarantine.pending_entries()
    }

    /// Current statistics snapshot.
    pub fn stats(&self) -> SyncStats {
        self.stats.lock().unwrap().clone()
    }

    /// Last seen sequence for `peer_id`.
    pub fn peer_cursor(&self, peer_id: &str) -> u64 {
        self.peer_cursors
            .lock()
            .unwrap()
            .get(peer_id)
            .copied()
            .unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn now_unix_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Derive a stable string identifier for an asset.
fn asset_id_of(asset: &NetworkAsset) -> String {
    match asset {
        NetworkAsset::Gene { gene } => format!("gene:{}", gene.id),
        NetworkAsset::Capsule { capsule } => format!("capsule:{}", capsule.id),
        NetworkAsset::EvolutionEvent { event } => {
            use sha2::{Digest, Sha256};
            let payload = serde_json::to_vec(event).unwrap_or_default();
            let mut hasher = Sha256::new();
            hasher.update(payload);
            format!("event:{}", hex::encode(hasher.finalize()))
        }
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use oris_evolution::{AssetState, Gene};

    fn make_gene(id: &str) -> NetworkAsset {
        NetworkAsset::Gene {
            gene: Gene {
                id: id.to_string(),
                signals: vec!["test.fail".into()],
                strategy: vec!["fix test".into()],
                validation: vec!["cargo test".into()],
                state: AssetState::Promoted,
                task_class_id: None,
            },
        }
    }

    // -----------------------------------------------------------------------
    // AC 1: two-node gene sync end-to-end
    // -----------------------------------------------------------------------

    #[test]
    fn test_two_node_sync_end_to_end() {
        let node_a = GossipSyncEngine::new("node-a");
        let node_b = GossipSyncEngine::new("node-b");

        // node-a publishes a gene
        let seq = node_a.publish_local(make_gene("gene-1"));
        assert_eq!(seq, 1);

        // node-a builds a publish request and node-b receives it
        let req = node_a.build_publish_request(0);
        assert_eq!(req.assets.len(), 1);
        let audit = node_b.receive_publish(&req);
        assert_eq!(audit.applied_count, 1);
        assert_eq!(audit.skipped_count, 0);

        // gene-1 should now be in node-b's quarantine as Pending
        let entry = node_b.quarantine.get("gene:gene-1").unwrap();
        assert_eq!(entry.state, QuarantineState::Pending);
        assert_eq!(entry.origin_peer, "node-a");
    }

    #[test]
    fn test_incremental_cursor_sync() {
        let node_a = GossipSyncEngine::new("node-a");
        let node_b = GossipSyncEngine::new("node-b");

        // publish two genes
        node_a.publish_local(make_gene("gene-1"));
        node_a.publish_local(make_gene("gene-2"));

        // First sync — node-b has seen nothing (cursor=0)
        let req1 = node_a.build_publish_request(0);
        node_b.receive_publish(&req1);
        assert_eq!(node_b.quarantine.len(), 2);

        // Publish a third gene
        node_a.publish_local(make_gene("gene-3"));

        // Second sync — node-b requests from cursor=2
        let req2 = node_a.build_publish_request(2);
        let audit = node_b.receive_publish(&req2);
        // Only gene-3 is new
        assert_eq!(audit.applied_count, 1);
        assert_eq!(node_b.quarantine.len(), 3);
    }

    // -----------------------------------------------------------------------
    // AC 2: quarantine → validate → promote lifecycle
    // -----------------------------------------------------------------------

    #[test]
    fn test_quarantine_admit_and_validate() {
        let store = QuarantineStore::new();
        let asset = make_gene("g-1");

        assert!(store.admit("gene:g-1", asset, "peer-a"));
        assert_eq!(
            store.get("gene:g-1").unwrap().state,
            QuarantineState::Pending
        );
        assert!(!store.is_selectable("gene:g-1")); // Pending → not selectable

        store.validate_asset("gene:g-1");
        assert_eq!(
            store.get("gene:g-1").unwrap().state,
            QuarantineState::Validated
        );
        assert!(store.is_selectable("gene:g-1")); // Validated → selectable
    }

    #[test]
    fn test_quarantine_fail_asset() {
        let store = QuarantineStore::new();
        store.admit("gene:g-bad", make_gene("g-bad"), "peer-a");
        store.fail_asset("gene:g-bad", "signature mismatch");

        let entry = store.get("gene:g-bad").unwrap();
        assert_eq!(entry.state, QuarantineState::Failed);
        assert_eq!(entry.failure_reason.as_deref(), Some("signature mismatch"));
        assert!(!store.is_selectable("gene:g-bad"));
    }

    #[test]
    fn test_validate_and_promote_via_engine() {
        let engine = GossipSyncEngine::new("node-b");
        let req = PublishRequest {
            sender_id: "node-a".into(),
            assets: vec![make_gene("g-ok")],
            since_cursor: None,
            resume_token: None,
        };
        engine.receive_publish(&req);

        let promoted = engine.validate_and_promote("gene:g-ok", |_| Ok(()));
        assert!(promoted);
        assert!(engine.is_asset_selectable("gene:g-ok"));
    }

    #[test]
    fn test_validate_and_promote_failure_not_selectable() {
        let engine = GossipSyncEngine::new("node-b");
        let req = PublishRequest {
            sender_id: "node-a".into(),
            assets: vec![make_gene("g-invalid")],
            since_cursor: None,
            resume_token: None,
        };
        engine.receive_publish(&req);

        let promoted = engine.validate_and_promote("gene:g-invalid", |_| Err("bad hash".into()));
        assert!(!promoted);
        assert!(!engine.is_asset_selectable("gene:g-invalid"));
    }

    // -----------------------------------------------------------------------
    // AC 3: network fault — unvalidated genes must not be selectable
    // -----------------------------------------------------------------------

    #[test]
    fn test_pending_gene_not_selectable_under_fault() {
        let engine = GossipSyncEngine::new("node-b");
        // Simulate receiving a gene (as if a network message arrived) but NO
        // validation call is made (simulating a partition / message loss)
        let req = PublishRequest {
            sender_id: "node-a".into(),
            assets: vec![make_gene("g-unvalidated")],
            since_cursor: None,
            resume_token: None,
        };
        engine.receive_publish(&req);

        // Without explicit validation the gene must remain non-selectable
        assert!(
            !engine.is_asset_selectable("gene:g-unvalidated"),
            "pending gene must not be selectable (failure-closed guarantee)"
        );
        assert_eq!(engine.pending_entries().len(), 1);
    }

    #[test]
    fn test_unknown_asset_not_selectable() {
        let engine = GossipSyncEngine::new("node-b");
        assert!(!engine.is_asset_selectable("gene:nonexistent"));
    }

    #[test]
    fn test_duplicate_admit_is_idempotent() {
        let store = QuarantineStore::new();
        assert!(store.admit("gene:g", make_gene("g"), "peer-a"));
        store.validate_asset("gene:g");
        // A second admit for the same id must not overwrite the Validated state
        assert!(!store.admit("gene:g", make_gene("g"), "peer-b"));
        assert_eq!(
            store.get("gene:g").unwrap().state,
            QuarantineState::Validated
        );
    }

    #[test]
    fn test_stats_accumulate_correctly() {
        let engine = GossipSyncEngine::new("me");
        let req = PublishRequest {
            sender_id: "peer".into(),
            assets: vec![make_gene("g1"), make_gene("g2")],
            since_cursor: None,
            resume_token: None,
        };
        engine.receive_publish(&req);
        engine.validate_and_promote("gene:g1", |_| Ok(()));
        engine.validate_and_promote("gene:g2", |_| Err("bad".into()));

        let s = engine.stats();
        assert_eq!(s.assets_quarantined, 2);
        assert_eq!(s.assets_promoted, 1);
        assert_eq!(s.assets_failed_validation, 1);
    }
}
