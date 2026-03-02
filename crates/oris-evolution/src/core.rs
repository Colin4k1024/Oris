//! Evolution domain model, append-only event store, projections, and selector logic.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::f64::consts::E;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Utc};
use oris_kernel::RunId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub type MutationId = String;
pub type GeneId = String;
pub type CapsuleId = String;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum AssetState {
    Candidate,
    #[default]
    Promoted,
    Revoked,
    Archived,
    Quarantined,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum CandidateSource {
    #[default]
    Local,
    Remote,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlastRadius {
    pub files_changed: usize,
    pub lines_changed: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ArtifactEncoding {
    UnifiedDiff,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum MutationTarget {
    WorkspaceRoot,
    Crate { name: String },
    Paths { allow: Vec<String> },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MutationIntent {
    pub id: MutationId,
    pub intent: String,
    pub target: MutationTarget,
    pub expected_effect: String,
    pub risk: RiskLevel,
    pub signals: Vec<String>,
    #[serde(default)]
    pub spec_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MutationArtifact {
    pub encoding: ArtifactEncoding,
    pub payload: String,
    pub base_revision: Option<String>,
    pub content_hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PreparedMutation {
    pub intent: MutationIntent,
    pub artifact: MutationArtifact,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ValidationSnapshot {
    pub success: bool,
    pub profile: String,
    pub duration_ms: u64,
    pub summary: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Outcome {
    pub success: bool,
    pub validation_profile: String,
    pub validation_duration_ms: u64,
    pub changed_files: Vec<String>,
    pub validator_hash: String,
    #[serde(default)]
    pub lines_changed: usize,
    #[serde(default)]
    pub replay_verified: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnvFingerprint {
    pub rustc_version: String,
    pub cargo_lock_hash: String,
    pub target_triple: String,
    pub os: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Capsule {
    pub id: CapsuleId,
    pub gene_id: GeneId,
    pub mutation_id: MutationId,
    pub run_id: RunId,
    pub diff_hash: String,
    pub confidence: f32,
    pub env: EnvFingerprint,
    pub outcome: Outcome,
    #[serde(default)]
    pub state: AssetState,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Gene {
    pub id: GeneId,
    pub signals: Vec<String>,
    pub strategy: Vec<String>,
    pub validation: Vec<String>,
    #[serde(default)]
    pub state: AssetState,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EvolutionEvent {
    MutationDeclared {
        mutation: PreparedMutation,
    },
    MutationApplied {
        mutation_id: MutationId,
        patch_hash: String,
        changed_files: Vec<String>,
    },
    MutationRejected {
        mutation_id: MutationId,
        reason: String,
    },
    ValidationPassed {
        mutation_id: MutationId,
        report: ValidationSnapshot,
        gene_id: Option<GeneId>,
    },
    ValidationFailed {
        mutation_id: MutationId,
        report: ValidationSnapshot,
        gene_id: Option<GeneId>,
    },
    CapsuleCommitted {
        capsule: Capsule,
    },
    CapsuleQuarantined {
        capsule_id: CapsuleId,
    },
    CapsuleReleased {
        capsule_id: CapsuleId,
        state: AssetState,
    },
    CapsuleReused {
        capsule_id: CapsuleId,
        gene_id: GeneId,
        run_id: RunId,
    },
    GeneProjected {
        gene: Gene,
    },
    GenePromoted {
        gene_id: GeneId,
    },
    GeneRevoked {
        gene_id: GeneId,
        reason: String,
    },
    GeneArchived {
        gene_id: GeneId,
    },
    PromotionEvaluated {
        gene_id: GeneId,
        state: AssetState,
        reason: String,
    },
    RemoteAssetImported {
        source: CandidateSource,
        asset_ids: Vec<String>,
    },
    SpecLinked {
        mutation_id: MutationId,
        spec_id: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoredEvolutionEvent {
    pub seq: u64,
    pub timestamp: String,
    pub prev_hash: String,
    pub record_hash: String,
    pub event: EvolutionEvent,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EvolutionProjection {
    pub genes: Vec<Gene>,
    pub capsules: Vec<Capsule>,
    pub reuse_counts: BTreeMap<GeneId, u64>,
    pub attempt_counts: BTreeMap<GeneId, u64>,
    pub last_updated_at: BTreeMap<GeneId, String>,
}

#[derive(Clone, Debug)]
pub struct SelectorInput {
    pub signals: Vec<String>,
    pub env: EnvFingerprint,
    pub limit: usize,
}

#[derive(Clone, Debug)]
pub struct GeneCandidate {
    pub gene: Gene,
    pub score: f32,
    pub capsules: Vec<Capsule>,
}

pub trait Selector: Send + Sync {
    fn select(&self, input: &SelectorInput) -> Vec<GeneCandidate>;
}

pub trait EvolutionStore: Send + Sync {
    fn append_event(&self, event: EvolutionEvent) -> Result<u64, EvolutionError>;
    fn scan(&self, from_seq: u64) -> Result<Vec<StoredEvolutionEvent>, EvolutionError>;
    fn rebuild_projection(&self) -> Result<EvolutionProjection, EvolutionError>;
}

#[derive(Debug, Error)]
pub enum EvolutionError {
    #[error("I/O error: {0}")]
    Io(String),
    #[error("Serialization error: {0}")]
    Serde(String),
    #[error("Hash chain validation failed: {0}")]
    HashChain(String),
}

pub struct JsonlEvolutionStore {
    root_dir: PathBuf,
    lock: Mutex<()>,
}

impl JsonlEvolutionStore {
    pub fn new<P: Into<PathBuf>>(root_dir: P) -> Self {
        Self {
            root_dir: root_dir.into(),
            lock: Mutex::new(()),
        }
    }

    pub fn root_dir(&self) -> &Path {
        &self.root_dir
    }

    fn ensure_layout(&self) -> Result<(), EvolutionError> {
        fs::create_dir_all(&self.root_dir).map_err(io_err)?;
        let lock_path = self.root_dir.join("LOCK");
        if !lock_path.exists() {
            File::create(lock_path).map_err(io_err)?;
        }
        let events_path = self.events_path();
        if !events_path.exists() {
            File::create(events_path).map_err(io_err)?;
        }
        Ok(())
    }

    fn events_path(&self) -> PathBuf {
        self.root_dir.join("events.jsonl")
    }

    fn genes_path(&self) -> PathBuf {
        self.root_dir.join("genes.json")
    }

    fn capsules_path(&self) -> PathBuf {
        self.root_dir.join("capsules.json")
    }

    fn read_all_events(&self) -> Result<Vec<StoredEvolutionEvent>, EvolutionError> {
        self.ensure_layout()?;
        let file = File::open(self.events_path()).map_err(io_err)?;
        let reader = BufReader::new(file);
        let mut events = Vec::new();
        for line in reader.lines() {
            let line = line.map_err(io_err)?;
            if line.trim().is_empty() {
                continue;
            }
            let event = serde_json::from_str::<StoredEvolutionEvent>(&line)
                .map_err(|err| EvolutionError::Serde(err.to_string()))?;
            events.push(event);
        }
        verify_hash_chain(&events)?;
        Ok(events)
    }

    fn write_projection_files(
        &self,
        projection: &EvolutionProjection,
    ) -> Result<(), EvolutionError> {
        write_json_atomic(&self.genes_path(), &projection.genes)?;
        write_json_atomic(&self.capsules_path(), &projection.capsules)?;
        Ok(())
    }

    fn append_event_locked(&self, event: EvolutionEvent) -> Result<u64, EvolutionError> {
        let existing = self.read_all_events()?;
        let next_seq = existing.last().map(|entry| entry.seq + 1).unwrap_or(1);
        let prev_hash = existing
            .last()
            .map(|entry| entry.record_hash.clone())
            .unwrap_or_default();
        let timestamp = Utc::now().to_rfc3339();
        let record_hash = hash_record(next_seq, &timestamp, &prev_hash, &event)?;
        let stored = StoredEvolutionEvent {
            seq: next_seq,
            timestamp,
            prev_hash,
            record_hash,
            event,
        };
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.events_path())
            .map_err(io_err)?;
        let line =
            serde_json::to_string(&stored).map_err(|err| EvolutionError::Serde(err.to_string()))?;
        file.write_all(line.as_bytes()).map_err(io_err)?;
        file.write_all(b"\n").map_err(io_err)?;
        file.sync_data().map_err(io_err)?;

        let events = self.read_all_events()?;
        let projection = rebuild_projection_from_events(&events);
        self.write_projection_files(&projection)?;
        Ok(next_seq)
    }
}

impl EvolutionStore for JsonlEvolutionStore {
    fn append_event(&self, event: EvolutionEvent) -> Result<u64, EvolutionError> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| EvolutionError::Io("evolution store lock poisoned".into()))?;
        self.append_event_locked(event)
    }

    fn scan(&self, from_seq: u64) -> Result<Vec<StoredEvolutionEvent>, EvolutionError> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| EvolutionError::Io("evolution store lock poisoned".into()))?;
        Ok(self
            .read_all_events()?
            .into_iter()
            .filter(|entry| entry.seq >= from_seq)
            .collect())
    }

    fn rebuild_projection(&self) -> Result<EvolutionProjection, EvolutionError> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| EvolutionError::Io("evolution store lock poisoned".into()))?;
        let projection = rebuild_projection_from_events(&self.read_all_events()?);
        self.write_projection_files(&projection)?;
        Ok(projection)
    }
}

pub struct ProjectionSelector {
    projection: EvolutionProjection,
    now: DateTime<Utc>,
}

impl ProjectionSelector {
    pub fn new(projection: EvolutionProjection) -> Self {
        Self {
            projection,
            now: Utc::now(),
        }
    }

    pub fn with_now(projection: EvolutionProjection, now: DateTime<Utc>) -> Self {
        Self { projection, now }
    }
}

impl Selector for ProjectionSelector {
    fn select(&self, input: &SelectorInput) -> Vec<GeneCandidate> {
        let mut out = Vec::new();
        for gene in &self.projection.genes {
            if gene.state != AssetState::Promoted {
                continue;
            }
            let capsules = self
                .projection
                .capsules
                .iter()
                .filter(|capsule| {
                    capsule.gene_id == gene.id && capsule.state == AssetState::Promoted
                })
                .cloned()
                .collect::<Vec<_>>();
            if capsules.is_empty() {
                continue;
            }

            let successful_capsules = capsules.len() as f64;
            let attempts = self
                .projection
                .attempt_counts
                .get(&gene.id)
                .copied()
                .unwrap_or(capsules.len() as u64) as f64;
            let success_rate = if attempts == 0.0 {
                0.0
            } else {
                successful_capsules / attempts
            };
            let successful_reuses = self
                .projection
                .reuse_counts
                .get(&gene.id)
                .copied()
                .unwrap_or(0) as f64;
            let reuse_count_factor = 1.0 + (1.0 + successful_reuses).ln();
            let env_fingerprints = capsules
                .iter()
                .map(|capsule| fingerprint_key(&capsule.env))
                .collect::<BTreeSet<_>>()
                .len() as f64;
            let env_diversity = (env_fingerprints / 5.0).min(1.0);
            let signal_overlap = normalized_signal_overlap(&gene.signals, &input.signals);
            let recency_decay = self
                .projection
                .last_updated_at
                .get(&gene.id)
                .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
                .map(|dt| {
                    let age_days = (self.now - dt.with_timezone(&Utc)).num_days().max(0) as f64;
                    E.powf(-age_days / 30.0)
                })
                .unwrap_or(0.0);
            let score = (success_rate
                * reuse_count_factor
                * env_diversity
                * recency_decay
                * signal_overlap) as f32;
            if score < 0.35 {
                continue;
            }
            out.push(GeneCandidate {
                gene: gene.clone(),
                score,
                capsules,
            });
        }

        out.sort_by(|left, right| {
            right
                .score
                .partial_cmp(&left.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left.gene.id.cmp(&right.gene.id))
        });
        out.truncate(input.limit.max(1));
        out
    }
}

pub struct StoreBackedSelector {
    store: std::sync::Arc<dyn EvolutionStore>,
}

impl StoreBackedSelector {
    pub fn new(store: std::sync::Arc<dyn EvolutionStore>) -> Self {
        Self { store }
    }
}

impl Selector for StoreBackedSelector {
    fn select(&self, input: &SelectorInput) -> Vec<GeneCandidate> {
        match self.store.rebuild_projection() {
            Ok(projection) => ProjectionSelector::new(projection).select(input),
            Err(_) => Vec::new(),
        }
    }
}

pub fn rebuild_projection_from_events(events: &[StoredEvolutionEvent]) -> EvolutionProjection {
    let mut genes = BTreeMap::<GeneId, Gene>::new();
    let mut capsules = BTreeMap::<CapsuleId, Capsule>::new();
    let mut reuse_counts = BTreeMap::<GeneId, u64>::new();
    let mut attempt_counts = BTreeMap::<GeneId, u64>::new();
    let mut last_updated_at = BTreeMap::<GeneId, String>::new();
    let mut mutation_to_gene = HashMap::<MutationId, GeneId>::new();

    for stored in events {
        match &stored.event {
            EvolutionEvent::GeneProjected { gene } => {
                genes.insert(gene.id.clone(), gene.clone());
                last_updated_at.insert(gene.id.clone(), stored.timestamp.clone());
            }
            EvolutionEvent::GenePromoted { gene_id } => {
                if let Some(gene) = genes.get_mut(gene_id) {
                    gene.state = AssetState::Promoted;
                }
                last_updated_at.insert(gene_id.clone(), stored.timestamp.clone());
            }
            EvolutionEvent::GeneRevoked { gene_id, .. } => {
                if let Some(gene) = genes.get_mut(gene_id) {
                    gene.state = AssetState::Revoked;
                }
                last_updated_at.insert(gene_id.clone(), stored.timestamp.clone());
            }
            EvolutionEvent::GeneArchived { gene_id } => {
                if let Some(gene) = genes.get_mut(gene_id) {
                    gene.state = AssetState::Archived;
                }
                last_updated_at.insert(gene_id.clone(), stored.timestamp.clone());
            }
            EvolutionEvent::PromotionEvaluated { gene_id, state, .. } => {
                if let Some(gene) = genes.get_mut(gene_id) {
                    gene.state = state.clone();
                }
                last_updated_at.insert(gene_id.clone(), stored.timestamp.clone());
            }
            EvolutionEvent::CapsuleCommitted { capsule } => {
                mutation_to_gene.insert(capsule.mutation_id.clone(), capsule.gene_id.clone());
                capsules.insert(capsule.id.clone(), capsule.clone());
                *attempt_counts.entry(capsule.gene_id.clone()).or_insert(0) += 1;
                last_updated_at.insert(capsule.gene_id.clone(), stored.timestamp.clone());
            }
            EvolutionEvent::CapsuleQuarantined { capsule_id } => {
                if let Some(capsule) = capsules.get_mut(capsule_id) {
                    capsule.state = AssetState::Quarantined;
                    last_updated_at.insert(capsule.gene_id.clone(), stored.timestamp.clone());
                }
            }
            EvolutionEvent::CapsuleReleased { capsule_id, state } => {
                if let Some(capsule) = capsules.get_mut(capsule_id) {
                    capsule.state = state.clone();
                    last_updated_at.insert(capsule.gene_id.clone(), stored.timestamp.clone());
                }
            }
            EvolutionEvent::CapsuleReused { gene_id, .. } => {
                *reuse_counts.entry(gene_id.clone()).or_insert(0) += 1;
                last_updated_at.insert(gene_id.clone(), stored.timestamp.clone());
            }
            EvolutionEvent::ValidationFailed {
                mutation_id,
                gene_id,
                ..
            } => {
                let id = gene_id
                    .clone()
                    .or_else(|| mutation_to_gene.get(mutation_id).cloned());
                if let Some(gene_id) = id {
                    *attempt_counts.entry(gene_id.clone()).or_insert(0) += 1;
                    last_updated_at.insert(gene_id, stored.timestamp.clone());
                }
            }
            EvolutionEvent::ValidationPassed {
                mutation_id,
                gene_id,
                ..
            } => {
                let id = gene_id
                    .clone()
                    .or_else(|| mutation_to_gene.get(mutation_id).cloned());
                if let Some(gene_id) = id {
                    *attempt_counts.entry(gene_id.clone()).or_insert(0) += 1;
                    last_updated_at.insert(gene_id, stored.timestamp.clone());
                }
            }
            _ => {}
        }
    }

    EvolutionProjection {
        genes: genes.into_values().collect(),
        capsules: capsules.into_values().collect(),
        reuse_counts,
        attempt_counts,
        last_updated_at,
    }
}

pub fn default_store_root() -> PathBuf {
    PathBuf::from(".oris").join("evolution")
}

pub fn hash_string(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn stable_hash_json<T: Serialize>(value: &T) -> Result<String, EvolutionError> {
    let bytes = serde_json::to_vec(value).map_err(|err| EvolutionError::Serde(err.to_string()))?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    Ok(hex::encode(hasher.finalize()))
}

pub fn compute_artifact_hash(payload: &str) -> String {
    hash_string(payload)
}

pub fn next_id(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{prefix}-{nanos:x}")
}

fn normalized_signal_overlap(gene_signals: &[String], input_signals: &[String]) -> f64 {
    if input_signals.is_empty() {
        return 0.0;
    }
    let gene = gene_signals
        .iter()
        .map(|signal| signal.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    let input = input_signals
        .iter()
        .map(|signal| signal.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    let matched = input.iter().filter(|signal| gene.contains(*signal)).count() as f64;
    matched / input.len() as f64
}

fn fingerprint_key(env: &EnvFingerprint) -> String {
    format!(
        "{}|{}|{}|{}",
        env.rustc_version, env.cargo_lock_hash, env.target_triple, env.os
    )
}

fn hash_record(
    seq: u64,
    timestamp: &str,
    prev_hash: &str,
    event: &EvolutionEvent,
) -> Result<String, EvolutionError> {
    stable_hash_json(&(seq, timestamp, prev_hash, event))
}

fn verify_hash_chain(events: &[StoredEvolutionEvent]) -> Result<(), EvolutionError> {
    let mut previous_hash = String::new();
    let mut expected_seq = 1u64;
    for event in events {
        if event.seq != expected_seq {
            return Err(EvolutionError::HashChain(format!(
                "expected seq {}, found {}",
                expected_seq, event.seq
            )));
        }
        if event.prev_hash != previous_hash {
            return Err(EvolutionError::HashChain(format!(
                "event {} prev_hash mismatch",
                event.seq
            )));
        }
        let actual_hash = hash_record(event.seq, &event.timestamp, &event.prev_hash, &event.event)?;
        if actual_hash != event.record_hash {
            return Err(EvolutionError::HashChain(format!(
                "event {} record_hash mismatch",
                event.seq
            )));
        }
        previous_hash = event.record_hash.clone();
        expected_seq += 1;
    }
    Ok(())
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), EvolutionError> {
    let tmp_path = path.with_extension("tmp");
    let bytes =
        serde_json::to_vec_pretty(value).map_err(|err| EvolutionError::Serde(err.to_string()))?;
    fs::write(&tmp_path, bytes).map_err(io_err)?;
    fs::rename(&tmp_path, path).map_err(io_err)?;
    Ok(())
}

fn io_err(err: std::io::Error) -> EvolutionError {
    EvolutionError::Io(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("oris-evolution-{name}-{}", next_id("t")))
    }

    fn sample_mutation() -> PreparedMutation {
        PreparedMutation {
            intent: MutationIntent {
                id: "mutation-1".into(),
                intent: "tighten borrow scope".into(),
                target: MutationTarget::Paths {
                    allow: vec!["crates/oris-kernel".into()],
                },
                expected_effect: "cargo check passes".into(),
                risk: RiskLevel::Low,
                signals: vec!["rust borrow error".into()],
                spec_id: None,
            },
            artifact: MutationArtifact {
                encoding: ArtifactEncoding::UnifiedDiff,
                payload: "diff --git a/foo b/foo".into(),
                base_revision: Some("HEAD".into()),
                content_hash: compute_artifact_hash("diff --git a/foo b/foo"),
            },
        }
    }

    #[test]
    fn append_event_assigns_monotonic_seq() {
        let root = temp_root("seq");
        let store = JsonlEvolutionStore::new(root);
        let first = store
            .append_event(EvolutionEvent::MutationDeclared {
                mutation: sample_mutation(),
            })
            .unwrap();
        let second = store
            .append_event(EvolutionEvent::MutationRejected {
                mutation_id: "mutation-1".into(),
                reason: "no-op".into(),
            })
            .unwrap();
        assert_eq!(first, 1);
        assert_eq!(second, 2);
    }

    #[test]
    fn tampered_hash_chain_is_rejected() {
        let root = temp_root("tamper");
        let store = JsonlEvolutionStore::new(&root);
        store
            .append_event(EvolutionEvent::MutationDeclared {
                mutation: sample_mutation(),
            })
            .unwrap();
        let path = root.join("events.jsonl");
        let contents = fs::read_to_string(&path).unwrap();
        let mutated = contents.replace("tighten borrow scope", "tampered");
        fs::write(&path, mutated).unwrap();
        let result = store.scan(1);
        assert!(matches!(result, Err(EvolutionError::HashChain(_))));
    }

    #[test]
    fn rebuild_projection_after_cache_deletion() {
        let root = temp_root("projection");
        let store = JsonlEvolutionStore::new(&root);
        let gene = Gene {
            id: "gene-1".into(),
            signals: vec!["rust borrow error".into()],
            strategy: vec!["crates".into()],
            validation: vec!["oris-default".into()],
            state: AssetState::Promoted,
        };
        let capsule = Capsule {
            id: "capsule-1".into(),
            gene_id: gene.id.clone(),
            mutation_id: "mutation-1".into(),
            run_id: "run-1".into(),
            diff_hash: "abc".into(),
            confidence: 0.7,
            env: EnvFingerprint {
                rustc_version: "rustc 1.80".into(),
                cargo_lock_hash: "lock".into(),
                target_triple: "x86_64-unknown-linux-gnu".into(),
                os: "linux".into(),
            },
            outcome: Outcome {
                success: true,
                validation_profile: "oris-default".into(),
                validation_duration_ms: 100,
                changed_files: vec!["crates/oris-kernel/src/lib.rs".into()],
                validator_hash: "vh".into(),
                lines_changed: 1,
                replay_verified: false,
            },
            state: AssetState::Promoted,
        };
        store
            .append_event(EvolutionEvent::GeneProjected { gene })
            .unwrap();
        store
            .append_event(EvolutionEvent::CapsuleCommitted { capsule })
            .unwrap();
        fs::remove_file(root.join("genes.json")).unwrap();
        fs::remove_file(root.join("capsules.json")).unwrap();
        let projection = store.rebuild_projection().unwrap();
        assert_eq!(projection.genes.len(), 1);
        assert_eq!(projection.capsules.len(), 1);
    }

    #[test]
    fn selector_orders_results_stably() {
        let projection = EvolutionProjection {
            genes: vec![
                Gene {
                    id: "gene-a".into(),
                    signals: vec!["signal".into()],
                    strategy: vec!["a".into()],
                    validation: vec!["oris-default".into()],
                    state: AssetState::Promoted,
                },
                Gene {
                    id: "gene-b".into(),
                    signals: vec!["signal".into()],
                    strategy: vec!["b".into()],
                    validation: vec!["oris-default".into()],
                    state: AssetState::Promoted,
                },
            ],
            capsules: vec![
                Capsule {
                    id: "capsule-a".into(),
                    gene_id: "gene-a".into(),
                    mutation_id: "m1".into(),
                    run_id: "r1".into(),
                    diff_hash: "1".into(),
                    confidence: 0.7,
                    env: EnvFingerprint {
                        rustc_version: "rustc".into(),
                        cargo_lock_hash: "lock".into(),
                        target_triple: "x86_64-unknown-linux-gnu".into(),
                        os: "linux".into(),
                    },
                    outcome: Outcome {
                        success: true,
                        validation_profile: "oris-default".into(),
                        validation_duration_ms: 1,
                        changed_files: vec!["crates/oris-kernel".into()],
                        validator_hash: "v".into(),
                        lines_changed: 1,
                        replay_verified: false,
                    },
                    state: AssetState::Promoted,
                },
                Capsule {
                    id: "capsule-b".into(),
                    gene_id: "gene-b".into(),
                    mutation_id: "m2".into(),
                    run_id: "r2".into(),
                    diff_hash: "2".into(),
                    confidence: 0.7,
                    env: EnvFingerprint {
                        rustc_version: "rustc".into(),
                        cargo_lock_hash: "lock".into(),
                        target_triple: "x86_64-unknown-linux-gnu".into(),
                        os: "linux".into(),
                    },
                    outcome: Outcome {
                        success: true,
                        validation_profile: "oris-default".into(),
                        validation_duration_ms: 1,
                        changed_files: vec!["crates/oris-kernel".into()],
                        validator_hash: "v".into(),
                        lines_changed: 1,
                        replay_verified: false,
                    },
                    state: AssetState::Promoted,
                },
            ],
            reuse_counts: BTreeMap::from([("gene-a".into(), 3), ("gene-b".into(), 3)]),
            attempt_counts: BTreeMap::from([("gene-a".into(), 1), ("gene-b".into(), 1)]),
            last_updated_at: BTreeMap::from([
                ("gene-a".into(), Utc::now().to_rfc3339()),
                ("gene-b".into(), Utc::now().to_rfc3339()),
            ]),
        };
        let selector = ProjectionSelector::new(projection);
        let input = SelectorInput {
            signals: vec!["signal".into()],
            env: EnvFingerprint {
                rustc_version: "rustc".into(),
                cargo_lock_hash: "lock".into(),
                target_triple: "x86_64-unknown-linux-gnu".into(),
                os: "linux".into(),
            },
            limit: 2,
        };
        let first = selector.select(&input);
        let second = selector.select(&input);
        assert_eq!(first.len(), 2);
        assert_eq!(
            first
                .iter()
                .map(|candidate| candidate.gene.id.clone())
                .collect::<Vec<_>>(),
            second
                .iter()
                .map(|candidate| candidate.gene.id.clone())
                .collect::<Vec<_>>()
        );
    }
}
