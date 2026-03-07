//! Evolution domain model, append-only event store, projections, and selector logic.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Duration, Utc};
use oris_kernel::RunId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub type MutationId = String;
pub type GeneId = String;
pub type CapsuleId = String;

pub const REPLAY_CONFIDENCE_DECAY_RATE_PER_HOUR: f32 = 0.05;
pub const MIN_REPLAY_CONFIDENCE: f32 = 0.35;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum AssetState {
    Candidate,
    #[default]
    Promoted,
    Revoked,
    Archived,
    Quarantined,
}

/// Convert Oris AssetState to EvoMap-compatible state string.
/// This mapping preserves the EvoMap terminology without modifying the core enum.
pub fn asset_state_to_evomap_compat(state: &AssetState) -> &'static str {
    match state {
        AssetState::Candidate => "candidate",
        AssetState::Promoted => "promoted",
        AssetState::Revoked => "revoked",
        AssetState::Archived => "rejected", // Archive maps to rejected in EvoMap terms
        AssetState::Quarantined => "quarantined",
    }
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
    SignalsExtracted {
        mutation_id: MutationId,
        hash: String,
        signals: Vec<String>,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        replay_run_id: Option<RunId>,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sender_id: Option<String>,
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
    pub spec_ids_by_gene: BTreeMap<GeneId, BTreeSet<String>>,
}

#[derive(Clone, Debug)]
pub struct SelectorInput {
    pub signals: Vec<String>,
    pub env: EnvFingerprint,
    pub spec_id: Option<String>,
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

    fn scan_projection(
        &self,
    ) -> Result<(Vec<StoredEvolutionEvent>, EvolutionProjection), EvolutionError> {
        let events = self.scan(1)?;
        let projection = rebuild_projection_from_events(&events);
        Ok((events, projection))
    }
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

    fn scan_projection(
        &self,
    ) -> Result<(Vec<StoredEvolutionEvent>, EvolutionProjection), EvolutionError> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| EvolutionError::Io("evolution store lock poisoned".into()))?;
        let events = self.read_all_events()?;
        let projection = rebuild_projection_from_events(&events);
        self.write_projection_files(&projection)?;
        Ok((events, projection))
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
        let requested_spec_id = input
            .spec_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let mut out = Vec::new();
        for gene in &self.projection.genes {
            if gene.state != AssetState::Promoted {
                continue;
            }
            if let Some(spec_id) = requested_spec_id {
                let matches_spec = self
                    .projection
                    .spec_ids_by_gene
                    .get(&gene.id)
                    .map(|values| {
                        values
                            .iter()
                            .any(|value| value.eq_ignore_ascii_case(spec_id))
                    })
                    .unwrap_or(false);
                if !matches_spec {
                    continue;
                }
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
            let mut capsules = capsules;
            capsules.sort_by(|left, right| {
                environment_match_factor(&input.env, &right.env)
                    .partial_cmp(&environment_match_factor(&input.env, &left.env))
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| {
                        right
                            .confidence
                            .partial_cmp(&left.confidence)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .then_with(|| left.id.cmp(&right.id))
            });
            let env_match_factor = capsules
                .first()
                .map(|capsule| environment_match_factor(&input.env, &capsule.env))
                .unwrap_or(0.0);

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
            let signal_overlap = normalized_signal_overlap(&gene.signals, &input.signals);
            let age_secs = self
                .projection
                .last_updated_at
                .get(&gene.id)
                .and_then(|value| seconds_since_timestamp(value, self.now));
            let peak_confidence = capsules
                .iter()
                .map(|capsule| capsule.confidence)
                .fold(0.0_f32, f32::max) as f64;
            let freshness_confidence = capsules
                .iter()
                .map(|capsule| decayed_replay_confidence(capsule.confidence, age_secs))
                .fold(0.0_f32, f32::max) as f64;
            if freshness_confidence < MIN_REPLAY_CONFIDENCE as f64 {
                continue;
            }
            let freshness_factor = if peak_confidence <= 0.0 {
                0.0
            } else {
                (freshness_confidence / peak_confidence).clamp(0.0, 1.0)
            };
            let score = (success_rate
                * reuse_count_factor
                * env_match_factor
                * freshness_factor
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
        match self.store.scan_projection() {
            Ok((_, projection)) => ProjectionSelector::new(projection).select(input),
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
    let mut spec_ids_by_gene = BTreeMap::<GeneId, BTreeSet<String>>::new();
    let mut mutation_to_gene = HashMap::<MutationId, GeneId>::new();
    let mut mutation_spec_ids = HashMap::<MutationId, String>::new();

    for stored in events {
        match &stored.event {
            EvolutionEvent::MutationDeclared { mutation } => {
                if let Some(spec_id) = mutation
                    .intent
                    .spec_id
                    .as_ref()
                    .map(|value| value.trim())
                    .filter(|value| !value.is_empty())
                {
                    mutation_spec_ids.insert(mutation.intent.id.clone(), spec_id.to_string());
                    if let Some(gene_id) = mutation_to_gene.get(&mutation.intent.id) {
                        spec_ids_by_gene
                            .entry(gene_id.clone())
                            .or_default()
                            .insert(spec_id.to_string());
                    }
                }
            }
            EvolutionEvent::SpecLinked {
                mutation_id,
                spec_id,
            } => {
                let spec_id = spec_id.trim();
                if !spec_id.is_empty() {
                    mutation_spec_ids.insert(mutation_id.clone(), spec_id.to_string());
                    if let Some(gene_id) = mutation_to_gene.get(mutation_id) {
                        spec_ids_by_gene
                            .entry(gene_id.clone())
                            .or_default()
                            .insert(spec_id.to_string());
                    }
                }
            }
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
                if let Some(spec_id) = mutation_spec_ids.get(&capsule.mutation_id) {
                    spec_ids_by_gene
                        .entry(capsule.gene_id.clone())
                        .or_default()
                        .insert(spec_id.clone());
                }
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
        spec_ids_by_gene,
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

pub fn decayed_replay_confidence(confidence: f32, age_secs: Option<u64>) -> f32 {
    if confidence <= 0.0 {
        return 0.0;
    }
    let age_hours = age_secs.unwrap_or(0) as f32 / 3600.0;
    let decay = (-REPLAY_CONFIDENCE_DECAY_RATE_PER_HOUR * age_hours).exp();
    (confidence * decay).clamp(0.0, 1.0)
}

fn normalized_signal_overlap(gene_signals: &[String], input_signals: &[String]) -> f64 {
    let gene = canonical_signal_phrases(gene_signals);
    let input = canonical_signal_phrases(input_signals);
    if input.is_empty() || gene.is_empty() {
        return 0.0;
    }
    let matched = input
        .iter()
        .map(|signal| best_signal_match(&gene, signal))
        .sum::<f64>();
    matched / input.len() as f64
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CanonicalSignal {
    phrase: String,
    tokens: BTreeSet<String>,
}

fn canonical_signal_phrases(signals: &[String]) -> Vec<CanonicalSignal> {
    signals
        .iter()
        .filter_map(|signal| canonical_signal_phrase(signal))
        .collect()
}

fn canonical_signal_phrase(input: &str) -> Option<CanonicalSignal> {
    let tokens = input
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .map(|token| token.trim().to_ascii_lowercase())
        .filter(|token| token.len() >= 3)
        .collect::<BTreeSet<_>>();
    if tokens.is_empty() {
        return None;
    }
    let phrase = tokens.iter().cloned().collect::<Vec<_>>().join(" ");
    Some(CanonicalSignal { phrase, tokens })
}

fn best_signal_match(gene_signals: &[CanonicalSignal], input: &CanonicalSignal) -> f64 {
    gene_signals
        .iter()
        .map(|candidate| deterministic_phrase_match(candidate, input))
        .fold(0.0, f64::max)
}

fn deterministic_phrase_match(candidate: &CanonicalSignal, input: &CanonicalSignal) -> f64 {
    if candidate.phrase == input.phrase {
        return 1.0;
    }
    if candidate.tokens.len() < 2 || input.tokens.len() < 2 {
        return 0.0;
    }
    let shared = candidate.tokens.intersection(&input.tokens).count();
    if shared < 2 {
        return 0.0;
    }
    let overlap = shared as f64 / candidate.tokens.len().min(input.tokens.len()) as f64;
    if overlap >= 0.67 {
        overlap
    } else {
        0.0
    }
}
fn seconds_since_timestamp(timestamp: &str, now: DateTime<Utc>) -> Option<u64> {
    let parsed = DateTime::parse_from_rfc3339(timestamp)
        .ok()?
        .with_timezone(&Utc);
    let elapsed = now.signed_duration_since(parsed);
    if elapsed < Duration::zero() {
        Some(0)
    } else {
        u64::try_from(elapsed.num_seconds()).ok()
    }
}
fn environment_match_factor(input: &EnvFingerprint, candidate: &EnvFingerprint) -> f64 {
    let fields = [
        input
            .rustc_version
            .eq_ignore_ascii_case(&candidate.rustc_version),
        input
            .cargo_lock_hash
            .eq_ignore_ascii_case(&candidate.cargo_lock_hash),
        input
            .target_triple
            .eq_ignore_ascii_case(&candidate.target_triple),
        input.os.eq_ignore_ascii_case(&candidate.os),
    ];
    let matched_fields = fields.into_iter().filter(|matched| *matched).count() as f64;
    0.5 + ((matched_fields / 4.0) * 0.5)
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
    fn rebuild_projection_tracks_spec_ids_for_genes() {
        let root = temp_root("projection-spec");
        let store = JsonlEvolutionStore::new(&root);
        let mut mutation = sample_mutation();
        mutation.intent.id = "mutation-spec".into();
        mutation.intent.spec_id = Some("spec-repair-1".into());
        let gene = Gene {
            id: "gene-spec".into(),
            signals: vec!["rust borrow error".into()],
            strategy: vec!["crates".into()],
            validation: vec!["oris-default".into()],
            state: AssetState::Promoted,
        };
        let capsule = Capsule {
            id: "capsule-spec".into(),
            gene_id: gene.id.clone(),
            mutation_id: mutation.intent.id.clone(),
            run_id: "run-spec".into(),
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
            .append_event(EvolutionEvent::MutationDeclared { mutation })
            .unwrap();
        store
            .append_event(EvolutionEvent::GeneProjected { gene })
            .unwrap();
        store
            .append_event(EvolutionEvent::CapsuleCommitted { capsule })
            .unwrap();

        let projection = store.rebuild_projection().unwrap();
        let spec_ids = projection.spec_ids_by_gene.get("gene-spec").unwrap();
        assert!(spec_ids.contains("spec-repair-1"));
    }

    #[test]
    fn rebuild_projection_tracks_spec_ids_from_spec_linked_events() {
        let root = temp_root("projection-spec-linked");
        let store = JsonlEvolutionStore::new(&root);
        let mut mutation = sample_mutation();
        mutation.intent.id = "mutation-spec-linked".into();
        mutation.intent.spec_id = None;
        let gene = Gene {
            id: "gene-spec-linked".into(),
            signals: vec!["rust borrow error".into()],
            strategy: vec!["crates".into()],
            validation: vec!["oris-default".into()],
            state: AssetState::Promoted,
        };
        let capsule = Capsule {
            id: "capsule-spec-linked".into(),
            gene_id: gene.id.clone(),
            mutation_id: mutation.intent.id.clone(),
            run_id: "run-spec-linked".into(),
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
            .append_event(EvolutionEvent::MutationDeclared { mutation })
            .unwrap();
        store
            .append_event(EvolutionEvent::GeneProjected { gene })
            .unwrap();
        store
            .append_event(EvolutionEvent::CapsuleCommitted { capsule })
            .unwrap();
        store
            .append_event(EvolutionEvent::SpecLinked {
                mutation_id: "mutation-spec-linked".into(),
                spec_id: "spec-repair-linked".into(),
            })
            .unwrap();

        let projection = store.rebuild_projection().unwrap();
        let spec_ids = projection.spec_ids_by_gene.get("gene-spec-linked").unwrap();
        assert!(spec_ids.contains("spec-repair-linked"));
    }

    #[test]
    fn rebuild_projection_tracks_inline_spec_ids_even_when_declared_late() {
        let root = temp_root("projection-spec-inline-late");
        let store = JsonlEvolutionStore::new(&root);
        let mut mutation = sample_mutation();
        mutation.intent.id = "mutation-inline-late".into();
        mutation.intent.spec_id = Some("spec-inline-late".into());
        let gene = Gene {
            id: "gene-inline-late".into(),
            signals: vec!["rust borrow error".into()],
            strategy: vec!["crates".into()],
            validation: vec!["oris-default".into()],
            state: AssetState::Promoted,
        };
        let capsule = Capsule {
            id: "capsule-inline-late".into(),
            gene_id: gene.id.clone(),
            mutation_id: mutation.intent.id.clone(),
            run_id: "run-inline-late".into(),
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
        store
            .append_event(EvolutionEvent::MutationDeclared { mutation })
            .unwrap();

        let projection = store.rebuild_projection().unwrap();
        let spec_ids = projection.spec_ids_by_gene.get("gene-inline-late").unwrap();
        assert!(spec_ids.contains("spec-inline-late"));
    }

    #[test]
    fn scan_projection_recreates_projection_files() {
        let root = temp_root("scan-projection");
        let store = JsonlEvolutionStore::new(&root);
        let mutation = sample_mutation();
        let gene = Gene {
            id: "gene-scan".into(),
            signals: vec!["rust borrow error".into()],
            strategy: vec!["crates".into()],
            validation: vec!["oris-default".into()],
            state: AssetState::Promoted,
        };
        let capsule = Capsule {
            id: "capsule-scan".into(),
            gene_id: gene.id.clone(),
            mutation_id: mutation.intent.id.clone(),
            run_id: "run-scan".into(),
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
            .append_event(EvolutionEvent::MutationDeclared { mutation })
            .unwrap();
        store
            .append_event(EvolutionEvent::GeneProjected { gene })
            .unwrap();
        store
            .append_event(EvolutionEvent::CapsuleCommitted { capsule })
            .unwrap();
        fs::remove_file(root.join("genes.json")).unwrap();
        fs::remove_file(root.join("capsules.json")).unwrap();

        let (events, projection) = store.scan_projection().unwrap();

        assert_eq!(events.len(), 3);
        assert_eq!(projection.genes.len(), 1);
        assert_eq!(projection.capsules.len(), 1);
        assert!(root.join("genes.json").exists());
        assert!(root.join("capsules.json").exists());
    }

    #[test]
    fn default_scan_projection_uses_single_event_snapshot() {
        struct InconsistentSnapshotStore {
            scanned_events: Vec<StoredEvolutionEvent>,
            rebuilt_projection: EvolutionProjection,
        }

        impl EvolutionStore for InconsistentSnapshotStore {
            fn append_event(&self, _event: EvolutionEvent) -> Result<u64, EvolutionError> {
                Err(EvolutionError::Io("unused in test".into()))
            }

            fn scan(&self, from_seq: u64) -> Result<Vec<StoredEvolutionEvent>, EvolutionError> {
                Ok(self
                    .scanned_events
                    .iter()
                    .filter(|stored| stored.seq >= from_seq)
                    .cloned()
                    .collect())
            }

            fn rebuild_projection(&self) -> Result<EvolutionProjection, EvolutionError> {
                Ok(self.rebuilt_projection.clone())
            }
        }

        let scanned_gene = Gene {
            id: "gene-scanned".into(),
            signals: vec!["signal".into()],
            strategy: vec!["a".into()],
            validation: vec!["oris-default".into()],
            state: AssetState::Promoted,
        };
        let store = InconsistentSnapshotStore {
            scanned_events: vec![StoredEvolutionEvent {
                seq: 1,
                timestamp: "2026-03-04T00:00:00Z".into(),
                prev_hash: String::new(),
                record_hash: "hash".into(),
                event: EvolutionEvent::GeneProjected {
                    gene: scanned_gene.clone(),
                },
            }],
            rebuilt_projection: EvolutionProjection {
                genes: vec![Gene {
                    id: "gene-rebuilt".into(),
                    signals: vec!["other".into()],
                    strategy: vec!["b".into()],
                    validation: vec!["oris-default".into()],
                    state: AssetState::Promoted,
                }],
                ..Default::default()
            },
        };

        let (events, projection) = store.scan_projection().unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(projection.genes.len(), 1);
        assert_eq!(projection.genes[0].id, scanned_gene.id);
    }

    #[test]
    fn store_backed_selector_uses_scan_projection_contract() {
        struct InconsistentSnapshotStore {
            scanned_events: Vec<StoredEvolutionEvent>,
            rebuilt_projection: EvolutionProjection,
        }

        impl EvolutionStore for InconsistentSnapshotStore {
            fn append_event(&self, _event: EvolutionEvent) -> Result<u64, EvolutionError> {
                Err(EvolutionError::Io("unused in test".into()))
            }

            fn scan(&self, from_seq: u64) -> Result<Vec<StoredEvolutionEvent>, EvolutionError> {
                Ok(self
                    .scanned_events
                    .iter()
                    .filter(|stored| stored.seq >= from_seq)
                    .cloned()
                    .collect())
            }

            fn rebuild_projection(&self) -> Result<EvolutionProjection, EvolutionError> {
                Ok(self.rebuilt_projection.clone())
            }
        }

        let scanned_gene = Gene {
            id: "gene-scanned".into(),
            signals: vec!["signal".into()],
            strategy: vec!["a".into()],
            validation: vec!["oris-default".into()],
            state: AssetState::Promoted,
        };
        let scanned_capsule = Capsule {
            id: "capsule-scanned".into(),
            gene_id: scanned_gene.id.clone(),
            mutation_id: "mutation-scanned".into(),
            run_id: "run-scanned".into(),
            diff_hash: "hash".into(),
            confidence: 0.8,
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
                changed_files: vec!["file.rs".into()],
                validator_hash: "validator".into(),
                lines_changed: 1,
                replay_verified: false,
            },
            state: AssetState::Promoted,
        };
        let fresh_ts = Utc::now().to_rfc3339();
        let store = std::sync::Arc::new(InconsistentSnapshotStore {
            scanned_events: vec![
                StoredEvolutionEvent {
                    seq: 1,
                    timestamp: fresh_ts.clone(),
                    prev_hash: String::new(),
                    record_hash: "hash-1".into(),
                    event: EvolutionEvent::GeneProjected {
                        gene: scanned_gene.clone(),
                    },
                },
                StoredEvolutionEvent {
                    seq: 2,
                    timestamp: fresh_ts,
                    prev_hash: "hash-1".into(),
                    record_hash: "hash-2".into(),
                    event: EvolutionEvent::CapsuleCommitted {
                        capsule: scanned_capsule.clone(),
                    },
                },
            ],
            rebuilt_projection: EvolutionProjection {
                genes: vec![Gene {
                    id: "gene-rebuilt".into(),
                    signals: vec!["other".into()],
                    strategy: vec!["b".into()],
                    validation: vec!["oris-default".into()],
                    state: AssetState::Promoted,
                }],
                ..Default::default()
            },
        });
        let selector = StoreBackedSelector::new(store);
        let input = SelectorInput {
            signals: vec!["signal".into()],
            env: scanned_capsule.env.clone(),
            spec_id: None,
            limit: 1,
        };

        let candidates = selector.select(&input);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].gene.id, scanned_gene.id);
        assert_eq!(candidates[0].capsules[0].id, scanned_capsule.id);
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
            spec_ids_by_gene: BTreeMap::new(),
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
            spec_id: None,
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

    #[test]
    fn selector_can_narrow_by_spec_id() {
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
            spec_ids_by_gene: BTreeMap::from([
                ("gene-a".into(), BTreeSet::from(["spec-a".to_string()])),
                ("gene-b".into(), BTreeSet::from(["spec-b".to_string()])),
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
            spec_id: Some("spec-b".into()),
            limit: 2,
        };
        let selected = selector.select(&input);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].gene.id, "gene-b");
    }

    #[test]
    fn selector_prefers_closest_environment_match() {
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
                    id: "capsule-a-stale".into(),
                    gene_id: "gene-a".into(),
                    mutation_id: "m1".into(),
                    run_id: "r1".into(),
                    diff_hash: "1".into(),
                    confidence: 0.2,
                    env: EnvFingerprint {
                        rustc_version: "old-rustc".into(),
                        cargo_lock_hash: "other-lock".into(),
                        target_triple: "aarch64-apple-darwin".into(),
                        os: "macos".into(),
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
                    id: "capsule-a-best".into(),
                    gene_id: "gene-a".into(),
                    mutation_id: "m2".into(),
                    run_id: "r2".into(),
                    diff_hash: "2".into(),
                    confidence: 0.9,
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
                    mutation_id: "m3".into(),
                    run_id: "r3".into(),
                    diff_hash: "3".into(),
                    confidence: 0.7,
                    env: EnvFingerprint {
                        rustc_version: "rustc".into(),
                        cargo_lock_hash: "different-lock".into(),
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
            attempt_counts: BTreeMap::from([("gene-a".into(), 2), ("gene-b".into(), 1)]),
            last_updated_at: BTreeMap::from([
                ("gene-a".into(), Utc::now().to_rfc3339()),
                ("gene-b".into(), Utc::now().to_rfc3339()),
            ]),
            spec_ids_by_gene: BTreeMap::new(),
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
            spec_id: None,
            limit: 2,
        };

        let selected = selector.select(&input);

        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].gene.id, "gene-a");
        assert_eq!(selected[0].capsules[0].id, "capsule-a-best");
        assert!(selected[0].score > selected[1].score);
    }

    #[test]
    fn selector_preserves_fresh_candidate_scores_while_ranking_by_confidence() {
        let now = Utc::now();
        let projection = EvolutionProjection {
            genes: vec![Gene {
                id: "gene-fresh".into(),
                signals: vec!["missing".into()],
                strategy: vec!["a".into()],
                validation: vec!["oris-default".into()],
                state: AssetState::Promoted,
            }],
            capsules: vec![Capsule {
                id: "capsule-fresh".into(),
                gene_id: "gene-fresh".into(),
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
                    changed_files: vec!["README.md".into()],
                    validator_hash: "v".into(),
                    lines_changed: 1,
                    replay_verified: false,
                },
                state: AssetState::Promoted,
            }],
            reuse_counts: BTreeMap::from([("gene-fresh".into(), 1)]),
            attempt_counts: BTreeMap::from([("gene-fresh".into(), 1)]),
            last_updated_at: BTreeMap::from([("gene-fresh".into(), now.to_rfc3339())]),
            spec_ids_by_gene: BTreeMap::new(),
        };
        let selector = ProjectionSelector::with_now(projection, now);
        let input = SelectorInput {
            signals: vec![
                "missing".into(),
                "token-a".into(),
                "token-b".into(),
                "token-c".into(),
            ],
            env: EnvFingerprint {
                rustc_version: "rustc".into(),
                cargo_lock_hash: "lock".into(),
                target_triple: "x86_64-unknown-linux-gnu".into(),
                os: "linux".into(),
            },
            spec_id: None,
            limit: 1,
        };

        let selected = selector.select(&input);

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].gene.id, "gene-fresh");
        assert!(selected[0].score > 0.35);
    }

    #[test]
    fn selector_skips_stale_candidates_after_confidence_decay() {
        let now = Utc::now();
        let projection = EvolutionProjection {
            genes: vec![Gene {
                id: "gene-stale".into(),
                signals: vec!["missing readme".into()],
                strategy: vec!["a".into()],
                validation: vec!["oris-default".into()],
                state: AssetState::Promoted,
            }],
            capsules: vec![Capsule {
                id: "capsule-stale".into(),
                gene_id: "gene-stale".into(),
                mutation_id: "m1".into(),
                run_id: "r1".into(),
                diff_hash: "1".into(),
                confidence: 0.8,
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
                    changed_files: vec!["README.md".into()],
                    validator_hash: "v".into(),
                    lines_changed: 1,
                    replay_verified: false,
                },
                state: AssetState::Promoted,
            }],
            reuse_counts: BTreeMap::from([("gene-stale".into(), 2)]),
            attempt_counts: BTreeMap::from([("gene-stale".into(), 1)]),
            last_updated_at: BTreeMap::from([(
                "gene-stale".into(),
                (now - chrono::Duration::hours(48)).to_rfc3339(),
            )]),
            spec_ids_by_gene: BTreeMap::new(),
        };
        let selector = ProjectionSelector::with_now(projection, now);
        let input = SelectorInput {
            signals: vec!["missing readme".into()],
            env: EnvFingerprint {
                rustc_version: "rustc".into(),
                cargo_lock_hash: "lock".into(),
                target_triple: "x86_64-unknown-linux-gnu".into(),
                os: "linux".into(),
            },
            spec_id: None,
            limit: 1,
        };

        let selected = selector.select(&input);

        assert!(selected.is_empty());
        assert!(decayed_replay_confidence(0.8, Some(48 * 60 * 60)) < MIN_REPLAY_CONFIDENCE);
    }

    #[test]
    fn legacy_capsule_reused_events_deserialize_without_replay_run_id() {
        let serialized = r#"{
  "seq": 1,
  "timestamp": "2026-03-04T00:00:00Z",
  "prev_hash": "",
  "record_hash": "hash",
  "event": {
    "kind": "capsule_reused",
    "capsule_id": "capsule-1",
    "gene_id": "gene-1",
    "run_id": "run-1"
  }
}"#;

        let stored = serde_json::from_str::<StoredEvolutionEvent>(serialized).unwrap();

        match stored.event {
            EvolutionEvent::CapsuleReused {
                capsule_id,
                gene_id,
                run_id,
                replay_run_id,
            } => {
                assert_eq!(capsule_id, "capsule-1");
                assert_eq!(gene_id, "gene-1");
                assert_eq!(run_id, "run-1");
                assert_eq!(replay_run_id, None);
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn legacy_remote_asset_imported_events_deserialize_without_sender_id() {
        let serialized = r#"{
  "seq": 1,
  "timestamp": "2026-03-04T00:00:00Z",
  "prev_hash": "",
  "record_hash": "hash",
  "event": {
    "kind": "remote_asset_imported",
    "source": "Remote",
    "asset_ids": ["gene-1"]
  }
}"#;

        let stored = serde_json::from_str::<StoredEvolutionEvent>(serialized).unwrap();

        match stored.event {
            EvolutionEvent::RemoteAssetImported {
                source,
                asset_ids,
                sender_id,
            } => {
                assert_eq!(source, CandidateSource::Remote);
                assert_eq!(asset_ids, vec!["gene-1"]);
                assert_eq!(sender_id, None);
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }
}
