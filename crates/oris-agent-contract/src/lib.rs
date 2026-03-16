//! Proposal-only runtime contract for external agents.

use serde::{Deserialize, Serialize};

pub const A2A_PROTOCOL_NAME: &str = "oris.a2a";
pub const A2A_PROTOCOL_VERSION: &str = "0.1.0-experimental";
pub const A2A_PROTOCOL_VERSION_V1: &str = "1.0.0";
pub const A2A_SUPPORTED_PROTOCOL_VERSIONS: [&str; 2] =
    [A2A_PROTOCOL_VERSION_V1, A2A_PROTOCOL_VERSION];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct A2aProtocol {
    pub name: String,
    pub version: String,
}

impl A2aProtocol {
    pub fn current() -> Self {
        Self {
            name: A2A_PROTOCOL_NAME.to_string(),
            version: A2A_PROTOCOL_VERSION.to_string(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum A2aCapability {
    Coordination,
    MutationProposal,
    ReplayFeedback,
    SupervisedDevloop,
    EvolutionPublish,
    EvolutionFetch,
    EvolutionRevoke,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct A2aHandshakeRequest {
    pub agent_id: String,
    pub role: AgentRole,
    pub capability_level: AgentCapabilityLevel,
    pub supported_protocols: Vec<A2aProtocol>,
    pub advertised_capabilities: Vec<A2aCapability>,
}

impl A2aHandshakeRequest {
    pub fn supports_protocol_version(&self, version: &str) -> bool {
        self.supported_protocols
            .iter()
            .any(|protocol| protocol.name == A2A_PROTOCOL_NAME && protocol.version == version)
    }

    pub fn supports_current_protocol(&self) -> bool {
        self.supports_protocol_version(A2A_PROTOCOL_VERSION)
    }

    pub fn negotiate_supported_protocol(&self) -> Option<A2aProtocol> {
        for version in A2A_SUPPORTED_PROTOCOL_VERSIONS {
            if self.supports_protocol_version(version) {
                return Some(A2aProtocol {
                    name: A2A_PROTOCOL_NAME.to_string(),
                    version: version.to_string(),
                });
            }
        }
        None
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct A2aHandshakeResponse {
    pub accepted: bool,
    pub negotiated_protocol: Option<A2aProtocol>,
    pub enabled_capabilities: Vec<A2aCapability>,
    pub message: Option<String>,
    pub error: Option<A2aErrorEnvelope>,
}

impl A2aHandshakeResponse {
    pub fn accept(enabled_capabilities: Vec<A2aCapability>) -> Self {
        Self {
            accepted: true,
            negotiated_protocol: Some(A2aProtocol::current()),
            enabled_capabilities,
            message: Some("handshake accepted".to_string()),
            error: None,
        }
    }

    pub fn reject(code: A2aErrorCode, message: impl Into<String>, details: Option<String>) -> Self {
        Self {
            accepted: false,
            negotiated_protocol: None,
            enabled_capabilities: Vec::new(),
            message: Some("handshake rejected".to_string()),
            error: Some(A2aErrorEnvelope {
                code,
                message: message.into(),
                retriable: true,
                details,
            }),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum A2aTaskLifecycleState {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct A2aTaskLifecycleEvent {
    pub task_id: String,
    pub state: A2aTaskLifecycleState,
    pub summary: String,
    pub updated_at_ms: u64,
    pub error: Option<A2aErrorEnvelope>,
}

pub const A2A_TASK_SESSION_PROTOCOL_VERSION: &str = A2A_PROTOCOL_VERSION;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum A2aTaskSessionState {
    Started,
    Dispatched,
    InProgress,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct A2aTaskSessionStartRequest {
    pub sender_id: String,
    pub protocol_version: String,
    pub task_id: String,
    pub task_summary: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct A2aTaskSessionDispatchRequest {
    pub sender_id: String,
    pub protocol_version: String,
    pub dispatch_id: String,
    pub summary: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct A2aTaskSessionProgressRequest {
    pub sender_id: String,
    pub protocol_version: String,
    pub progress_pct: u8,
    pub summary: String,
    pub retryable: bool,
    pub retry_after_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct A2aTaskSessionCompletionRequest {
    pub sender_id: String,
    pub protocol_version: String,
    pub terminal_state: A2aTaskLifecycleState,
    pub summary: String,
    pub retryable: bool,
    pub retry_after_ms: Option<u64>,
    pub failure_code: Option<A2aErrorCode>,
    pub failure_details: Option<String>,
    pub used_capsule: bool,
    pub capsule_id: Option<String>,
    pub reasoning_steps_avoided: u64,
    pub fallback_reason: Option<String>,
    pub reason_code: Option<ReplayFallbackReasonCode>,
    pub repair_hint: Option<String>,
    pub next_action: Option<ReplayFallbackNextAction>,
    pub confidence: Option<u8>,
    pub task_class_id: String,
    pub task_label: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct A2aTaskSessionProgressItem {
    pub progress_pct: u8,
    pub summary: String,
    pub retryable: bool,
    pub retry_after_ms: Option<u64>,
    pub updated_at_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct A2aTaskSessionAck {
    pub session_id: String,
    pub task_id: String,
    pub state: A2aTaskSessionState,
    pub summary: String,
    pub retryable: bool,
    pub retry_after_ms: Option<u64>,
    pub updated_at_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct A2aTaskSessionResult {
    pub terminal_state: A2aTaskLifecycleState,
    pub summary: String,
    pub retryable: bool,
    pub retry_after_ms: Option<u64>,
    pub failure_code: Option<A2aErrorCode>,
    pub failure_details: Option<String>,
    pub replay_feedback: ReplayFeedback,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct A2aTaskSessionCompletionResponse {
    pub ack: A2aTaskSessionAck,
    pub result: A2aTaskSessionResult,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct A2aTaskSessionSnapshot {
    pub session_id: String,
    pub sender_id: String,
    pub task_id: String,
    pub protocol_version: String,
    pub state: A2aTaskSessionState,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub dispatch_ids: Vec<String>,
    pub progress: Vec<A2aTaskSessionProgressItem>,
    pub result: Option<A2aTaskSessionResult>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum A2aErrorCode {
    UnsupportedProtocol,
    UnsupportedCapability,
    ValidationFailed,
    AuthorizationDenied,
    Timeout,
    Internal,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct A2aErrorEnvelope {
    pub code: A2aErrorCode,
    pub message: String,
    pub retriable: bool,
    pub details: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum AgentCapabilityLevel {
    A0,
    A1,
    A2,
    A3,
    A4,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ProposalTarget {
    WorkspaceRoot,
    Paths(Vec<String>),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentTask {
    pub id: String,
    pub description: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum AgentRole {
    Planner,
    Coder,
    Repair,
    Optimizer,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum CoordinationPrimitive {
    Sequential,
    Parallel,
    Conditional,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoordinationTask {
    pub id: String,
    pub role: AgentRole,
    pub description: String,
    pub depends_on: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoordinationMessage {
    pub from_role: AgentRole,
    pub to_role: AgentRole,
    pub task_id: String,
    pub content: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoordinationPlan {
    pub root_goal: String,
    pub primitive: CoordinationPrimitive,
    pub tasks: Vec<CoordinationTask>,
    pub timeout_ms: u64,
    pub max_retries: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoordinationResult {
    pub completed_tasks: Vec<String>,
    pub failed_tasks: Vec<String>,
    pub messages: Vec<CoordinationMessage>,
    pub summary: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MutationProposal {
    pub intent: String,
    pub files: Vec<String>,
    pub expected_effect: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MutationProposalContractReasonCode {
    Accepted,
    CandidateRejected,
    MissingTargetFiles,
    OutOfBoundsPath,
    UnsupportedTaskClass,
    ValidationBudgetExceeded,
    ExpectedEvidenceMissing,
    UnknownFailClosed,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MutationProposalEvidence {
    HumanApproval,
    BoundedScope,
    ValidationPass,
    ExecutionAudit,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MutationProposalValidationBudget {
    pub max_diff_bytes: usize,
    pub max_changed_lines: usize,
    pub validation_timeout_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionFeedback {
    pub accepted: bool,
    pub asset_state: Option<String>,
    pub summary: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ReplayPlannerDirective {
    SkipPlanner,
    PlanFallback,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReplayFallbackReasonCode {
    NoCandidateAfterSelect,
    ScoreBelowThreshold,
    CandidateHasNoCapsule,
    MutationPayloadMissing,
    PatchApplyFailed,
    ValidationFailed,
    UnmappedFallbackReason,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReplayFallbackNextAction {
    PlanFromScratch,
    ValidateSignalsThenPlan,
    RebuildCapsule,
    RegenerateMutationPayload,
    RebasePatchAndRetry,
    RepairAndRevalidate,
    EscalateFailClosed,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplayFallbackContract {
    pub reason_code: ReplayFallbackReasonCode,
    pub fallback_reason: String,
    pub repair_hint: String,
    pub next_action: ReplayFallbackNextAction,
    /// Confidence score in [0, 100].
    pub confidence: u8,
}

pub fn infer_replay_fallback_reason_code(reason: &str) -> Option<ReplayFallbackReasonCode> {
    let normalized = reason.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }
    if normalized == "no_candidate_after_select" || normalized.contains("no matching gene") {
        return Some(ReplayFallbackReasonCode::NoCandidateAfterSelect);
    }
    if normalized == "score_below_threshold" || normalized.contains("below replay threshold") {
        return Some(ReplayFallbackReasonCode::ScoreBelowThreshold);
    }
    if normalized == "candidate_has_no_capsule" || normalized.contains("has no capsule") {
        return Some(ReplayFallbackReasonCode::CandidateHasNoCapsule);
    }
    if normalized == "mutation_payload_missing" || normalized.contains("payload missing") {
        return Some(ReplayFallbackReasonCode::MutationPayloadMissing);
    }
    if normalized == "patch_apply_failed" || normalized.contains("patch apply failed") {
        return Some(ReplayFallbackReasonCode::PatchApplyFailed);
    }
    if normalized == "validation_failed" || normalized.contains("validation failed") {
        return Some(ReplayFallbackReasonCode::ValidationFailed);
    }
    None
}

pub fn normalize_replay_fallback_contract(
    planner_directive: &ReplayPlannerDirective,
    fallback_reason: Option<&str>,
    reason_code: Option<ReplayFallbackReasonCode>,
    repair_hint: Option<&str>,
    next_action: Option<ReplayFallbackNextAction>,
    confidence: Option<u8>,
) -> Option<ReplayFallbackContract> {
    if !matches!(planner_directive, ReplayPlannerDirective::PlanFallback) {
        return None;
    }

    let normalized_reason = normalize_optional_text(fallback_reason);
    let normalized_repair_hint = normalize_optional_text(repair_hint);
    let mut resolved_reason_code = reason_code
        .or_else(|| {
            normalized_reason
                .as_deref()
                .and_then(infer_replay_fallback_reason_code)
        })
        .unwrap_or(ReplayFallbackReasonCode::UnmappedFallbackReason);
    let mut defaults = replay_fallback_defaults(&resolved_reason_code);

    let mut force_fail_closed = false;
    if let Some(provided_action) = next_action {
        if provided_action != defaults.next_action {
            resolved_reason_code = ReplayFallbackReasonCode::UnmappedFallbackReason;
            defaults = replay_fallback_defaults(&resolved_reason_code);
            force_fail_closed = true;
        }
    }

    Some(ReplayFallbackContract {
        reason_code: resolved_reason_code,
        fallback_reason: normalized_reason.unwrap_or_else(|| defaults.fallback_reason.to_string()),
        repair_hint: normalized_repair_hint.unwrap_or_else(|| defaults.repair_hint.to_string()),
        next_action: if force_fail_closed {
            defaults.next_action
        } else {
            next_action.unwrap_or(defaults.next_action)
        },
        confidence: if force_fail_closed {
            defaults.confidence
        } else {
            confidence.unwrap_or(defaults.confidence).min(100)
        },
    })
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplayFeedback {
    pub used_capsule: bool,
    pub capsule_id: Option<String>,
    pub planner_directive: ReplayPlannerDirective,
    pub reasoning_steps_avoided: u64,
    pub fallback_reason: Option<String>,
    pub reason_code: Option<ReplayFallbackReasonCode>,
    pub repair_hint: Option<String>,
    pub next_action: Option<ReplayFallbackNextAction>,
    pub confidence: Option<u8>,
    pub task_class_id: String,
    pub task_label: String,
    pub summary: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MutationNeededFailureReasonCode {
    PolicyDenied,
    ValidationFailed,
    UnsafePatch,
    Timeout,
    MutationPayloadMissing,
    UnknownFailClosed,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MutationNeededRecoveryAction {
    NarrowScopeAndRetry,
    RepairAndRevalidate,
    ProduceSafePatch,
    ReduceExecutionBudget,
    RegenerateMutationPayload,
    EscalateFailClosed,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MutationNeededFailureContract {
    pub reason_code: MutationNeededFailureReasonCode,
    pub failure_reason: String,
    pub recovery_hint: String,
    pub recovery_action: MutationNeededRecoveryAction,
    pub fail_closed: bool,
}

pub fn infer_mutation_needed_failure_reason_code(
    reason: &str,
) -> Option<MutationNeededFailureReasonCode> {
    let normalized = reason.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }
    if normalized.contains("mutation payload missing") || normalized == "mutation_payload_missing" {
        return Some(MutationNeededFailureReasonCode::MutationPayloadMissing);
    }
    if normalized.contains("command timed out") || normalized.contains(" timeout") {
        return Some(MutationNeededFailureReasonCode::Timeout);
    }
    if normalized.contains("patch rejected")
        || normalized.contains("patch apply failed")
        || normalized.contains("target violation")
        || normalized.contains("unsafe patch")
    {
        return Some(MutationNeededFailureReasonCode::UnsafePatch);
    }
    if normalized.contains("validation failed") {
        return Some(MutationNeededFailureReasonCode::ValidationFailed);
    }
    if normalized.contains("command denied by policy")
        || normalized.contains("rejected task")
        || normalized.contains("unsupported task outside the bounded scope")
        || normalized.contains("budget exceeds bounded policy")
    {
        return Some(MutationNeededFailureReasonCode::PolicyDenied);
    }
    None
}

pub fn normalize_mutation_needed_failure_contract(
    failure_reason: Option<&str>,
    reason_code: Option<MutationNeededFailureReasonCode>,
) -> MutationNeededFailureContract {
    let normalized_reason = normalize_optional_text(failure_reason);
    let resolved_reason_code = reason_code
        .or_else(|| {
            normalized_reason
                .as_deref()
                .and_then(infer_mutation_needed_failure_reason_code)
        })
        .unwrap_or(MutationNeededFailureReasonCode::UnknownFailClosed);
    let defaults = mutation_needed_failure_defaults(&resolved_reason_code);

    MutationNeededFailureContract {
        reason_code: resolved_reason_code,
        failure_reason: normalized_reason.unwrap_or_else(|| defaults.failure_reason.to_string()),
        recovery_hint: defaults.recovery_hint.to_string(),
        recovery_action: defaults.recovery_action,
        fail_closed: true,
    }
}

fn normalize_optional_text(value: Option<&str>) -> Option<String> {
    let trimmed = value?.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[derive(Clone, Copy)]
struct ReplayFallbackDefaults {
    fallback_reason: &'static str,
    repair_hint: &'static str,
    next_action: ReplayFallbackNextAction,
    confidence: u8,
}

fn replay_fallback_defaults(reason_code: &ReplayFallbackReasonCode) -> ReplayFallbackDefaults {
    match reason_code {
        ReplayFallbackReasonCode::NoCandidateAfterSelect => ReplayFallbackDefaults {
            fallback_reason: "no matching gene",
            repair_hint:
                "No reusable capsule matched deterministic signals; run planner for a minimal patch.",
            next_action: ReplayFallbackNextAction::PlanFromScratch,
            confidence: 92,
        },
        ReplayFallbackReasonCode::ScoreBelowThreshold => ReplayFallbackDefaults {
            fallback_reason: "candidate score below replay threshold",
            repair_hint:
                "Best replay candidate is below threshold; validate task signals and re-plan.",
            next_action: ReplayFallbackNextAction::ValidateSignalsThenPlan,
            confidence: 86,
        },
        ReplayFallbackReasonCode::CandidateHasNoCapsule => ReplayFallbackDefaults {
            fallback_reason: "candidate gene has no capsule",
            repair_hint: "Matched gene has no executable capsule; rebuild capsule from planner output.",
            next_action: ReplayFallbackNextAction::RebuildCapsule,
            confidence: 80,
        },
        ReplayFallbackReasonCode::MutationPayloadMissing => ReplayFallbackDefaults {
            fallback_reason: "mutation payload missing from store",
            repair_hint:
                "Mutation payload is missing; regenerate and persist a minimal mutation payload.",
            next_action: ReplayFallbackNextAction::RegenerateMutationPayload,
            confidence: 76,
        },
        ReplayFallbackReasonCode::PatchApplyFailed => ReplayFallbackDefaults {
            fallback_reason: "replay patch apply failed",
            repair_hint: "Replay patch cannot be applied cleanly; rebase patch and retry planning.",
            next_action: ReplayFallbackNextAction::RebasePatchAndRetry,
            confidence: 68,
        },
        ReplayFallbackReasonCode::ValidationFailed => ReplayFallbackDefaults {
            fallback_reason: "replay validation failed",
            repair_hint: "Replay validation failed; produce a repair mutation and re-run validation.",
            next_action: ReplayFallbackNextAction::RepairAndRevalidate,
            confidence: 64,
        },
        ReplayFallbackReasonCode::UnmappedFallbackReason => ReplayFallbackDefaults {
            fallback_reason: "unmapped replay fallback reason",
            repair_hint:
                "Fallback reason is unmapped; fail closed and require explicit planner intervention.",
            next_action: ReplayFallbackNextAction::EscalateFailClosed,
            confidence: 0,
        },
    }
}

#[derive(Clone, Copy)]
struct MutationNeededFailureDefaults {
    failure_reason: &'static str,
    recovery_hint: &'static str,
    recovery_action: MutationNeededRecoveryAction,
}

fn mutation_needed_failure_defaults(
    reason_code: &MutationNeededFailureReasonCode,
) -> MutationNeededFailureDefaults {
    match reason_code {
        MutationNeededFailureReasonCode::PolicyDenied => MutationNeededFailureDefaults {
            failure_reason: "mutation needed denied by bounded execution policy",
            recovery_hint:
                "Narrow changed scope to the approved docs boundary and re-run with explicit approval.",
            recovery_action: MutationNeededRecoveryAction::NarrowScopeAndRetry,
        },
        MutationNeededFailureReasonCode::ValidationFailed => MutationNeededFailureDefaults {
            failure_reason: "mutation needed validation failed",
            recovery_hint:
                "Repair mutation and re-run validation to produce a deterministic pass before capture.",
            recovery_action: MutationNeededRecoveryAction::RepairAndRevalidate,
        },
        MutationNeededFailureReasonCode::UnsafePatch => MutationNeededFailureDefaults {
            failure_reason: "mutation needed rejected unsafe patch",
            recovery_hint:
                "Generate a safer minimal diff confined to approved paths and verify patch applicability.",
            recovery_action: MutationNeededRecoveryAction::ProduceSafePatch,
        },
        MutationNeededFailureReasonCode::Timeout => MutationNeededFailureDefaults {
            failure_reason: "mutation needed execution timed out",
            recovery_hint:
                "Reduce execution budget or split the mutation into smaller steps before retrying.",
            recovery_action: MutationNeededRecoveryAction::ReduceExecutionBudget,
        },
        MutationNeededFailureReasonCode::MutationPayloadMissing => MutationNeededFailureDefaults {
            failure_reason: "mutation payload missing from store",
            recovery_hint: "Regenerate and persist mutation payload before retrying mutation-needed.",
            recovery_action: MutationNeededRecoveryAction::RegenerateMutationPayload,
        },
        MutationNeededFailureReasonCode::UnknownFailClosed => MutationNeededFailureDefaults {
            failure_reason: "mutation needed failed with unmapped reason",
            recovery_hint:
                "Unknown failure class; fail closed and require explicit maintainer triage before retry.",
            recovery_action: MutationNeededRecoveryAction::EscalateFailClosed,
        },
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum BoundedTaskClass {
    DocsSingleFile,
    DocsMultiFile,
    /// Dependency version bump: restricted to `Cargo.toml` / `Cargo.lock`
    /// paths, version fields only, max 5 manifests.
    CargoDepUpgrade,
    /// Lint / formatting fix: auto-fixable `cargo fmt` or `cargo clippy --fix`
    /// changes, no logic modifications, max 5 source files.
    LintFix,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MutationProposalScope {
    pub task_class: BoundedTaskClass,
    pub target_files: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SelfEvolutionMutationProposalContract {
    pub mutation_proposal: MutationProposal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposal_scope: Option<MutationProposalScope>,
    pub validation_budget: MutationProposalValidationBudget,
    pub approval_required: bool,
    pub expected_evidence: Vec<MutationProposalEvidence>,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery_hint: Option<String>,
    pub reason_code: MutationProposalContractReasonCode,
    pub fail_closed: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SelfEvolutionCandidateIntakeRequest {
    pub issue_number: u64,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub labels: Vec<String>,
    pub state: String,
    #[serde(default)]
    pub candidate_hint_paths: Vec<String>,
}

/// Signal source for an autonomously discovered candidate.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum AutonomousCandidateSource {
    CiFailure,
    TestRegression,
    CompileRegression,
    LintRegression,
    RuntimeIncident,
}

/// Reason code for the outcome of autonomous candidate classification.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutonomousIntakeReasonCode {
    Accepted,
    UnsupportedSignalClass,
    AmbiguousSignal,
    DuplicateCandidate,
    UnknownFailClosed,
}

/// A candidate discovered autonomously from CI or runtime signals without
/// a caller-supplied issue number.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveredCandidate {
    /// Stable identity hash, deterministic for the same raw signals.
    pub dedupe_key: String,
    /// Classified signal source.
    pub candidate_source: AutonomousCandidateSource,
    /// Normalised candidate class (reuses `BoundedTaskClass`).
    pub candidate_class: Option<BoundedTaskClass>,
    /// Normalised signal tokens used as the discovered work description.
    pub signals: Vec<String>,
    /// Whether this candidate was accepted for further work.
    pub accepted: bool,
    /// Outcome reason code.
    pub reason_code: AutonomousIntakeReasonCode,
    /// Human-readable summary.
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery_hint: Option<String>,
    /// Fail-closed flag: true on any non-accepted outcome.
    pub fail_closed: bool,
}

/// Input for autonomous candidate discovery from raw diagnostic output.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutonomousIntakeInput {
    /// Raw source identifier (e.g. CI run ID, log stream name).
    pub source_id: String,
    /// Classified origin of the raw signals.
    pub candidate_source: AutonomousCandidateSource,
    /// Raw text lines from diagnostics, test output, or incident logs.
    pub raw_signals: Vec<String>,
}

/// Output of autonomous candidate intake: one or more discovered candidates
/// (deduplicated) plus any that were denied with fail-closed reason codes.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutonomousIntakeOutput {
    pub candidates: Vec<DiscoveredCandidate>,
    pub accepted_count: usize,
    pub denied_count: usize,
}

// ── AUTO-02: Bounded Task Planning and Risk Scoring ──────────────────────────

/// Risk tier assigned to an autonomous task plan.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum AutonomousRiskTier {
    /// Minimal blast radius, fully reversible, single-file scope.
    Low,
    /// Multi-file scope or non-trivial dependency changes.
    Medium,
    /// High blast radius, wide impact, or unknown effect on public API.
    High,
}

/// Reason code for the outcome of autonomous task planning.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutonomousPlanReasonCode {
    Approved,
    DeniedHighRisk,
    DeniedLowFeasibility,
    DeniedUnsupportedClass,
    DeniedNoEvidence,
    UnknownFailClosed,
}

/// A denial condition attached to a rejected autonomous task plan.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutonomousDenialCondition {
    pub reason_code: AutonomousPlanReasonCode,
    pub description: String,
    pub recovery_hint: String,
}

/// An approved or denied autonomous task plan produced from a discovered candidate.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutonomousTaskPlan {
    /// Stable identity derived from the originating `DiscoveredCandidate.dedupe_key`.
    pub plan_id: String,
    /// The input candidate this plan was derived from.
    pub dedupe_key: String,
    /// Normalised task class (same as candidate class when approved).
    pub task_class: Option<BoundedTaskClass>,
    /// Assigned risk tier.
    pub risk_tier: AutonomousRiskTier,
    /// Feasibility score in [0, 100]; 0 means not feasible.
    pub feasibility_score: u8,
    /// Estimated validation budget (number of validation stages required).
    pub validation_budget: u8,
    /// Evidence templates required for this plan class.
    pub expected_evidence: Vec<String>,
    /// Whether the plan was approved for proposal generation.
    pub approved: bool,
    /// Planning outcome reason code.
    pub reason_code: AutonomousPlanReasonCode,
    /// Short human-readable summary of the planning outcome.
    pub summary: String,
    /// Present when the plan was denied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub denial_condition: Option<AutonomousDenialCondition>,
    /// Fail-closed flag: true on any non-approved outcome.
    pub fail_closed: bool,
}

pub fn approve_autonomous_task_plan(
    plan_id: impl Into<String>,
    dedupe_key: impl Into<String>,
    task_class: BoundedTaskClass,
    risk_tier: AutonomousRiskTier,
    feasibility_score: u8,
    validation_budget: u8,
    expected_evidence: Vec<String>,
    summary: Option<&str>,
) -> AutonomousTaskPlan {
    let summary = normalize_optional_text(summary).unwrap_or_else(|| {
        format!("autonomous task plan approved for {task_class:?} at {risk_tier:?} risk")
    });
    AutonomousTaskPlan {
        plan_id: plan_id.into(),
        dedupe_key: dedupe_key.into(),
        task_class: Some(task_class),
        risk_tier,
        feasibility_score,
        validation_budget,
        expected_evidence,
        approved: true,
        reason_code: AutonomousPlanReasonCode::Approved,
        summary,
        denial_condition: None,
        fail_closed: false,
    }
}

pub fn deny_autonomous_task_plan(
    plan_id: impl Into<String>,
    dedupe_key: impl Into<String>,
    risk_tier: AutonomousRiskTier,
    reason_code: AutonomousPlanReasonCode,
) -> AutonomousTaskPlan {
    let (description, recovery_hint) = match reason_code {
        AutonomousPlanReasonCode::DeniedHighRisk => (
            "task plan denied because risk tier is too high for autonomous execution",
            "reduce blast radius by scoping the change to a single bounded file before retrying",
        ),
        AutonomousPlanReasonCode::DeniedLowFeasibility => (
            "task plan denied because feasibility score is below the policy threshold",
            "provide stronger evidence or narrow the task scope before retrying",
        ),
        AutonomousPlanReasonCode::DeniedUnsupportedClass => (
            "task plan denied because task class is not supported for autonomous planning",
            "route this task class through the supervised planning path instead",
        ),
        AutonomousPlanReasonCode::DeniedNoEvidence => (
            "task plan denied because no evidence was available to assess feasibility",
            "ensure signals and candidate class are populated before planning",
        ),
        AutonomousPlanReasonCode::UnknownFailClosed => (
            "task plan failed with an unmapped reason; fail closed",
            "require explicit maintainer triage before retry",
        ),
        AutonomousPlanReasonCode::Approved => (
            "unexpected approved reason on deny path",
            "use approve_autonomous_task_plan for approved outcomes",
        ),
    };
    let summary = format!("autonomous task plan denied [{reason_code:?}]: {description}");
    AutonomousTaskPlan {
        plan_id: plan_id.into(),
        dedupe_key: dedupe_key.into(),
        task_class: None,
        risk_tier,
        feasibility_score: 0,
        validation_budget: 0,
        expected_evidence: Vec::new(),
        approved: false,
        reason_code,
        summary,
        denial_condition: Some(AutonomousDenialCondition {
            reason_code,
            description: description.to_string(),
            recovery_hint: recovery_hint.to_string(),
        }),
        fail_closed: true,
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SelfEvolutionSelectionReasonCode {
    Accepted,
    IssueClosed,
    MissingEvolutionLabel,
    MissingFeatureLabel,
    ExcludedByLabel,
    UnsupportedCandidateScope,
    UnknownFailClosed,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SelfEvolutionSelectionDecision {
    pub issue_number: u64,
    pub selected: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_class: Option<BoundedTaskClass>,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason_code: Option<SelfEvolutionSelectionReasonCode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery_hint: Option<String>,
    pub fail_closed: bool,
}

#[derive(Clone, Copy)]
struct SelfEvolutionSelectionDefaults {
    failure_reason: &'static str,
    recovery_hint: &'static str,
}

fn self_evolution_selection_defaults(
    reason_code: &SelfEvolutionSelectionReasonCode,
) -> Option<SelfEvolutionSelectionDefaults> {
    match reason_code {
        SelfEvolutionSelectionReasonCode::Accepted => None,
        SelfEvolutionSelectionReasonCode::IssueClosed => Some(SelfEvolutionSelectionDefaults {
            failure_reason: "self-evolution candidate rejected because the issue is closed",
            recovery_hint: "Reopen the issue or choose an active open issue before retrying selection.",
        }),
        SelfEvolutionSelectionReasonCode::MissingEvolutionLabel => {
            Some(SelfEvolutionSelectionDefaults {
                failure_reason: "self-evolution candidate rejected because the issue is missing area/evolution",
                recovery_hint:
                    "Add the area/evolution label or choose an issue already scoped to self-evolution.",
            })
        }
        SelfEvolutionSelectionReasonCode::MissingFeatureLabel => {
            Some(SelfEvolutionSelectionDefaults {
                failure_reason: "self-evolution candidate rejected because the issue is missing type/feature",
                recovery_hint:
                    "Add the type/feature label or narrow the issue to a bounded feature slice before retrying.",
            })
        }
        SelfEvolutionSelectionReasonCode::ExcludedByLabel => Some(SelfEvolutionSelectionDefaults {
            failure_reason: "self-evolution candidate rejected by an excluded issue label",
            recovery_hint:
                "Remove the excluded label or choose a non-duplicate, non-invalid, actionable issue.",
        }),
        SelfEvolutionSelectionReasonCode::UnsupportedCandidateScope => {
            Some(SelfEvolutionSelectionDefaults {
                failure_reason:
                    "self-evolution candidate rejected because the hinted file scope is outside the bounded docs policy",
                recovery_hint:
                    "Narrow candidate paths to the approved docs/*.md boundary before retrying selection.",
            })
        }
        SelfEvolutionSelectionReasonCode::UnknownFailClosed => Some(SelfEvolutionSelectionDefaults {
            failure_reason: "self-evolution candidate failed with an unmapped selection reason",
            recovery_hint: "Unknown selection failure; fail closed and require explicit maintainer triage before retry.",
        }),
    }
}

pub fn accept_self_evolution_selection_decision(
    issue_number: u64,
    candidate_class: BoundedTaskClass,
    summary: Option<&str>,
) -> SelfEvolutionSelectionDecision {
    let summary = normalize_optional_text(summary).unwrap_or_else(|| {
        format!("selected GitHub issue #{issue_number} as a bounded self-evolution candidate")
    });
    SelfEvolutionSelectionDecision {
        issue_number,
        selected: true,
        candidate_class: Some(candidate_class),
        summary,
        reason_code: Some(SelfEvolutionSelectionReasonCode::Accepted),
        failure_reason: None,
        recovery_hint: None,
        fail_closed: false,
    }
}

pub fn reject_self_evolution_selection_decision(
    issue_number: u64,
    reason_code: SelfEvolutionSelectionReasonCode,
    failure_reason: Option<&str>,
    summary: Option<&str>,
) -> SelfEvolutionSelectionDecision {
    let defaults = self_evolution_selection_defaults(&reason_code)
        .unwrap_or(SelfEvolutionSelectionDefaults {
        failure_reason: "self-evolution candidate rejected",
        recovery_hint:
            "Review candidate selection inputs and retry within the bounded self-evolution policy.",
    });
    let failure_reason = normalize_optional_text(failure_reason)
        .unwrap_or_else(|| defaults.failure_reason.to_string());
    let reason_code_key = match reason_code {
        SelfEvolutionSelectionReasonCode::Accepted => "accepted",
        SelfEvolutionSelectionReasonCode::IssueClosed => "issue_closed",
        SelfEvolutionSelectionReasonCode::MissingEvolutionLabel => "missing_evolution_label",
        SelfEvolutionSelectionReasonCode::MissingFeatureLabel => "missing_feature_label",
        SelfEvolutionSelectionReasonCode::ExcludedByLabel => "excluded_by_label",
        SelfEvolutionSelectionReasonCode::UnsupportedCandidateScope => {
            "unsupported_candidate_scope"
        }
        SelfEvolutionSelectionReasonCode::UnknownFailClosed => "unknown_fail_closed",
    };
    let summary = normalize_optional_text(summary).unwrap_or_else(|| {
        format!(
            "rejected GitHub issue #{issue_number} as a self-evolution candidate [{reason_code_key}]"
        )
    });

    SelfEvolutionSelectionDecision {
        issue_number,
        selected: false,
        candidate_class: None,
        summary,
        reason_code: Some(reason_code),
        failure_reason: Some(failure_reason),
        recovery_hint: Some(defaults.recovery_hint.to_string()),
        fail_closed: true,
    }
}

pub fn accept_discovered_candidate(
    dedupe_key: impl Into<String>,
    candidate_source: AutonomousCandidateSource,
    candidate_class: BoundedTaskClass,
    signals: Vec<String>,
    summary: Option<&str>,
) -> DiscoveredCandidate {
    let summary = normalize_optional_text(summary)
        .unwrap_or_else(|| format!("accepted autonomous candidate from {candidate_source:?}"));
    DiscoveredCandidate {
        dedupe_key: dedupe_key.into(),
        candidate_source,
        candidate_class: Some(candidate_class),
        signals,
        accepted: true,
        reason_code: AutonomousIntakeReasonCode::Accepted,
        summary,
        failure_reason: None,
        recovery_hint: None,
        fail_closed: false,
    }
}

pub fn deny_discovered_candidate(
    dedupe_key: impl Into<String>,
    candidate_source: AutonomousCandidateSource,
    signals: Vec<String>,
    reason_code: AutonomousIntakeReasonCode,
) -> DiscoveredCandidate {
    let (failure_reason, recovery_hint) = match reason_code {
        AutonomousIntakeReasonCode::UnsupportedSignalClass => (
            "signal class is not supported by the bounded evolution policy",
            "review supported candidate signal classes and filter input before retry",
        ),
        AutonomousIntakeReasonCode::AmbiguousSignal => (
            "signals do not map to a unique bounded candidate class",
            "provide more specific signal tokens or triage manually before resubmitting",
        ),
        AutonomousIntakeReasonCode::DuplicateCandidate => (
            "an equivalent candidate has already been discovered in this intake window",
            "deduplicate signals before resubmitting or check the existing candidate queue",
        ),
        AutonomousIntakeReasonCode::UnknownFailClosed => (
            "candidate intake failed with an unmapped reason; fail closed",
            "require explicit maintainer triage before retry",
        ),
        AutonomousIntakeReasonCode::Accepted => (
            "unexpected accepted reason on deny path",
            "use accept_discovered_candidate for accepted outcomes",
        ),
    };
    let summary =
        format!("denied autonomous candidate from {candidate_source:?}: {failure_reason}");
    DiscoveredCandidate {
        dedupe_key: dedupe_key.into(),
        candidate_source,
        candidate_class: None,
        signals,
        accepted: false,
        reason_code,
        summary,
        failure_reason: Some(failure_reason.to_string()),
        recovery_hint: Some(recovery_hint.to_string()),
        fail_closed: true,
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct HumanApproval {
    pub approved: bool,
    pub approver: Option<String>,
    pub note: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SupervisedDevloopRequest {
    pub task: AgentTask,
    pub proposal: MutationProposal,
    pub approval: HumanApproval,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum SupervisedDevloopStatus {
    AwaitingApproval,
    RejectedByPolicy,
    FailedClosed,
    Executed,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SupervisedDeliveryStatus {
    Prepared,
    Denied,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SupervisedDeliveryApprovalState {
    Approved,
    MissingExplicitApproval,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SupervisedDeliveryReasonCode {
    DeliveryPrepared,
    AwaitingApproval,
    DeliveryEvidenceMissing,
    ValidationEvidenceMissing,
    UnsupportedTaskScope,
    InconsistentDeliveryEvidence,
    UnknownFailClosed,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SupervisedDeliveryContract {
    pub delivery_summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_summary: Option<String>,
    pub delivery_status: SupervisedDeliveryStatus,
    pub approval_state: SupervisedDeliveryApprovalState,
    pub reason_code: SupervisedDeliveryReasonCode,
    #[serde(default)]
    pub fail_closed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery_hint: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SupervisedExecutionDecision {
    AwaitingApproval,
    ReplayHit,
    PlannerFallback,
    RejectedByPolicy,
    FailedClosed,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SupervisedValidationOutcome {
    NotRun,
    Passed,
    FailedClosed,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SupervisedExecutionReasonCode {
    AwaitingHumanApproval,
    ReplayHit,
    ReplayFallback,
    PolicyDenied,
    ValidationFailed,
    UnsafePatch,
    Timeout,
    MutationPayloadMissing,
    UnknownFailClosed,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SupervisedDevloopOutcome {
    pub task_id: String,
    pub task_class: Option<BoundedTaskClass>,
    pub status: SupervisedDevloopStatus,
    pub execution_decision: SupervisedExecutionDecision,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay_outcome: Option<ReplayFeedback>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<String>,
    pub validation_outcome: SupervisedValidationOutcome,
    pub evidence_summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason_code: Option<SupervisedExecutionReasonCode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery_hint: Option<String>,
    pub execution_feedback: Option<ExecutionFeedback>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_contract: Option<MutationNeededFailureContract>,
    pub summary: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SelfEvolutionAuditConsistencyResult {
    Consistent,
    Inconsistent,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SelfEvolutionAcceptanceGateReasonCode {
    Accepted,
    MissingSelectionEvidence,
    MissingProposalEvidence,
    MissingApprovalEvidence,
    MissingExecutionEvidence,
    MissingDeliveryEvidence,
    InconsistentReasonCodeMatrix,
    UnknownFailClosed,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SelfEvolutionApprovalEvidence {
    pub approval_required: bool,
    pub approved: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approver: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SelfEvolutionDeliveryOutcome {
    pub delivery_status: SupervisedDeliveryStatus,
    pub approval_state: SupervisedDeliveryApprovalState,
    pub reason_code: SupervisedDeliveryReasonCode,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SelfEvolutionReasonCodeMatrix {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection_reason_code: Option<SelfEvolutionSelectionReasonCode>,
    pub proposal_reason_code: MutationProposalContractReasonCode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_reason_code: Option<SupervisedExecutionReasonCode>,
    pub delivery_reason_code: SupervisedDeliveryReasonCode,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SelfEvolutionAcceptanceGateInput {
    pub selection_decision: SelfEvolutionSelectionDecision,
    pub proposal_contract: SelfEvolutionMutationProposalContract,
    pub supervised_request: SupervisedDevloopRequest,
    pub execution_outcome: SupervisedDevloopOutcome,
    pub delivery_contract: SupervisedDeliveryContract,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SelfEvolutionAcceptanceGateContract {
    pub acceptance_gate_summary: String,
    pub audit_consistency_result: SelfEvolutionAuditConsistencyResult,
    pub approval_evidence: SelfEvolutionApprovalEvidence,
    pub delivery_outcome: SelfEvolutionDeliveryOutcome,
    pub reason_code_matrix: SelfEvolutionReasonCodeMatrix,
    pub fail_closed: bool,
    pub reason_code: SelfEvolutionAcceptanceGateReasonCode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery_hint: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handshake_request_with_versions(versions: &[&str]) -> A2aHandshakeRequest {
        A2aHandshakeRequest {
            agent_id: "agent-test".into(),
            role: AgentRole::Planner,
            capability_level: AgentCapabilityLevel::A2,
            supported_protocols: versions
                .iter()
                .map(|version| A2aProtocol {
                    name: A2A_PROTOCOL_NAME.into(),
                    version: (*version).into(),
                })
                .collect(),
            advertised_capabilities: vec![A2aCapability::Coordination],
        }
    }

    #[test]
    fn negotiate_supported_protocol_prefers_v1_when_available() {
        let req = handshake_request_with_versions(&[A2A_PROTOCOL_VERSION, A2A_PROTOCOL_VERSION_V1]);
        let negotiated = req
            .negotiate_supported_protocol()
            .expect("expected protocol negotiation success");
        assert_eq!(negotiated.name, A2A_PROTOCOL_NAME);
        assert_eq!(negotiated.version, A2A_PROTOCOL_VERSION_V1);
    }

    #[test]
    fn negotiate_supported_protocol_falls_back_to_experimental() {
        let req = handshake_request_with_versions(&[A2A_PROTOCOL_VERSION]);
        let negotiated = req
            .negotiate_supported_protocol()
            .expect("expected protocol negotiation success");
        assert_eq!(negotiated.version, A2A_PROTOCOL_VERSION);
    }

    #[test]
    fn negotiate_supported_protocol_returns_none_without_overlap() {
        let req = handshake_request_with_versions(&["0.0.1"]);
        assert!(req.negotiate_supported_protocol().is_none());
    }

    #[test]
    fn normalize_replay_fallback_contract_maps_known_reason() {
        let contract = normalize_replay_fallback_contract(
            &ReplayPlannerDirective::PlanFallback,
            Some("no matching gene"),
            None,
            None,
            None,
            None,
        )
        .expect("contract should exist");

        assert_eq!(
            contract.reason_code,
            ReplayFallbackReasonCode::NoCandidateAfterSelect
        );
        assert_eq!(
            contract.next_action,
            ReplayFallbackNextAction::PlanFromScratch
        );
        assert_eq!(contract.confidence, 92);
    }

    #[test]
    fn normalize_replay_fallback_contract_fails_closed_for_unknown_reason() {
        let contract = normalize_replay_fallback_contract(
            &ReplayPlannerDirective::PlanFallback,
            Some("something unexpected"),
            None,
            None,
            None,
            None,
        )
        .expect("contract should exist");

        assert_eq!(
            contract.reason_code,
            ReplayFallbackReasonCode::UnmappedFallbackReason
        );
        assert_eq!(
            contract.next_action,
            ReplayFallbackNextAction::EscalateFailClosed
        );
        assert_eq!(contract.confidence, 0);
    }

    #[test]
    fn normalize_replay_fallback_contract_rejects_conflicting_next_action() {
        let contract = normalize_replay_fallback_contract(
            &ReplayPlannerDirective::PlanFallback,
            Some("replay validation failed"),
            Some(ReplayFallbackReasonCode::ValidationFailed),
            None,
            Some(ReplayFallbackNextAction::PlanFromScratch),
            Some(88),
        )
        .expect("contract should exist");

        assert_eq!(
            contract.reason_code,
            ReplayFallbackReasonCode::UnmappedFallbackReason
        );
        assert_eq!(
            contract.next_action,
            ReplayFallbackNextAction::EscalateFailClosed
        );
        assert_eq!(contract.confidence, 0);
    }

    #[test]
    fn normalize_mutation_needed_failure_contract_maps_policy_denied() {
        let contract = normalize_mutation_needed_failure_contract(
            Some("supervised devloop rejected task because it is outside bounded scope"),
            None,
        );

        assert_eq!(
            contract.reason_code,
            MutationNeededFailureReasonCode::PolicyDenied
        );
        assert_eq!(
            contract.recovery_action,
            MutationNeededRecoveryAction::NarrowScopeAndRetry
        );
        assert!(contract.fail_closed);
    }

    #[test]
    fn normalize_mutation_needed_failure_contract_maps_timeout() {
        let contract = normalize_mutation_needed_failure_contract(
            Some("command timed out: git apply --check patch.diff"),
            None,
        );

        assert_eq!(
            contract.reason_code,
            MutationNeededFailureReasonCode::Timeout
        );
        assert_eq!(
            contract.recovery_action,
            MutationNeededRecoveryAction::ReduceExecutionBudget
        );
        assert!(contract.fail_closed);
    }

    #[test]
    fn normalize_mutation_needed_failure_contract_fails_closed_for_unknown_reason() {
        let contract =
            normalize_mutation_needed_failure_contract(Some("unexpected runner panic"), None);

        assert_eq!(
            contract.reason_code,
            MutationNeededFailureReasonCode::UnknownFailClosed
        );
        assert_eq!(
            contract.recovery_action,
            MutationNeededRecoveryAction::EscalateFailClosed
        );
        assert!(contract.fail_closed);
    }

    #[test]
    fn reject_self_evolution_selection_decision_maps_closed_issue_defaults() {
        let decision = reject_self_evolution_selection_decision(
            234,
            SelfEvolutionSelectionReasonCode::IssueClosed,
            None,
            None,
        );

        assert!(!decision.selected);
        assert_eq!(decision.issue_number, 234);
        assert_eq!(
            decision.reason_code,
            Some(SelfEvolutionSelectionReasonCode::IssueClosed)
        );
        assert!(decision.fail_closed);
        assert!(decision
            .failure_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("closed")));
        assert!(decision.recovery_hint.is_some());
    }

    #[test]
    fn accept_self_evolution_selection_decision_marks_candidate_selected() {
        let decision =
            accept_self_evolution_selection_decision(235, BoundedTaskClass::DocsSingleFile, None);

        assert!(decision.selected);
        assert_eq!(decision.issue_number, 235);
        assert_eq!(
            decision.candidate_class,
            Some(BoundedTaskClass::DocsSingleFile)
        );
        assert_eq!(
            decision.reason_code,
            Some(SelfEvolutionSelectionReasonCode::Accepted)
        );
        assert!(!decision.fail_closed);
        assert_eq!(decision.failure_reason, None);
        assert_eq!(decision.recovery_hint, None);
    }
}

/// Hub trust tier - defines operational permissions for a Hub
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum HubTrustTier {
    /// Full trust - allows all operations (internal/private Hub)
    Full,
    /// Read-only - allows only read operations (public Hub)
    ReadOnly,
}

/// Hub operation class - classifies the type of A2A operation
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum HubOperationClass {
    Hello,
    Fetch,
    Publish,
    Revoke,
    TaskClaim,
    TaskComplete,
    WorkerRegister,
    Recipe,
    Session,
    Dispute,
    Swarm,
}

impl HubOperationClass {
    /// Returns true if the operation is read-only (allowed for ReadOnly hubs)
    pub fn is_read_only(&self) -> bool {
        matches!(self, HubOperationClass::Hello | HubOperationClass::Fetch)
    }
}

/// Hub profile - describes a Hub's capabilities and configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HubProfile {
    pub hub_id: String,
    pub base_url: String,
    pub trust_tier: HubTrustTier,
    /// Priority for hub selection (higher = preferred)
    pub priority: u32,
    /// Optional health check endpoint
    pub health_url: Option<String>,
}

impl HubProfile {
    /// Check if this hub allows the given operation class
    pub fn allows_operation(&self, operation: &HubOperationClass) -> bool {
        match &self.trust_tier {
            HubTrustTier::Full => true,
            HubTrustTier::ReadOnly => operation.is_read_only(),
        }
    }
}

/// Hub selection policy - defines how to choose between multiple hubs
#[derive(Clone, Debug)]
pub struct HubSelectionPolicy {
    /// Map operation class to allowed trust tiers
    pub allowed_tiers_for_operation: Vec<(HubOperationClass, Vec<HubTrustTier>)>,
    /// Default trust tiers if no specific mapping
    pub default_allowed_tiers: Vec<HubTrustTier>,
}

impl Default for HubSelectionPolicy {
    fn default() -> Self {
        Self {
            allowed_tiers_for_operation: vec![
                (
                    HubOperationClass::Hello,
                    vec![HubTrustTier::Full, HubTrustTier::ReadOnly],
                ),
                (
                    HubOperationClass::Fetch,
                    vec![HubTrustTier::Full, HubTrustTier::ReadOnly],
                ),
                // All write operations require Full trust
                (HubOperationClass::Publish, vec![HubTrustTier::Full]),
                (HubOperationClass::Revoke, vec![HubTrustTier::Full]),
                (HubOperationClass::TaskClaim, vec![HubTrustTier::Full]),
                (HubOperationClass::TaskComplete, vec![HubTrustTier::Full]),
                (HubOperationClass::WorkerRegister, vec![HubTrustTier::Full]),
                (HubOperationClass::Recipe, vec![HubTrustTier::Full]),
                (HubOperationClass::Session, vec![HubTrustTier::Full]),
                (HubOperationClass::Dispute, vec![HubTrustTier::Full]),
                (HubOperationClass::Swarm, vec![HubTrustTier::Full]),
            ],
            default_allowed_tiers: vec![HubTrustTier::Full],
        }
    }
}

impl HubSelectionPolicy {
    /// Get allowed trust tiers for a given operation
    pub fn allowed_tiers(&self, operation: &HubOperationClass) -> &[HubTrustTier] {
        self.allowed_tiers_for_operation
            .iter()
            .find(|(op, _)| op == operation)
            .map(|(_, tiers)| tiers.as_slice())
            .unwrap_or(&self.default_allowed_tiers)
    }
}
