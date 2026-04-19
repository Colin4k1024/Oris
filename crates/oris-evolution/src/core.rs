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

/// Stable identifier for a pending or applied mutation.
pub type MutationId = String;
/// Stable identifier for a Gene (reusable solution template).
pub type GeneId = String;
/// Stable identifier for a Capsule (concrete, validated mutation result).
pub type CapsuleId = String;

/// Hourly confidence decay applied to replayed capsules.
pub const REPLAY_CONFIDENCE_DECAY_RATE_PER_HOUR: f32 = 0.05;
/// Minimum confidence below which a capsule is not eligible for replay.
pub const MIN_REPLAY_CONFIDENCE: f32 = 0.35;

/// Lifecycle state of a Gene or Capsule asset in the evolution store.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum AssetState {
    /// Newly created; awaiting validation or promotion.
    Candidate,
    /// Validated and active; eligible for replay and reuse.
    #[default]
    Promoted,
    /// Deliberately withdrawn; no longer used for new replays.
    Revoked,
    /// Superseded or retired; retained for audit only.
    Archived,
    /// Flagged for investigation; not used for new replays.
    Quarantined,
    /// Passed shadow validation; candidate for full promotion.
    ShadowValidated,
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
        // EvoMap does not yet model shadow trust directly, so map it to candidate semantics.
        AssetState::ShadowValidated => "candidate",
    }
}

/// Origin of a candidate asset — whether it was produced locally or received from a peer.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum CandidateSource {
    /// Produced by the local evolution runtime.
    #[default]
    Local,
    /// Received from a remote peer via the evolution network.
    Remote,
}

/// Machine-readable reason code recorded when an asset changes state.
///
/// Used in `PromotionEvaluated` events to explain why a Gene was promoted,
/// downgraded, rate-limited, or put in cooldown.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransitionReasonCode {
    /// No specific reason recorded.
    #[default]
    Unspecified,
    /// Gene met the success-rate threshold for promotion.
    PromotionSuccessThreshold,
    /// Gene was promoted after remote replay validation passed.
    PromotionRemoteReplayValidated,
    /// Gene promoted via built-in cold-start compatibility check.
    PromotionBuiltinColdStartCompatibility,
    /// Gene promoted based on a trusted local validation report.
    PromotionTrustedLocalReport,
    /// Gene re-entered validation due to confidence decay over time.
    RevalidationConfidenceDecay,
    /// Gene was downgraded because replay performance regressed.
    DowngradeReplayRegression,
    /// Gene was downgraded because its confidence score fell below threshold.
    DowngradeConfidenceRegression,
    /// Remote-sourced gene requires local validation before promotion.
    DowngradeRemoteRequiresLocalValidation,
    /// Bootstrap-phase gene requires local validation before promotion.
    DowngradeBootstrapRequiresLocalValidation,
    /// Built-in gene requires explicit validation pass before promotion.
    DowngradeBuiltinRequiresValidation,
    /// Gene is held as candidate due to rate limiting.
    CandidateRateLimited,
    /// Gene is in a mandatory cooling window after a failure.
    CandidateCoolingWindow,
    /// Gene exceeds allowed blast-radius limits.
    CandidateBlastRadiusExceeded,
    /// Gene is actively collecting replay evidence before promotion.
    CandidateCollectingEvidence,
    /// Gene passed shadow validation and is eligible for full promotion.
    PromotionShadowValidationPassed,
    /// Gene reached shadow-mode replay threshold.
    PromotionShadowThresholdPassed,
    /// Gene is in shadow mode collecting replay evidence.
    ShadowCollectingReplayEvidence,
}

/// Reason code for a replay ROI (return-on-investment) accounting record.
///
/// Captured in `ReplayEconomicsRecorded` events. `ReplayHit` means the
/// capsule was reused successfully; `ReplayMiss*` variants explain why it was not.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReplayRoiReasonCode {
    /// No specific reason recorded.
    #[default]
    Unspecified,
    /// The capsule was successfully replayed (cache hit).
    ReplayHit,
    /// No gene matched the incoming signal pattern.
    ReplayMissNoMatchingGene,
    /// The best candidate's score was below the replay threshold.
    ReplayMissScoreBelowThreshold,
    /// The matched gene has no associated capsule to replay.
    ReplayMissCandidateHasNoCapsule,
    /// The capsule's mutation diff payload is missing from the store.
    ReplayMissMutationPayloadMissing,
    /// Applying the capsule's patch to the workspace failed.
    ReplayMissPatchApplyFailed,
    /// The replayed patch failed validation (build or tests).
    ReplayMissValidationFailed,
}

/// Evidence snapshot attached to a `PromotionEvaluated` event.
///
/// Fields are optional; only those relevant to the specific transition reason are populated.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct TransitionEvidence {
    /// Total replay attempts used to compute the success rate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay_attempts: Option<u64>,
    /// Number of replay attempts that succeeded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay_successes: Option<u64>,
    /// Ratio of successes to attempts (in `[0.0, 1.0]`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay_success_rate: Option<f32>,
    /// How closely the current environment matches the capsule's build environment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment_match_factor: Option<f32>,
    /// Confidence score after applying time-based decay.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decayed_confidence: Option<f32>,
    /// Ratio of decayed confidence to original confidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence_decay_ratio: Option<f32>,
    /// Human-readable summary of the transition evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

/// Economic accounting record for a single replay attempt.
///
/// Attached to `ReplayEconomicsRecorded` events and used by `oris-economics`
/// to update the local EVU ledger and gene reputation scores.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct ReplayRoiEvidence {
    /// `true` if the replay was successful (patch applied and validated).
    pub success: bool,
    /// Machine-readable reason for the replay outcome.
    #[serde(default)]
    pub reason_code: ReplayRoiReasonCode,
    /// Task class ID that was matched for this replay.
    pub task_class_id: String,
    /// Human-readable label for the matched task class.
    pub task_label: String,
    /// LLM reasoning tokens saved by replaying instead of re-mutating.
    pub reasoning_avoided_tokens: u64,
    /// Cost (in tokens or EVU) of the replay fallback path.
    pub replay_fallback_cost: u64,
    /// Net ROI of the replay: `reasoning_avoided_tokens - replay_fallback_cost`.
    pub replay_roi: f64,
    /// Origin identifier of the asset that was replayed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_origin: Option<String>,
    /// Peer sender ID if the asset was received from a remote node.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_sender_id: Option<String>,
    /// Context dimensions used to match the task class.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context_dimensions: Vec<String>,
}

/// Estimated scope of a mutation, used by the governor to enforce safety limits.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlastRadius {
    /// Number of source files touched by the mutation.
    pub files_changed: usize,
    /// Number of lines added or removed by the mutation.
    pub lines_changed: usize,
}

/// Estimated risk of applying a mutation to the codebase.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum RiskLevel {
    /// Minor, well-contained change.
    Low,
    /// Moderate change; review recommended.
    Medium,
    /// Broad or structural change; careful validation required.
    High,
}

/// Wire encoding used for a mutation artifact payload.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ArtifactEncoding {
    /// Standard unified diff format (`--- a/...` / `+++ b/...`).
    UnifiedDiff,
}

/// Scope constraint for where a mutation is allowed to apply changes.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum MutationTarget {
    /// Any path under the Cargo workspace root.
    WorkspaceRoot,
    /// A specific named crate within the workspace.
    Crate {
        /// Crate name as it appears in `Cargo.toml`.
        name: String,
    },
    /// An explicit allowlist of file or directory paths.
    Paths {
        /// Allowed paths (relative to workspace root).
        allow: Vec<String>,
    },
}

/// Declarative intent for a mutation: what to change and why.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MutationIntent {
    /// Unique identifier for this mutation intent.
    pub id: MutationId,
    /// Human-readable description of the goal.
    pub intent: String,
    /// Scope constraint (workspace, crate, or explicit paths).
    pub target: MutationTarget,
    /// Expected observable effect after the mutation is applied.
    pub expected_effect: String,
    /// Estimated risk level for governor policy decisions.
    pub risk: RiskLevel,
    /// Diagnostic signals that triggered or inform this mutation.
    pub signals: Vec<String>,
    /// Optional OUSL spec contract that this mutation satisfies.
    #[serde(default)]
    pub spec_id: Option<String>,
}

/// Concrete patch artifact produced by the mutation step.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MutationArtifact {
    /// Encoding format of `payload` (currently always `UnifiedDiff`).
    pub encoding: ArtifactEncoding,
    /// The patch payload (e.g. a unified diff string).
    pub payload: String,
    /// Git revision the diff was generated against, if known.
    pub base_revision: Option<String>,
    /// SHA-256 hex hash of `payload` for integrity verification.
    pub content_hash: String,
}

/// A mutation ready for sandbox execution: intent paired with its artifact.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PreparedMutation {
    /// Declarative description of what the mutation should do.
    pub intent: MutationIntent,
    /// Concrete patch to apply.
    pub artifact: MutationArtifact,
}

/// Immutable record of one validation run (build + test suite).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ValidationSnapshot {
    /// `true` if the build and all tests passed.
    pub success: bool,
    /// Name of the validation profile used (e.g. `"release"`, `"full"`).
    pub profile: String,
    /// Wall-clock duration of the validation run in milliseconds.
    pub duration_ms: u64,
    /// Human-readable summary (pass/fail counts, first failure, etc.).
    pub summary: String,
}

/// Final result recorded when a sandboxed mutation execution completes.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Outcome {
    /// `true` if the build and test suite passed.
    pub success: bool,
    /// Name of the validation profile used.
    pub validation_profile: String,
    /// Total validation time in milliseconds.
    pub validation_duration_ms: u64,
    /// Relative paths of files modified by the mutation.
    pub changed_files: Vec<String>,
    /// Hash identifying the validator binary or configuration.
    pub validator_hash: String,
    /// Total lines added and removed (used for blast-radius accounting).
    #[serde(default)]
    pub lines_changed: usize,
    /// `true` if the mutation has been verified by a replay run.
    #[serde(default)]
    pub replay_verified: bool,
}

/// Snapshot of the build environment at the time a Capsule was created.
///
/// Two capsules with different `EnvFingerprint`s may not be safely interchangeable;
/// the selector uses this to bias toward environment-matching candidates.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnvFingerprint {
    /// Rustc version string (e.g. `"rustc 1.76.0 (07dca489a 2024-02-04)"`).
    pub rustc_version: String,
    /// SHA-256 hash of `Cargo.lock` at build time.
    pub cargo_lock_hash: String,
    /// Target triple (e.g. `"x86_64-unknown-linux-gnu"`).
    pub target_triple: String,
    /// Operating system name (e.g. `"linux"`).
    pub os: String,
}

/// A validated, immutable record of one successful mutation execution.
///
/// A `Capsule` is the unit of reuse: when a matching signal pattern is seen again,
/// the pipeline replays the capsule's diff instead of re-running the full mutation.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Capsule {
    /// Unique identifier for this capsule.
    pub id: CapsuleId,
    /// Gene this capsule was derived from.
    pub gene_id: GeneId,
    /// Mutation run that produced this capsule.
    pub mutation_id: MutationId,
    /// Kernel run ID of the execution that produced this capsule.
    pub run_id: RunId,
    /// Content hash of the diff artifact (for integrity checks and deduplication).
    pub diff_hash: String,
    /// Confidence score in `[0.0, 1.0]`; decays over time.
    pub confidence: f32,
    /// Build-environment snapshot captured at capsule creation time.
    pub env: EnvFingerprint,
    /// Detailed outcome from the validation run.
    pub outcome: Outcome,
    /// Current lifecycle state of this capsule.
    #[serde(default)]
    pub state: AssetState,
}

/// A reusable solution template derived from one or more successful mutations.
///
/// A `Gene` captures the signal pattern, mutation strategy, and validation
/// commands for a class of recurring problems. The selector matches genes
/// against incoming signals to decide which capsule to replay.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Gene {
    /// Unique identifier for this gene.
    pub id: GeneId,
    /// Diagnostic signal patterns that this gene is designed to address.
    pub signals: Vec<String>,
    /// Ordered mutation strategy steps (e.g. instructions for the LLM mutator).
    pub strategy: Vec<String>,
    /// Validation commands to confirm the mutation succeeded.
    pub validation: Vec<String>,
    /// Current lifecycle state (promoted, revoked, etc.).
    #[serde(default)]
    pub state: AssetState,
    /// Optional task-class ID from `oris_evolution::task_class::TaskClass`.
    ///
    /// When set, the `TaskClassAwareSelector` can match this gene for any
    /// incoming signal list that classifies to the same task-class, enabling
    /// semantic-equivalent task reuse beyond exact signal matching.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_class_id: Option<String>,
}

/// Domain event for the evolution pipeline's append-only event store.
///
/// Every state change in the evolution lifecycle — from mutation declaration
/// through gene promotion or revocation — is recorded as one of these variants.
/// The projection is rebuilt by replaying the full event sequence.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EvolutionEvent {
    /// A new mutation has been declared and is ready for sandbox execution.
    MutationDeclared {
        /// The prepared mutation (intent + artifact) to execute.
        mutation: PreparedMutation,
    },
    /// The mutation's patch was successfully applied to the workspace.
    MutationApplied {
        /// ID of the mutation that was applied.
        mutation_id: MutationId,
        /// Content hash of the applied patch.
        patch_hash: String,
        /// Paths of files modified by the patch.
        changed_files: Vec<String>,
    },
    /// Diagnostic signals were extracted from the mutation context.
    SignalsExtracted {
        /// ID of the mutation the signals were extracted from.
        mutation_id: MutationId,
        /// Hash of the signal set (for deduplication).
        hash: String,
        /// Extracted signal strings.
        signals: Vec<String>,
    },
    /// The mutation was rejected (policy, validation, or blast-radius limit).
    MutationRejected {
        /// ID of the rejected mutation.
        mutation_id: MutationId,
        /// Human-readable rejection reason.
        reason: String,
        /// Machine-readable reason code.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason_code: Option<String>,
        /// Optional hint for the operator on how to recover.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        recovery_hint: Option<String>,
        /// If `true`, the pipeline was halted (no further candidates tried).
        #[serde(default)]
        fail_closed: bool,
    },
    /// Build and tests passed for the mutation.
    ValidationPassed {
        /// ID of the mutation that was validated.
        mutation_id: MutationId,
        /// Snapshot of the validation run results.
        report: ValidationSnapshot,
        /// Gene this validation is attributed to, if known.
        gene_id: Option<GeneId>,
    },
    /// Build or tests failed for the mutation.
    ValidationFailed {
        /// ID of the mutation that failed validation.
        mutation_id: MutationId,
        /// Snapshot of the validation run results.
        report: ValidationSnapshot,
        /// Gene this validation is attributed to, if known.
        gene_id: Option<GeneId>,
    },
    /// A new capsule was committed to the store.
    CapsuleCommitted {
        /// The newly committed capsule.
        capsule: Capsule,
    },
    /// A capsule was quarantined for investigation.
    CapsuleQuarantined {
        /// ID of the quarantined capsule.
        capsule_id: CapsuleId,
    },
    /// A capsule was released from quarantine or revoked.
    CapsuleReleased {
        /// ID of the capsule being released.
        capsule_id: CapsuleId,
        /// New state of the capsule after release.
        state: AssetState,
    },
    /// A capsule was replayed (reused) for a new run.
    CapsuleReused {
        /// ID of the replayed capsule.
        capsule_id: CapsuleId,
        /// Gene the capsule belongs to.
        gene_id: GeneId,
        /// Kernel run ID of the new run that triggered the replay.
        run_id: RunId,
        /// Kernel run ID of the replay execution, if separate.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        replay_run_id: Option<RunId>,
    },
    /// A gene was projected (created or updated) from the event stream.
    GeneProjected {
        /// The gene as projected at this point in the event stream.
        gene: Gene,
    },
    /// A gene was promoted to active status.
    GenePromoted {
        /// ID of the promoted gene.
        gene_id: GeneId,
    },
    /// A gene was revoked (withdrawn from active use).
    GeneRevoked {
        /// ID of the revoked gene.
        gene_id: GeneId,
        /// Human-readable reason for revocation.
        reason: String,
    },
    /// A gene was archived (retired, retained for audit).
    GeneArchived {
        /// ID of the archived gene.
        gene_id: GeneId,
    },
    /// The governor evaluated whether a gene should be promoted, downgraded, or held.
    PromotionEvaluated {
        /// ID of the gene being evaluated.
        gene_id: GeneId,
        /// New asset state resulting from the evaluation.
        state: AssetState,
        /// Human-readable evaluation summary.
        reason: String,
        /// Machine-readable reason code for the transition.
        #[serde(default)]
        reason_code: TransitionReasonCode,
        /// Optional evidence data that informed the decision.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        evidence: Option<TransitionEvidence>,
    },
    /// Replay economics (ROI) were recorded for a replay attempt.
    ReplayEconomicsRecorded {
        /// Gene associated with this replay, if known.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        gene_id: Option<GeneId>,
        /// Capsule that was replayed, if known.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        capsule_id: Option<CapsuleId>,
        /// Kernel run ID of the replay execution, if available.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        replay_run_id: Option<RunId>,
        /// Full ROI accounting record.
        evidence: ReplayRoiEvidence,
    },
    /// Assets were imported from a remote peer node.
    RemoteAssetImported {
        /// Whether the assets came from a local or remote source.
        source: CandidateSource,
        /// IDs of the imported assets.
        asset_ids: Vec<String>,
        /// Peer node ID that sent the assets, if known.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sender_id: Option<String>,
    },
    /// An evolution network manifest was validated (or rejected).
    ManifestValidated {
        /// `true` if the manifest was accepted.
        accepted: bool,
        /// Human-readable reason for the accept/reject decision.
        reason: String,
        /// Peer node that sent the manifest, if known.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sender_id: Option<String>,
        /// Publisher identity, if provided in the manifest.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        publisher: Option<String>,
        /// Asset IDs referenced in the manifest.
        #[serde(default)]
        asset_ids: Vec<String>,
    },
    /// A mutation was linked to an OUSL spec contract.
    SpecLinked {
        /// ID of the mutation.
        mutation_id: MutationId,
        /// OUSL spec contract ID that was satisfied.
        spec_id: String,
    },
    /// A delivery (PR) was prepared for a completed task.
    DeliveryPrepared {
        /// Orchestrator task ID this delivery belongs to.
        task_id: String,
        /// Git branch name created for the delivery.
        branch_name: String,
        /// Pull request title.
        pr_title: String,
        /// Pull request body summary.
        pr_summary: String,
        /// Human-readable delivery summary for the event log.
        delivery_summary: String,
        /// Current delivery status (e.g. `"pending"`, `"submitted"`).
        delivery_status: String,
        /// Current approval state (e.g. `"pending_review"`, `"approved"`).
        approval_state: String,
        /// Machine-readable reason code for this delivery state.
        reason_code: String,
    },
    /// An acceptance gate was evaluated for a delivered task.
    AcceptanceGateEvaluated {
        /// Orchestrator task ID.
        task_id: String,
        /// GitHub issue number associated with the task.
        issue_number: u64,
        /// Summary of the acceptance gate decision.
        acceptance_gate_summary: String,
        /// Result of the audit consistency check.
        audit_consistency_result: String,
        /// Evidence used for the approval decision.
        approval_evidence: String,
        /// Delivery outcome (e.g. `"accepted"`, `"rejected"`).
        delivery_outcome: String,
        /// JSON-serialized reason code matrix for audit.
        reason_code_matrix: String,
        /// If `true`, the pipeline was halted on failure.
        fail_closed: bool,
        /// Primary reason code for the gate outcome.
        reason_code: String,
    },
}

/// A hash-chained envelope wrapping an `EvolutionEvent` in the JSONL event log.
///
/// The `prev_hash` / `record_hash` chain detects out-of-order writes or tampering.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoredEvolutionEvent {
    /// Monotonically increasing sequence number (starts at 1).
    pub seq: u64,
    /// RFC 3339 timestamp of when the event was appended.
    pub timestamp: String,
    /// Hash of the previous record (empty string for the first event).
    pub prev_hash: String,
    /// SHA-256 hash of `(seq, timestamp, prev_hash, event)` for chain integrity.
    pub record_hash: String,
    /// The domain event payload.
    pub event: EvolutionEvent,
}

/// In-memory read projection rebuilt from the evolution event log.
///
/// Updated on every `append_event` call and also written to disk as `genes.json`
/// and `capsules.json` for fast cold-start recovery.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EvolutionProjection {
    /// All known genes (promoted and otherwise).
    pub genes: Vec<Gene>,
    /// All known capsules.
    pub capsules: Vec<Capsule>,
    /// Number of times each gene's capsule has been successfully replayed.
    pub reuse_counts: BTreeMap<GeneId, u64>,
    /// Total mutation attempts attributed to each gene.
    pub attempt_counts: BTreeMap<GeneId, u64>,
    /// Timestamp of the most recent event touching each gene.
    pub last_updated_at: BTreeMap<GeneId, String>,
    /// OUSL spec IDs associated with each gene.
    pub spec_ids_by_gene: BTreeMap<GeneId, BTreeSet<String>>,
}

/// Input to the gene selector for a single selection query.
#[derive(Clone, Debug)]
pub struct SelectorInput {
    /// Diagnostic signals extracted from the current issue.
    pub signals: Vec<String>,
    /// Build environment to use for environment-match scoring.
    pub env: EnvFingerprint,
    /// Optional OUSL spec ID to restrict candidates.
    pub spec_id: Option<String>,
    /// Maximum number of candidates to return.
    pub limit: usize,
}

/// A gene candidate returned by a `Selector`, with its score and associated capsules.
#[derive(Clone, Debug)]
pub struct GeneCandidate {
    /// The matched gene.
    pub gene: Gene,
    /// Relevance score in `[0.0, 1.0]`; higher is a better match.
    pub score: f32,
    /// Capsules belonging to this gene, sorted by confidence descending.
    pub capsules: Vec<Capsule>,
}

/// Selects candidate genes from the evolution store for a given set of signals.
pub trait Selector: Send + Sync {
    /// Returns up to `input.limit` candidate genes ranked by relevance score.
    fn select(&self, input: &SelectorInput) -> Vec<GeneCandidate>;
}

/// Append-only evolution event store with projection and scan support.
pub trait EvolutionStore: Send + Sync {
    /// Appends one event and returns the assigned sequence number.
    fn append_event(&self, event: EvolutionEvent) -> Result<u64, EvolutionError>;
    /// Returns all events with `seq >= from_seq`, in ascending order.
    fn scan(&self, from_seq: u64) -> Result<Vec<StoredEvolutionEvent>, EvolutionError>;
    /// Rebuilds and returns the current projection from the full event log.
    fn rebuild_projection(&self) -> Result<EvolutionProjection, EvolutionError>;

    /// Returns both the full event log and the current projection in a single locked read.
    fn scan_projection(
        &self,
    ) -> Result<(Vec<StoredEvolutionEvent>, EvolutionProjection), EvolutionError> {
        let events = self.scan(1)?;
        let projection = rebuild_projection_from_events(&events);
        Ok((events, projection))
    }
}

/// Errors returned by the evolution store and related operations.
#[derive(Debug, Error)]
pub enum EvolutionError {
    /// An I/O error reading or writing the event log or projection files.
    #[error("I/O error: {0}")]
    Io(String),
    /// A serialization or deserialization error.
    #[error("Serialization error: {0}")]
    Serde(String),
    /// The hash chain integrity check failed (possible data corruption or tamper).
    #[error("Hash chain validation failed: {0}")]
    HashChain(String),
}

/// JSONL-backed implementation of `EvolutionStore` with a hash-chained event log.
///
/// Events are appended to `{root_dir}/events.jsonl`; the projection is persisted
/// to `genes.json` and `capsules.json` after every append for fast cold-start recovery.
pub struct JsonlEvolutionStore {
    root_dir: PathBuf,
    lock: Mutex<()>,
}

impl JsonlEvolutionStore {
    /// Creates a new store rooted at `root_dir`. The directory and required files
    /// are created on first use.
    pub fn new<P: Into<PathBuf>>(root_dir: P) -> Self {
        Self {
            root_dir: root_dir.into(),
            lock: Mutex::new(()),
        }
    }

    /// Returns the root directory of this store.
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

/// `Selector` implementation that ranks genes from an in-memory `EvolutionProjection`.
///
/// Scoring factors: signal overlap, environment match, capsule confidence (with time decay),
/// and reuse-to-attempt ratio. Only `Promoted` genes with at least one `Promoted` capsule
/// are eligible.
pub struct ProjectionSelector {
    projection: EvolutionProjection,
    now: DateTime<Utc>,
}

impl ProjectionSelector {
    /// Creates a selector using the current wall-clock time for confidence decay.
    pub fn new(projection: EvolutionProjection) -> Self {
        Self {
            projection,
            now: Utc::now(),
        }
    }

    /// Creates a selector with an explicit `now` timestamp (useful for deterministic tests).
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

/// `Selector` that reads the projection live from an `EvolutionStore` on every call.
///
/// Use this when you need the most up-to-date projection without pre-loading it.
/// For performance-critical paths, prefer `ProjectionSelector` with a pre-loaded snapshot.
pub struct StoreBackedSelector {
    store: std::sync::Arc<dyn EvolutionStore>,
}

impl StoreBackedSelector {
    /// Creates a selector backed by the given store.
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

/// Rebuilds an `EvolutionProjection` by replaying the given event sequence from the beginning.
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

/// Returns the default root directory for the evolution store (`.oris/evolution`).
pub fn default_store_root() -> PathBuf {
    PathBuf::from(".oris").join("evolution")
}

/// Computes the SHA-256 hex digest of a UTF-8 string.
pub fn hash_string(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    hex::encode(hasher.finalize())
}

/// Serializes `value` to canonical JSON and returns its SHA-256 hex digest.
pub fn stable_hash_json<T: Serialize>(value: &T) -> Result<String, EvolutionError> {
    let bytes = serde_json::to_vec(value).map_err(|err| EvolutionError::Serde(err.to_string()))?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    Ok(hex::encode(hasher.finalize()))
}

/// Computes the SHA-256 content hash of an artifact payload string.
pub fn compute_artifact_hash(payload: &str) -> String {
    hash_string(payload)
}

/// Generates a time-based unique ID with the given prefix (e.g. `"gene-1a2b3c"`).
pub fn next_id(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{prefix}-{nanos:x}")
}

/// Applies exponential time-decay to a capsule confidence score.
///
/// Returns the decayed confidence clamped to `[0.0, 1.0]`.
/// `age_secs` is the time since the capsule was created; `None` is treated as 0.
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
        .filter_map(canonical_signal_token)
        .collect::<BTreeSet<_>>();
    if tokens.is_empty() {
        return None;
    }
    let phrase = tokens.iter().cloned().collect::<Vec<_>>().join(" ");
    Some(CanonicalSignal { phrase, tokens })
}

fn canonical_signal_token(token: &str) -> Option<String> {
    let normalized = token.trim().to_ascii_lowercase();
    if normalized.len() < 3 {
        return None;
    }
    if normalized.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    match normalized.as_str() {
        "absent" | "unavailable" | "vanished" => Some("missing".into()),
        "file" | "files" | "error" | "errors" => None,
        _ => Some(normalized),
    }
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
    fn evomap_asset_state_mapping_archived_is_rejected() {
        assert_eq!(
            asset_state_to_evomap_compat(&AssetState::Archived),
            "rejected"
        );
        assert_eq!(
            asset_state_to_evomap_compat(&AssetState::Quarantined),
            "quarantined"
        );
        assert_eq!(
            asset_state_to_evomap_compat(&AssetState::ShadowValidated),
            "candidate"
        );
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
                reason_code: None,
                recovery_hint: None,
                fail_closed: true,
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
            task_class_id: None,
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
            task_class_id: None,
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
            task_class_id: None,
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
            task_class_id: None,
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
            task_class_id: None,
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
            task_class_id: None,
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
                    task_class_id: None,
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
            task_class_id: None,
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
                    task_class_id: None,
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
                    task_class_id: None,
                },
                Gene {
                    id: "gene-b".into(),
                    signals: vec!["signal".into()],
                    strategy: vec!["b".into()],
                    validation: vec!["oris-default".into()],
                    state: AssetState::Promoted,
                    task_class_id: None,
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
                    task_class_id: None,
                },
                Gene {
                    id: "gene-b".into(),
                    signals: vec!["signal".into()],
                    strategy: vec!["b".into()],
                    validation: vec!["oris-default".into()],
                    state: AssetState::Promoted,
                    task_class_id: None,
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
                    task_class_id: None,
                },
                Gene {
                    id: "gene-b".into(),
                    signals: vec!["signal".into()],
                    strategy: vec!["b".into()],
                    validation: vec!["oris-default".into()],
                    state: AssetState::Promoted,
                    task_class_id: None,
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
                task_class_id: None,
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
                task_class_id: None,
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

    #[test]
    fn normalized_signal_overlap_accepts_semantic_multisignal_variants() {
        let overlap = normalized_signal_overlap(
            &["missing readme".into(), "route beijing shanghai".into()],
            &[
                "README file absent".into(),
                "travel route beijing shanghai".into(),
            ],
        );

        assert!(overlap >= 0.99, "expected strong overlap, got {overlap}");
    }

    #[test]
    fn normalized_signal_overlap_rejects_single_shared_token_false_positives() {
        let overlap =
            normalized_signal_overlap(&["missing readme".into()], &["missing cargo".into()]);

        assert_eq!(overlap, 0.0);
    }
}
