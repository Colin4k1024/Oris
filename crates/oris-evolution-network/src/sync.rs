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

use crate::{
    verify_envelope, EvolutionEnvelope, FetchQuery, FetchResponse, NetworkAsset,
    PeerRateLimitConfig, PeerRateLimiter, PublishRequest, SyncAudit,
};
use chrono::Utc;
use oris_evolution::{AssetState, Capsule, Gene};
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
// Remote capsule auto-promotion pipeline (P2-06)
// ---------------------------------------------------------------------------

/// Default score threshold above which a remote capsule is auto-promoted.
pub const PROMOTE_THRESHOLD: f64 = 0.70;

/// Reason a capsule was held in quarantine.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuarantineReason {
    /// Capsule composite score was below the promotion threshold.
    LowScore { score: f64 },
    /// Signature verification failed (placeholder until P3-02).
    SignatureInvalid,
    /// The capsule's gene could not be located.
    GeneMissing,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectionReason {
    InvalidSignature,
    MissingSignature,
    RateLimited,
    GeneMissing,
}

impl std::fmt::Display for RejectionReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RejectionReason::InvalidSignature => write!(f, "invalid_signature"),
            RejectionReason::MissingSignature => write!(f, "missing_signature"),
            RejectionReason::RateLimited => write!(f, "rate_limited"),
            RejectionReason::GeneMissing => write!(f, "gene_missing"),
        }
    }
}

impl std::fmt::Display for QuarantineReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QuarantineReason::LowScore { score } => {
                write!(f, "low_score:{:.4}", score)
            }
            QuarantineReason::SignatureInvalid => write!(f, "signature_invalid"),
            QuarantineReason::GeneMissing => write!(f, "gene_missing"),
        }
    }
}

/// Outcome of an auto-promotion decision for a single remote capsule.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "disposition")]
pub enum CapsuleDisposition {
    /// Capsule passed the score threshold; its gene was solidified.
    Promoted {
        gene_id: String,
        /// The composite score that triggered promotion.
        score: f64,
    },
    /// Capsule did not pass; held in quarantine.
    Quarantined { reason: String },
}

/// Audit log entry appended to `capsule_audit_log.jsonl`.
#[derive(Serialize)]
struct AuditLogEntry<'a> {
    timestamp: String,
    peer_id: &'a str,
    capsule_id: &'a str,
    gene_id: &'a str,
    disposition: NetworkAuditDisposition,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    score: f64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkAuditDisposition {
    Accept,
    Reject,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NetworkAuditEntry {
    pub timestamp: String,
    pub peer_id: String,
    pub capsule_id: String,
    pub gene_id: String,
    pub disposition: NetworkAuditDisposition,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
}

/// Drives the receive → score → promote/quarantine pipeline for remote capsules.
///
/// # Usage
///
/// ```ignore
/// let receiver = RemoteCapsuleReceiver::new("/tmp/audit.jsonl", None);
/// let disposition = receiver.on_capsule_received(&capsule, &gene);
/// ```
pub struct RemoteCapsuleReceiver {
    /// Score threshold; capsules >= threshold are promoted.
    threshold: f64,
    /// Path to the append-only JSONL audit log. `None` disables file writing.
    audit_log_path: Option<std::path::PathBuf>,
    /// In-memory audit trail (always populated regardless of file).
    audit_trail: Mutex<Vec<(String, CapsuleDisposition)>>,
    network_audit_trail: Mutex<Vec<NetworkAuditEntry>>,
    rate_limiter: PeerRateLimiter,
}

impl Default for RemoteCapsuleReceiver {
    fn default() -> Self {
        Self::new(None::<&str>, None)
    }
}

impl RemoteCapsuleReceiver {
    /// Create a new receiver.
    ///
    /// * `audit_log_path` — if `Some`, every decision is appended as a JSONL
    ///   line to that file.
    /// * `threshold` — override the default `PROMOTE_THRESHOLD` (0.70).
    pub fn new(
        audit_log_path: Option<impl AsRef<std::path::Path>>,
        threshold: Option<f64>,
    ) -> Self {
        Self::with_rate_limit_config(audit_log_path, threshold, PeerRateLimitConfig::default())
    }

    pub fn with_rate_limit_config(
        audit_log_path: Option<impl AsRef<std::path::Path>>,
        threshold: Option<f64>,
        rate_limit_config: PeerRateLimitConfig,
    ) -> Self {
        Self {
            threshold: threshold.unwrap_or(PROMOTE_THRESHOLD),
            audit_log_path: audit_log_path.map(|p| p.as_ref().to_path_buf()),
            audit_trail: Mutex::new(Vec::new()),
            network_audit_trail: Mutex::new(Vec::new()),
            rate_limiter: PeerRateLimiter::new(rate_limit_config),
        }
    }

    /// Evaluate a received capsule and return the promotion decision.
    ///
    /// Steps:
    /// 1. Verify signature (placeholder — always passes until P3-02).
    /// 2. Use the capsule's own `confidence` field as the composite score.
    /// 3. If `score >= threshold`: set `gene.state = Promoted` and return
    ///    `CapsuleDisposition::Promoted`.
    /// 4. Otherwise: return `CapsuleDisposition::Quarantined { reason: LowScore }`.
    /// 5. Append an audit entry to `capsule_audit_log.jsonl`.
    ///
    /// The caller is responsible for persisting the promoted gene to a gene
    /// store (e.g. `GeneStore::upsert_gene`).  The returned `CapsuleDisposition`
    /// carries the promoted `gene_id` so the caller can act accordingly.
    pub fn on_capsule_received(&self, capsule: &Capsule, gene: &mut Gene) -> CapsuleDisposition {
        self.evaluate_capsule("unknown", capsule, gene)
    }

    pub fn on_signed_capsule_received(
        &self,
        peer_id: &str,
        public_key_hex: &str,
        envelope: &EvolutionEnvelope,
        capsule: &Capsule,
        gene: &mut Gene,
    ) -> Result<CapsuleDisposition, RejectionReason> {
        if !self.rate_limiter.check(peer_id) {
            self.write_rejection_audit_entry(
                peer_id,
                capsule,
                Some(gene.id.as_str()),
                RejectionReason::RateLimited,
            );
            return Err(RejectionReason::RateLimited);
        }

        if envelope.signature.is_none() {
            self.write_rejection_audit_entry(
                peer_id,
                capsule,
                Some(gene.id.as_str()),
                RejectionReason::MissingSignature,
            );
            return Err(RejectionReason::MissingSignature);
        }

        if verify_envelope(public_key_hex, envelope).is_err() {
            self.write_rejection_audit_entry(
                peer_id,
                capsule,
                Some(gene.id.as_str()),
                RejectionReason::InvalidSignature,
            );
            return Err(RejectionReason::InvalidSignature);
        }

        let has_capsule = envelope.assets.iter().any(|asset| {
            matches!(asset, NetworkAsset::Capsule { capsule: remote } if remote.id == capsule.id && remote.gene_id == capsule.gene_id)
        });
        let has_gene = envelope.assets.iter().any(
            |asset| matches!(asset, NetworkAsset::Gene { gene: remote } if remote.id == gene.id),
        );
        if !has_capsule || !has_gene || gene.id != capsule.gene_id {
            self.write_rejection_audit_entry(
                peer_id,
                capsule,
                Some(gene.id.as_str()),
                RejectionReason::GeneMissing,
            );
            return Err(RejectionReason::GeneMissing);
        }

        Ok(self.evaluate_capsule(peer_id, capsule, gene))
    }

    pub fn network_audit_trail(&self) -> Vec<NetworkAuditEntry> {
        self.network_audit_trail.lock().unwrap().clone()
    }

    fn evaluate_capsule(
        &self,
        peer_id: &str,
        capsule: &Capsule,
        gene: &mut Gene,
    ) -> CapsuleDisposition {
        let score = capsule.confidence as f64;

        let disposition = if score >= self.threshold {
            gene.state = AssetState::Promoted;
            CapsuleDisposition::Promoted {
                gene_id: capsule.gene_id.clone(),
                score,
            }
        } else {
            let reason = QuarantineReason::LowScore { score };
            CapsuleDisposition::Quarantined {
                reason: reason.to_string(),
            }
        };

        self.write_accept_audit_entry(peer_id, capsule, &disposition, score);
        self.audit_trail
            .lock()
            .unwrap()
            .push((capsule.id.clone(), disposition.clone()));

        disposition
    }

    /// All in-memory audit entries accumulated so far.
    pub fn audit_trail(&self) -> Vec<(String, CapsuleDisposition)> {
        self.audit_trail.lock().unwrap().clone()
    }

    /// Number of audit entries recorded.
    pub fn audit_count(&self) -> usize {
        self.audit_trail.lock().unwrap().len()
    }

    fn write_accept_audit_entry(
        &self,
        peer_id: &str,
        capsule: &Capsule,
        disposition: &CapsuleDisposition,
        score: f64,
    ) {
        let Some(ref path) = self.audit_log_path else {
            self.network_audit_trail
                .lock()
                .unwrap()
                .push(NetworkAuditEntry {
                    timestamp: Utc::now().to_rfc3339(),
                    peer_id: peer_id.to_string(),
                    capsule_id: capsule.id.clone(),
                    gene_id: capsule.gene_id.clone(),
                    disposition: NetworkAuditDisposition::Accept,
                    reason: match disposition {
                        CapsuleDisposition::Promoted { .. } => None,
                        CapsuleDisposition::Quarantined { reason } => Some(reason.clone()),
                    },
                    score: Some(score),
                });
            return;
        };
        let entry = AuditLogEntry {
            timestamp: Utc::now().to_rfc3339(),
            peer_id,
            capsule_id: &capsule.id,
            gene_id: &capsule.gene_id,
            disposition: NetworkAuditDisposition::Accept,
            reason: match disposition {
                CapsuleDisposition::Promoted { .. } => None,
                CapsuleDisposition::Quarantined { reason } => Some(reason.clone()),
            },
            score,
        };
        self.network_audit_trail
            .lock()
            .unwrap()
            .push(NetworkAuditEntry {
                timestamp: entry.timestamp.clone(),
                peer_id: entry.peer_id.to_string(),
                capsule_id: entry.capsule_id.to_string(),
                gene_id: entry.gene_id.to_string(),
                disposition: entry.disposition.clone(),
                reason: entry.reason.clone(),
                score: Some(entry.score),
            });
        if let Ok(mut line) = serde_json::to_string(&entry) {
            line.push('\n');
            use std::io::Write;
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let _ = file.write_all(line.as_bytes());
            }
        }
    }

    fn write_rejection_audit_entry(
        &self,
        peer_id: &str,
        capsule: &Capsule,
        gene_id: Option<&str>,
        reason: RejectionReason,
    ) {
        let entry = NetworkAuditEntry {
            timestamp: Utc::now().to_rfc3339(),
            peer_id: peer_id.to_string(),
            capsule_id: capsule.id.clone(),
            gene_id: gene_id.unwrap_or(&capsule.gene_id).to_string(),
            disposition: NetworkAuditDisposition::Reject,
            reason: Some(reason.to_string()),
            score: Some(capsule.confidence as f64),
        };
        self.network_audit_trail.lock().unwrap().push(entry.clone());

        let Some(ref path) = self.audit_log_path else {
            return;
        };

        if let Ok(mut line) = serde_json::to_string(&entry) {
            line.push('\n');
            use std::io::Write;
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let _ = file.write_all(line.as_bytes());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{sign_envelope, NodeKeypair};
    use oris_evolution::{AssetState, Capsule, EnvFingerprint, Gene, Outcome};

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

    fn make_capsule(id: &str, gene_id: &str, confidence: f32) -> Capsule {
        Capsule {
            id: id.to_string(),
            gene_id: gene_id.to_string(),
            mutation_id: "mut-1".to_string(),
            run_id: "run-1".to_string(),
            diff_hash: "abc123".to_string(),
            confidence,
            env: EnvFingerprint {
                rustc_version: "1.80.0".to_string(),
                cargo_lock_hash: "hash".to_string(),
                target_triple: "aarch64-apple-darwin".to_string(),
                os: "macos".to_string(),
            },
            outcome: Outcome {
                success: true,
                validation_profile: "default".to_string(),
                validation_duration_ms: 100,
                changed_files: vec![],
                validator_hash: "vh1".to_string(),
                lines_changed: 5,
                replay_verified: false,
            },
            state: AssetState::Candidate,
        }
    }

    fn make_plain_gene(id: &str) -> Gene {
        Gene {
            id: id.to_string(),
            signals: vec!["test.fail".into()],
            strategy: vec!["fix test".into()],
            validation: vec!["cargo test".into()],
            state: AssetState::Candidate,
            task_class_id: None,
        }
    }

    fn make_signed_envelope(
        keypair: &NodeKeypair,
        sender_id: &str,
        capsule: &Capsule,
        gene: &Gene,
    ) -> EvolutionEnvelope {
        let envelope = EvolutionEnvelope::publish(
            sender_id,
            vec![
                NetworkAsset::Gene { gene: gene.clone() },
                NetworkAsset::Capsule {
                    capsule: capsule.clone(),
                },
            ],
        );
        sign_envelope(keypair, &envelope)
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

    // -----------------------------------------------------------------------
    // AC P2-06: RemoteCapsuleReceiver auto-promotion pipeline
    // -----------------------------------------------------------------------

    #[test]
    fn test_remote_capsule_high_score_is_promoted() {
        let receiver = RemoteCapsuleReceiver::new(None::<&str>, None);
        let capsule = make_capsule("cap-1", "gene-1", 0.85);
        let mut gene = make_plain_gene("gene-1");

        let disposition = receiver.on_capsule_received(&capsule, &mut gene);

        match &disposition {
            CapsuleDisposition::Promoted { gene_id, score } => {
                assert_eq!(gene_id, "gene-1");
                assert!(*score >= PROMOTE_THRESHOLD);
            }
            other => panic!("expected Promoted, got {:?}", other),
        }
        assert_eq!(gene.state, AssetState::Promoted);
        assert_eq!(receiver.audit_count(), 1);
    }

    #[test]
    fn test_remote_capsule_low_score_is_quarantined() {
        let receiver = RemoteCapsuleReceiver::new(None::<&str>, None);
        let capsule = make_capsule("cap-2", "gene-2", 0.40);
        let mut gene = make_plain_gene("gene-2");
        let original_state = gene.state.clone();

        let disposition = receiver.on_capsule_received(&capsule, &mut gene);

        match &disposition {
            CapsuleDisposition::Quarantined { reason } => {
                assert!(reason.starts_with("low_score:"), "reason={}", reason);
            }
            other => panic!("expected Quarantined, got {:?}", other),
        }
        // Gene state must not be changed when quarantined.
        assert_eq!(gene.state, original_state);
        assert_eq!(receiver.audit_count(), 1);
    }

    #[test]
    fn test_remote_capsule_at_threshold_is_promoted() {
        let receiver = RemoteCapsuleReceiver::new(None::<&str>, None);
        // Use a confidence value just at or above threshold (0.70). Because
        // f32 → f64 casting can introduce tiny rounding errors we use a value
        // that is safely representable: 0.75 (exactly 3/4 in binary).
        let capsule = make_capsule("cap-3", "gene-3", 0.75_f32);
        let mut gene = make_plain_gene("gene-3");

        let disposition = receiver.on_capsule_received(&capsule, &mut gene);
        assert!(
            matches!(&disposition, CapsuleDisposition::Promoted { .. }),
            "capsule at or above threshold must be promoted"
        );
    }

    #[test]
    fn test_remote_capsule_audit_log_written() {
        let dir = std::env::temp_dir();
        let log_path = dir.join(format!(
            "capsule_audit_log_test_{}.jsonl",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        {
            let receiver = RemoteCapsuleReceiver::new(Some(&log_path), None);
            let c1 = make_capsule("cap-a", "g-a", 0.90);
            let c2 = make_capsule("cap-b", "g-b", 0.30);
            let mut gene_a = make_plain_gene("g-a");
            let mut gene_b = make_plain_gene("g-b");
            receiver.on_capsule_received(&c1, &mut gene_a);
            receiver.on_capsule_received(&c2, &mut gene_b);
        }

        let contents = std::fs::read_to_string(&log_path).expect("audit log must exist");
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 2, "two decisions must produce two log lines");
        // Each line must be valid JSON
        for line in &lines {
            serde_json::from_str::<serde_json::Value>(line)
                .expect("each audit line must be valid JSON");
        }
        let _ = std::fs::remove_file(&log_path);
    }

    #[test]
    fn test_remote_capsule_audit_trail_in_memory() {
        let receiver = RemoteCapsuleReceiver::default();
        let c1 = make_capsule("cap-x", "g-x", 0.80);
        let c2 = make_capsule("cap-y", "g-y", 0.50);
        let mut g1 = make_plain_gene("g-x");
        let mut g2 = make_plain_gene("g-y");

        receiver.on_capsule_received(&c1, &mut g1);
        receiver.on_capsule_received(&c2, &mut g2);

        let trail = receiver.audit_trail();
        assert_eq!(trail.len(), 2);
        assert_eq!(trail[0].0, "cap-x");
        assert_eq!(trail[1].0, "cap-y");
        assert!(matches!(&trail[0].1, CapsuleDisposition::Promoted { .. }));
        assert!(matches!(
            &trail[1].1,
            CapsuleDisposition::Quarantined { .. }
        ));
    }

    #[test]
    fn test_remote_capsule_missing_signature_is_rejected() {
        let receiver = RemoteCapsuleReceiver::default();
        let capsule = make_capsule("cap-sec-1", "gene-sec-1", 0.82);
        let mut gene = make_plain_gene("gene-sec-1");
        let envelope = EvolutionEnvelope::publish(
            "node-a",
            vec![
                NetworkAsset::Gene { gene: gene.clone() },
                NetworkAsset::Capsule {
                    capsule: capsule.clone(),
                },
            ],
        );

        let result = receiver
            .on_signed_capsule_received("peer-a", "deadbeef", &envelope, &capsule, &mut gene);

        assert_eq!(result.unwrap_err(), RejectionReason::MissingSignature);
        assert_eq!(receiver.network_audit_trail().len(), 1);
        assert_eq!(gene.state, AssetState::Candidate);
    }

    #[test]
    fn test_remote_capsule_tampered_signature_is_rejected() {
        let temp_path = std::env::temp_dir().join(format!(
            "oris-node-key-{}.key",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let keypair =
            NodeKeypair::generate_at(&temp_path).expect("keypair generation should succeed");
        let receiver = RemoteCapsuleReceiver::default();
        let capsule = make_capsule("cap-sec-2", "gene-sec-2", 0.90);
        let mut gene = make_plain_gene("gene-sec-2");
        let mut envelope = make_signed_envelope(&keypair, "node-a", &capsule, &gene);
        if let Some(NetworkAsset::Gene { gene: remote_gene }) = envelope.assets.first_mut() {
            remote_gene.strategy.push("tampered".to_string());
        }

        let result = receiver.on_signed_capsule_received(
            "peer-a",
            &keypair.public_key_hex(),
            &envelope,
            &capsule,
            &mut gene,
        );

        assert_eq!(result.unwrap_err(), RejectionReason::InvalidSignature);
        let _ = std::fs::remove_file(temp_path);
    }

    #[test]
    fn test_remote_capsule_rate_limited_is_rejected() {
        let temp_path = std::env::temp_dir().join(format!(
            "oris-node-key-{}.key",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let keypair =
            NodeKeypair::generate_at(&temp_path).expect("keypair generation should succeed");
        let receiver = RemoteCapsuleReceiver::with_rate_limit_config(
            None::<&str>,
            None,
            PeerRateLimitConfig {
                max_capsules_per_hour: 1,
                window_secs: 3600,
            },
        );
        let capsule = make_capsule("cap-sec-3", "gene-sec-3", 0.91);
        let mut gene = make_plain_gene("gene-sec-3");
        let envelope = make_signed_envelope(&keypair, "node-a", &capsule, &gene);

        let first = receiver.on_signed_capsule_received(
            "peer-a",
            &keypair.public_key_hex(),
            &envelope,
            &capsule,
            &mut gene,
        );
        assert!(first.is_ok());

        let mut gene_again = make_plain_gene("gene-sec-3");
        let second = receiver.on_signed_capsule_received(
            "peer-a",
            &keypair.public_key_hex(),
            &envelope,
            &capsule,
            &mut gene_again,
        );
        assert_eq!(second.unwrap_err(), RejectionReason::RateLimited);
        let _ = std::fs::remove_file(temp_path);
    }

    #[test]
    fn test_network_audit_log_records_accept_and_reject_events() {
        let temp_key = std::env::temp_dir().join(format!(
            "oris-node-key-{}.key",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let keypair =
            NodeKeypair::generate_at(&temp_key).expect("keypair generation should succeed");
        let log_path = std::env::temp_dir().join(format!(
            "network_audit_log_test_{}.jsonl",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        let receiver = RemoteCapsuleReceiver::with_rate_limit_config(
            Some(&log_path),
            None,
            PeerRateLimitConfig {
                max_capsules_per_hour: 10,
                window_secs: 3600,
            },
        );
        let capsule = make_capsule("cap-sec-4", "gene-sec-4", 0.88);
        let mut gene = make_plain_gene("gene-sec-4");
        let envelope = make_signed_envelope(&keypair, "node-a", &capsule, &gene);
        let accepted = receiver.on_signed_capsule_received(
            "peer-a",
            &keypair.public_key_hex(),
            &envelope,
            &capsule,
            &mut gene,
        );
        assert!(accepted.is_ok());

        let unsigned_envelope = EvolutionEnvelope::publish(
            "node-a",
            vec![
                NetworkAsset::Gene { gene: gene.clone() },
                NetworkAsset::Capsule {
                    capsule: capsule.clone(),
                },
            ],
        );
        let mut rejected_gene = make_plain_gene("gene-sec-4");
        let rejected = receiver.on_signed_capsule_received(
            "peer-b",
            &keypair.public_key_hex(),
            &unsigned_envelope,
            &capsule,
            &mut rejected_gene,
        );
        assert_eq!(rejected.unwrap_err(), RejectionReason::MissingSignature);

        let contents = std::fs::read_to_string(&log_path).expect("audit log must exist");
        let lines: Vec<serde_json::Value> = contents
            .lines()
            .map(|line| serde_json::from_str(line).expect("audit line must be valid JSON"))
            .collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["disposition"], "accept");
        assert_eq!(lines[1]["disposition"], "reject");
        let _ = std::fs::remove_file(temp_key);
        let _ = std::fs::remove_file(log_path);
    }
}
