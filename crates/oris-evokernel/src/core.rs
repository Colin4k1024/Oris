//! EvoKernel orchestration: mutation capture, validation, capsule construction, and replay-first reuse.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use oris_agent_contract::{
    accept_discovered_candidate, accept_self_evolution_selection_decision,
    approve_autonomous_mutation_proposal, approve_autonomous_task_plan,
    deny_autonomous_mutation_proposal, deny_autonomous_task_plan, deny_discovered_candidate,
    infer_mutation_needed_failure_reason_code, infer_replay_fallback_reason_code,
    normalize_mutation_needed_failure_contract, normalize_replay_fallback_contract,
    reject_self_evolution_selection_decision, AgentRole, AutonomousApprovalMode,
    AutonomousCandidateSource, AutonomousIntakeInput, AutonomousIntakeOutput,
    AutonomousIntakeReasonCode, AutonomousMutationProposal, AutonomousPlanReasonCode,
    AutonomousProposalReasonCode, AutonomousProposalScope, AutonomousRiskTier, AutonomousTaskPlan,
    BoundedTaskClass, CoordinationMessage, CoordinationPlan, CoordinationPrimitive,
    CoordinationResult, CoordinationTask, DiscoveredCandidate, ExecutionFeedback,
    MutationNeededFailureContract, MutationNeededFailureReasonCode,
    MutationProposal as AgentMutationProposal, MutationProposalContractReasonCode,
    MutationProposalEvidence, MutationProposalScope, MutationProposalValidationBudget,
    ReplayFallbackReasonCode, ReplayFeedback, ReplayPlannerDirective,
    SelfEvolutionAcceptanceGateContract, SelfEvolutionAcceptanceGateInput,
    SelfEvolutionAcceptanceGateReasonCode, SelfEvolutionApprovalEvidence,
    SelfEvolutionAuditConsistencyResult, SelfEvolutionCandidateIntakeRequest,
    SelfEvolutionDeliveryOutcome, SelfEvolutionMutationProposalContract,
    SelfEvolutionReasonCodeMatrix, SelfEvolutionSelectionDecision,
    SelfEvolutionSelectionReasonCode, SupervisedDeliveryApprovalState, SupervisedDeliveryContract,
    SupervisedDeliveryReasonCode, SupervisedDeliveryStatus, SupervisedDevloopOutcome,
    SupervisedDevloopRequest, SupervisedDevloopStatus, SupervisedExecutionDecision,
    SupervisedExecutionReasonCode, SupervisedValidationOutcome,
};
use oris_economics::{EconomicsSignal, EvuLedger, StakePolicy};
use oris_evolution::{
    compute_artifact_hash, decayed_replay_confidence, next_id, stable_hash_json, AssetState,
    BlastRadius, CandidateSource, Capsule, CapsuleId, EnvFingerprint, EvolutionError,
    EvolutionEvent, EvolutionProjection, EvolutionStore, Gene, GeneCandidate, MutationId,
    PreparedMutation, ReplayRoiEvidence, ReplayRoiReasonCode, Selector, SelectorInput,
    StoreBackedSelector, StoredEvolutionEvent, ValidationSnapshot, MIN_REPLAY_CONFIDENCE,
};
use oris_evolution_network::{EvolutionEnvelope, NetworkAsset, SyncAudit};
use oris_governor::{DefaultGovernor, Governor, GovernorDecision, GovernorInput};
use oris_kernel::{Kernel, KernelState, RunId};
use oris_sandbox::{
    compute_blast_radius, execute_allowed_command, Sandbox, SandboxPolicy, SandboxReceipt,
};
use oris_spec::CompiledMutationPlan;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

pub use oris_evolution::{
    builtin_task_classes, default_store_root, signals_match_class, ArtifactEncoding,
    AssetState as EvoAssetState, BlastRadius as EvoBlastRadius,
    CandidateSource as EvoCandidateSource, EnvFingerprint as EvoEnvFingerprint,
    EvolutionStore as EvoEvolutionStore, JsonlEvolutionStore, MutationArtifact, MutationIntent,
    MutationTarget, Outcome, RiskLevel, SelectorInput as EvoSelectorInput, TaskClass,
    TaskClassMatcher, TransitionEvidence, TransitionReasonCode,
    TransitionReasonCode as EvoTransitionReasonCode,
};
pub use oris_evolution_network::{
    FetchQuery, FetchResponse, MessageType, PublishRequest, RevokeNotice,
};
pub use oris_governor::{CoolingWindow, GovernorConfig, RevocationReason};
pub use oris_sandbox::{LocalProcessSandbox, SandboxPolicy as EvoSandboxPolicy};
pub use oris_spec::{SpecCompileError, SpecCompiler, SpecDocument};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ValidationPlan {
    pub profile: String,
    pub stages: Vec<ValidationStage>,
}

impl ValidationPlan {
    pub fn oris_default() -> Self {
        Self {
            profile: "oris-default".into(),
            stages: vec![
                ValidationStage::Command {
                    program: "cargo".into(),
                    args: vec!["fmt".into(), "--all".into(), "--check".into()],
                    timeout_ms: 60_000,
                },
                ValidationStage::Command {
                    program: "cargo".into(),
                    args: vec!["check".into(), "--workspace".into()],
                    timeout_ms: 180_000,
                },
                ValidationStage::Command {
                    program: "cargo".into(),
                    args: vec![
                        "test".into(),
                        "-p".into(),
                        "oris-kernel".into(),
                        "-p".into(),
                        "oris-evolution".into(),
                        "-p".into(),
                        "oris-sandbox".into(),
                        "-p".into(),
                        "oris-evokernel".into(),
                        "--lib".into(),
                    ],
                    timeout_ms: 300_000,
                },
                ValidationStage::Command {
                    program: "cargo".into(),
                    args: vec![
                        "test".into(),
                        "-p".into(),
                        "oris-runtime".into(),
                        "--lib".into(),
                    ],
                    timeout_ms: 300_000,
                },
            ],
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ValidationStage {
    Command {
        program: String,
        args: Vec<String>,
        timeout_ms: u64,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ValidationStageReport {
    pub stage: String,
    pub success: bool,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ValidationReport {
    pub success: bool,
    pub duration_ms: u64,
    pub stages: Vec<ValidationStageReport>,
    pub logs: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignalExtractionInput {
    pub patch_diff: String,
    pub intent: String,
    pub expected_effect: String,
    pub declared_signals: Vec<String>,
    pub changed_files: Vec<String>,
    pub validation_success: bool,
    pub validation_logs: String,
    pub stage_outputs: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignalExtractionOutput {
    pub values: Vec<String>,
    pub hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SeedTemplate {
    pub id: String,
    pub intent: String,
    pub signals: Vec<String>,
    pub diff_payload: String,
    pub validation_profile: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct BootstrapReport {
    pub seeded: bool,
    pub genes_added: usize,
    pub capsules_added: usize,
}

const REPORTED_EXPERIENCE_RETENTION_LIMIT: usize = 3;
const SHADOW_PROMOTION_MIN_REPLAY_ATTEMPTS: u64 = 2;
const SHADOW_PROMOTION_MIN_SUCCESS_RATE: f32 = 0.70;
const SHADOW_PROMOTION_MIN_ENV_MATCH: f32 = 0.75;
const SHADOW_PROMOTION_MIN_DECAYED_CONFIDENCE: f32 = MIN_REPLAY_CONFIDENCE;
const REPLAY_REASONING_TOKEN_FLOOR: u64 = 192;
const REPLAY_REASONING_TOKEN_SIGNAL_WEIGHT: u64 = 24;
const COLD_START_LOOKUP_PENALTY: f32 = 0.05;
const MUTATION_NEEDED_MAX_DIFF_BYTES: usize = 128 * 1024;
const MUTATION_NEEDED_MAX_CHANGED_LINES: usize = 600;
const MUTATION_NEEDED_MAX_SANDBOX_DURATION_MS: u64 = 120_000;
const MUTATION_NEEDED_MAX_VALIDATION_BUDGET_MS: u64 = 900_000;
const SUPERVISED_DEVLOOP_MAX_DOC_FILES: usize = 3;
const SUPERVISED_DEVLOOP_MAX_CARGO_TOML_FILES: usize = 5;
const SUPERVISED_DEVLOOP_MAX_LINT_FILES: usize = 5;
pub const REPLAY_RELEASE_GATE_AGGREGATION_DIMENSIONS: [&str; 2] =
    ["task_class", "source_sender_id"];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepairQualityGateReport {
    pub root_cause: bool,
    pub fix: bool,
    pub verification: bool,
    pub rollback: bool,
    pub incident_anchor: bool,
    pub structure_score: usize,
    pub has_actionable_command: bool,
}

impl RepairQualityGateReport {
    pub fn passes(&self) -> bool {
        self.incident_anchor
            && self.structure_score >= 3
            && (self.has_actionable_command || self.verification)
    }

    pub fn failed_checks(&self) -> Vec<String> {
        let mut failed = Vec::new();
        if !self.incident_anchor {
            failed.push("包含unknown command故障上下文".to_string());
        }
        if self.structure_score < 3 {
            failed.push("结构化修复信息至少满足3项（根因/修复/验证/回滚）".to_string());
        }
        if !(self.has_actionable_command || self.verification) {
            failed.push("包含可执行验证命令或验证计划".to_string());
        }
        failed
    }
}

pub fn evaluate_repair_quality_gate(plan: &str) -> RepairQualityGateReport {
    fn contains_any(haystack: &str, needles: &[&str]) -> bool {
        needles.iter().any(|needle| haystack.contains(needle))
    }

    let lower = plan.to_ascii_lowercase();
    let root_cause = contains_any(
        plan,
        &["根因", "原因分析", "问题定位", "原因定位", "根本原因"],
    ) || contains_any(
        &lower,
        &[
            "root cause",
            "cause analysis",
            "problem diagnosis",
            "diagnosis",
        ],
    );
    let fix = contains_any(
        plan,
        &["修复步骤", "修复方案", "处理步骤", "修复建议", "整改方案"],
    ) || contains_any(
        &lower,
        &[
            "fix",
            "remediation",
            "mitigation",
            "resolution",
            "repair steps",
        ],
    );
    let verification = contains_any(
        plan,
        &["验证命令", "验证步骤", "回归测试", "验证方式", "验收步骤"],
    ) || contains_any(
        &lower,
        &[
            "verification",
            "validate",
            "regression test",
            "smoke test",
            "test command",
        ],
    );
    let rollback = contains_any(plan, &["回滚方案", "回滚步骤", "恢复方案", "撤销方案"])
        || contains_any(&lower, &["rollback", "revert", "fallback plan", "undo"]);
    let incident_anchor = contains_any(
        &lower,
        &[
            "unknown command",
            "process",
            "proccess",
            "command not found",
        ],
    ) || contains_any(plan, &["命令不存在", "命令未找到", "未知命令"]);
    let structure_score = [root_cause, fix, verification, rollback]
        .into_iter()
        .filter(|ok| *ok)
        .count();
    let has_actionable_command = contains_any(
        &lower,
        &[
            "cargo ", "git ", "python ", "pip ", "npm ", "pnpm ", "yarn ", "bash ", "make ",
        ],
    );

    RepairQualityGateReport {
        root_cause,
        fix,
        verification,
        rollback,
        incident_anchor,
        structure_score,
        has_actionable_command,
    }
}

impl ValidationReport {
    pub fn to_snapshot(&self, profile: &str) -> ValidationSnapshot {
        ValidationSnapshot {
            success: self.success,
            profile: profile.to_string(),
            duration_ms: self.duration_ms,
            summary: if self.success {
                "validation passed".into()
            } else {
                "validation failed".into()
            },
        }
    }
}

pub fn extract_deterministic_signals(input: &SignalExtractionInput) -> SignalExtractionOutput {
    let mut signals = BTreeSet::new();

    for declared in &input.declared_signals {
        if let Some(phrase) = normalize_signal_phrase(declared) {
            signals.insert(phrase);
        }
        extend_signal_tokens(&mut signals, declared);
    }

    for text in [
        input.patch_diff.as_str(),
        input.intent.as_str(),
        input.expected_effect.as_str(),
        input.validation_logs.as_str(),
    ] {
        extend_signal_tokens(&mut signals, text);
    }

    for changed_file in &input.changed_files {
        extend_signal_tokens(&mut signals, changed_file);
    }

    for stage_output in &input.stage_outputs {
        extend_signal_tokens(&mut signals, stage_output);
    }

    signals.insert(if input.validation_success {
        "validation passed".into()
    } else {
        "validation failed".into()
    });

    let values = signals.into_iter().take(32).collect::<Vec<_>>();
    let hash =
        stable_hash_json(&values).unwrap_or_else(|_| compute_artifact_hash(&values.join("\n")));
    SignalExtractionOutput { values, hash }
}

#[derive(Debug, Error)]
pub enum ValidationError {
    #[error("validation execution failed: {0}")]
    Execution(String),
}

#[async_trait]
pub trait Validator: Send + Sync {
    async fn run(
        &self,
        receipt: &SandboxReceipt,
        plan: &ValidationPlan,
    ) -> Result<ValidationReport, ValidationError>;
}

pub struct CommandValidator {
    policy: SandboxPolicy,
}

impl CommandValidator {
    pub fn new(policy: SandboxPolicy) -> Self {
        Self { policy }
    }
}

#[async_trait]
impl Validator for CommandValidator {
    async fn run(
        &self,
        receipt: &SandboxReceipt,
        plan: &ValidationPlan,
    ) -> Result<ValidationReport, ValidationError> {
        let started = std::time::Instant::now();
        let mut stages = Vec::new();
        let mut success = true;
        let mut logs = String::new();

        for stage in &plan.stages {
            match stage {
                ValidationStage::Command {
                    program,
                    args,
                    timeout_ms,
                } => {
                    let result = execute_allowed_command(
                        &self.policy,
                        &receipt.workdir,
                        program,
                        args,
                        *timeout_ms,
                    )
                    .await;
                    let report = match result {
                        Ok(output) => ValidationStageReport {
                            stage: format!("{program} {}", args.join(" ")),
                            success: output.success,
                            exit_code: output.exit_code,
                            duration_ms: output.duration_ms,
                            stdout: output.stdout,
                            stderr: output.stderr,
                        },
                        Err(err) => ValidationStageReport {
                            stage: format!("{program} {}", args.join(" ")),
                            success: false,
                            exit_code: None,
                            duration_ms: 0,
                            stdout: String::new(),
                            stderr: err.to_string(),
                        },
                    };
                    if !report.success {
                        success = false;
                    }
                    if !report.stdout.is_empty() {
                        logs.push_str(&report.stdout);
                        logs.push('\n');
                    }
                    if !report.stderr.is_empty() {
                        logs.push_str(&report.stderr);
                        logs.push('\n');
                    }
                    stages.push(report);
                    if !success {
                        break;
                    }
                }
            }
        }

        Ok(ValidationReport {
            success,
            duration_ms: started.elapsed().as_millis() as u64,
            stages,
            logs,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplayDetectEvidence {
    pub task_class_id: String,
    pub task_label: String,
    pub matched_signals: Vec<String>,
    pub mismatch_reasons: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ReplayCandidateEvidence {
    pub rank: usize,
    pub gene_id: String,
    pub capsule_id: Option<String>,
    pub match_quality: f32,
    pub confidence: Option<f32>,
    pub environment_match_factor: Option<f32>,
    pub cold_start_penalty: f32,
    pub final_score: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ReplaySelectEvidence {
    pub exact_match_lookup: bool,
    pub selected_gene_id: Option<String>,
    pub selected_capsule_id: Option<String>,
    pub candidates: Vec<ReplayCandidateEvidence>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ReplayDecision {
    pub used_capsule: bool,
    pub capsule_id: Option<CapsuleId>,
    pub fallback_to_planner: bool,
    pub reason: String,
    pub detect_evidence: ReplayDetectEvidence,
    pub select_evidence: ReplaySelectEvidence,
    pub economics_evidence: ReplayRoiEvidence,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ReplayTaskClassMetrics {
    pub task_class_id: String,
    pub task_label: String,
    pub replay_success_total: u64,
    pub replay_failure_total: u64,
    pub reasoning_steps_avoided_total: u64,
    pub reasoning_avoided_tokens_total: u64,
    pub replay_fallback_cost_total: u64,
    pub replay_roi: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ReplaySourceRoiMetrics {
    pub source_sender_id: String,
    pub replay_success_total: u64,
    pub replay_failure_total: u64,
    pub reasoning_avoided_tokens_total: u64,
    pub replay_fallback_cost_total: u64,
    pub replay_roi: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ReplayRoiWindowSummary {
    pub generated_at: String,
    pub window_seconds: u64,
    pub replay_attempts_total: u64,
    pub replay_success_total: u64,
    pub replay_failure_total: u64,
    pub reasoning_avoided_tokens_total: u64,
    pub replay_fallback_cost_total: u64,
    pub replay_roi: f64,
    pub replay_task_classes: Vec<ReplayTaskClassMetrics>,
    pub replay_sources: Vec<ReplaySourceRoiMetrics>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ReplayRoiReleaseGateThresholds {
    pub min_replay_attempts: u64,
    pub min_replay_hit_rate: f64,
    pub max_false_replay_rate: f64,
    pub min_reasoning_avoided_tokens: u64,
    pub min_replay_roi: f64,
    pub require_replay_safety: bool,
}

impl Default for ReplayRoiReleaseGateThresholds {
    fn default() -> Self {
        Self {
            min_replay_attempts: 3,
            min_replay_hit_rate: 0.60,
            max_false_replay_rate: 0.25,
            min_reasoning_avoided_tokens: REPLAY_REASONING_TOKEN_FLOOR,
            min_replay_roi: 0.05,
            require_replay_safety: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReplayRoiReleaseGateAction {
    BlockRelease,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplayRoiReleaseGateFailClosedPolicy {
    pub on_threshold_violation: ReplayRoiReleaseGateAction,
    pub on_missing_metrics: ReplayRoiReleaseGateAction,
    pub on_invalid_metrics: ReplayRoiReleaseGateAction,
}

impl Default for ReplayRoiReleaseGateFailClosedPolicy {
    fn default() -> Self {
        Self {
            on_threshold_violation: ReplayRoiReleaseGateAction::BlockRelease,
            on_missing_metrics: ReplayRoiReleaseGateAction::BlockRelease,
            on_invalid_metrics: ReplayRoiReleaseGateAction::BlockRelease,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplayRoiReleaseGateSafetySignal {
    pub fail_closed_default: bool,
    pub rollback_ready: bool,
    pub audit_trail_complete: bool,
    pub has_replay_activity: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ReplayRoiReleaseGateInputContract {
    pub generated_at: String,
    pub window_seconds: u64,
    pub aggregation_dimensions: Vec<String>,
    pub replay_attempts_total: u64,
    pub replay_success_total: u64,
    pub replay_failure_total: u64,
    pub replay_hit_rate: f64,
    pub false_replay_rate: f64,
    pub reasoning_avoided_tokens: u64,
    pub replay_fallback_cost_total: u64,
    pub replay_roi: f64,
    pub replay_safety: bool,
    pub replay_safety_signal: ReplayRoiReleaseGateSafetySignal,
    pub thresholds: ReplayRoiReleaseGateThresholds,
    pub fail_closed_policy: ReplayRoiReleaseGateFailClosedPolicy,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReplayRoiReleaseGateStatus {
    Pass,
    FailClosed,
    Indeterminate,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ReplayRoiReleaseGateOutputContract {
    pub status: ReplayRoiReleaseGateStatus,
    pub failed_checks: Vec<String>,
    pub evidence_refs: Vec<String>,
    pub summary: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ReplayRoiReleaseGateContract {
    pub input: ReplayRoiReleaseGateInputContract,
    pub output: ReplayRoiReleaseGateOutputContract,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CoordinationTaskState {
    Ready,
    Waiting,
    BlockedByFailure,
    PermanentlyBlocked,
}

#[derive(Clone, Debug, Default)]
pub struct MultiAgentCoordinator;

impl MultiAgentCoordinator {
    pub fn new() -> Self {
        Self
    }

    pub fn coordinate(&self, plan: CoordinationPlan) -> CoordinationResult {
        let primitive = plan.primitive.clone();
        let root_goal = plan.root_goal.clone();
        let timeout_ms = plan.timeout_ms;
        let max_retries = plan.max_retries;
        let mut tasks = BTreeMap::new();
        for task in plan.tasks {
            tasks.entry(task.id.clone()).or_insert(task);
        }

        let mut pending = tasks.keys().cloned().collect::<BTreeSet<_>>();
        let mut completed = BTreeSet::new();
        let mut failed = BTreeSet::new();
        let mut completed_order = Vec::new();
        let mut failed_order = Vec::new();
        let mut skipped = BTreeSet::new();
        let mut attempts = BTreeMap::new();
        let mut messages = Vec::new();

        loop {
            if matches!(primitive, CoordinationPrimitive::Conditional) {
                self.apply_conditional_skips(
                    &tasks,
                    &mut pending,
                    &completed,
                    &failed,
                    &mut skipped,
                    &mut messages,
                );
            }

            let mut ready = self.ready_task_ids(&tasks, &pending, &completed, &failed, &skipped);
            if ready.is_empty() {
                break;
            }
            if matches!(primitive, CoordinationPrimitive::Sequential) {
                ready.truncate(1);
            }

            for task_id in ready {
                let Some(task) = tasks.get(&task_id) else {
                    continue;
                };
                if !pending.contains(&task_id) {
                    continue;
                }
                self.record_handoff_messages(task, &tasks, &completed, &failed, &mut messages);

                let prior_failures = attempts.get(&task_id).copied().unwrap_or(0);
                if Self::simulate_task_failure(task, prior_failures) {
                    let failure_count = prior_failures + 1;
                    attempts.insert(task_id.clone(), failure_count);
                    let will_retry = failure_count <= max_retries;
                    messages.push(CoordinationMessage {
                        from_role: task.role.clone(),
                        to_role: task.role.clone(),
                        task_id: task_id.clone(),
                        content: if will_retry {
                            format!("task {task_id} failed on attempt {failure_count} and will retry")
                        } else {
                            format!(
                                "task {task_id} failed on attempt {failure_count} and exhausted retries"
                            )
                        },
                    });
                    if !will_retry {
                        pending.remove(&task_id);
                        if failed.insert(task_id.clone()) {
                            failed_order.push(task_id);
                        }
                    }
                    continue;
                }

                pending.remove(&task_id);
                if completed.insert(task_id.clone()) {
                    completed_order.push(task_id);
                }
            }
        }

        let blocked_ids = pending.into_iter().collect::<Vec<_>>();
        for task_id in blocked_ids {
            let Some(task) = tasks.get(&task_id) else {
                continue;
            };
            let state = self.classify_task(task, &tasks, &completed, &failed, &skipped);
            let content = match state {
                CoordinationTaskState::BlockedByFailure => {
                    format!("task {task_id} blocked by failed dependencies")
                }
                CoordinationTaskState::PermanentlyBlocked => {
                    format!("task {task_id} has invalid coordination prerequisites")
                }
                CoordinationTaskState::Waiting => {
                    format!("task {task_id} has unresolved dependencies")
                }
                CoordinationTaskState::Ready => {
                    format!("task {task_id} was left pending unexpectedly")
                }
            };
            messages.push(CoordinationMessage {
                from_role: task.role.clone(),
                to_role: task.role.clone(),
                task_id: task_id.clone(),
                content,
            });
            if failed.insert(task_id.clone()) {
                failed_order.push(task_id);
            }
        }

        CoordinationResult {
            completed_tasks: completed_order,
            failed_tasks: failed_order,
            messages,
            summary: format!(
                "goal '{}' completed {} tasks, failed {}, skipped {} using {:?} coordination (timeout={}ms, max_retries={})",
                root_goal,
                completed.len(),
                failed.len(),
                skipped.len(),
                primitive,
                timeout_ms,
                max_retries
            ),
        }
    }

    fn ready_task_ids(
        &self,
        tasks: &BTreeMap<String, CoordinationTask>,
        pending: &BTreeSet<String>,
        completed: &BTreeSet<String>,
        failed: &BTreeSet<String>,
        skipped: &BTreeSet<String>,
    ) -> Vec<String> {
        pending
            .iter()
            .filter_map(|task_id| {
                let task = tasks.get(task_id)?;
                (self.classify_task(task, tasks, completed, failed, skipped)
                    == CoordinationTaskState::Ready)
                    .then(|| task_id.clone())
            })
            .collect()
    }

    fn apply_conditional_skips(
        &self,
        tasks: &BTreeMap<String, CoordinationTask>,
        pending: &mut BTreeSet<String>,
        completed: &BTreeSet<String>,
        failed: &BTreeSet<String>,
        skipped: &mut BTreeSet<String>,
        messages: &mut Vec<CoordinationMessage>,
    ) {
        let skip_ids = pending
            .iter()
            .filter_map(|task_id| {
                let task = tasks.get(task_id)?;
                (self.classify_task(task, tasks, completed, failed, skipped)
                    == CoordinationTaskState::BlockedByFailure)
                    .then(|| task_id.clone())
            })
            .collect::<Vec<_>>();

        for task_id in skip_ids {
            let Some(task) = tasks.get(&task_id) else {
                continue;
            };
            pending.remove(&task_id);
            skipped.insert(task_id.clone());
            messages.push(CoordinationMessage {
                from_role: task.role.clone(),
                to_role: task.role.clone(),
                task_id: task_id.clone(),
                content: format!("task {task_id} skipped due to failed dependency chain"),
            });
        }
    }

    fn classify_task(
        &self,
        task: &CoordinationTask,
        tasks: &BTreeMap<String, CoordinationTask>,
        completed: &BTreeSet<String>,
        failed: &BTreeSet<String>,
        skipped: &BTreeSet<String>,
    ) -> CoordinationTaskState {
        match task.role {
            AgentRole::Planner | AgentRole::Coder => {
                let mut waiting = false;
                for dependency_id in &task.depends_on {
                    if !tasks.contains_key(dependency_id) {
                        return CoordinationTaskState::PermanentlyBlocked;
                    }
                    if skipped.contains(dependency_id) || failed.contains(dependency_id) {
                        return CoordinationTaskState::BlockedByFailure;
                    }
                    if !completed.contains(dependency_id) {
                        waiting = true;
                    }
                }
                if waiting {
                    CoordinationTaskState::Waiting
                } else {
                    CoordinationTaskState::Ready
                }
            }
            AgentRole::Repair => {
                let mut waiting = false;
                let mut has_coder_dependency = false;
                let mut has_failed_coder = false;
                for dependency_id in &task.depends_on {
                    let Some(dependency) = tasks.get(dependency_id) else {
                        return CoordinationTaskState::PermanentlyBlocked;
                    };
                    let is_coder = matches!(dependency.role, AgentRole::Coder);
                    if is_coder {
                        has_coder_dependency = true;
                    }
                    if skipped.contains(dependency_id) {
                        return CoordinationTaskState::BlockedByFailure;
                    }
                    if failed.contains(dependency_id) {
                        if is_coder {
                            has_failed_coder = true;
                        } else {
                            return CoordinationTaskState::BlockedByFailure;
                        }
                        continue;
                    }
                    if !completed.contains(dependency_id) {
                        waiting = true;
                    }
                }
                if !has_coder_dependency {
                    CoordinationTaskState::PermanentlyBlocked
                } else if waiting {
                    CoordinationTaskState::Waiting
                } else if has_failed_coder {
                    CoordinationTaskState::Ready
                } else {
                    CoordinationTaskState::PermanentlyBlocked
                }
            }
            AgentRole::Optimizer => {
                let mut waiting = false;
                let mut has_impl_dependency = false;
                let mut has_completed_impl = false;
                let mut has_failed_impl = false;
                for dependency_id in &task.depends_on {
                    let Some(dependency) = tasks.get(dependency_id) else {
                        return CoordinationTaskState::PermanentlyBlocked;
                    };
                    let is_impl = matches!(dependency.role, AgentRole::Coder | AgentRole::Repair);
                    if is_impl {
                        has_impl_dependency = true;
                    }
                    if skipped.contains(dependency_id) || failed.contains(dependency_id) {
                        if is_impl {
                            has_failed_impl = true;
                            continue;
                        }
                        return CoordinationTaskState::BlockedByFailure;
                    }
                    if completed.contains(dependency_id) {
                        if is_impl {
                            has_completed_impl = true;
                        }
                        continue;
                    }
                    waiting = true;
                }
                if !has_impl_dependency {
                    CoordinationTaskState::PermanentlyBlocked
                } else if waiting {
                    CoordinationTaskState::Waiting
                } else if has_completed_impl {
                    CoordinationTaskState::Ready
                } else if has_failed_impl {
                    CoordinationTaskState::BlockedByFailure
                } else {
                    CoordinationTaskState::PermanentlyBlocked
                }
            }
        }
    }

    fn record_handoff_messages(
        &self,
        task: &CoordinationTask,
        tasks: &BTreeMap<String, CoordinationTask>,
        completed: &BTreeSet<String>,
        failed: &BTreeSet<String>,
        messages: &mut Vec<CoordinationMessage>,
    ) {
        let mut dependency_ids = task.depends_on.clone();
        dependency_ids.sort();
        dependency_ids.dedup();

        for dependency_id in dependency_ids {
            let Some(dependency) = tasks.get(&dependency_id) else {
                continue;
            };
            if completed.contains(&dependency_id) {
                messages.push(CoordinationMessage {
                    from_role: dependency.role.clone(),
                    to_role: task.role.clone(),
                    task_id: task.id.clone(),
                    content: format!("handoff from {dependency_id} to {}", task.id),
                });
            } else if failed.contains(&dependency_id) {
                messages.push(CoordinationMessage {
                    from_role: dependency.role.clone(),
                    to_role: task.role.clone(),
                    task_id: task.id.clone(),
                    content: format!("failed dependency {dependency_id} routed to {}", task.id),
                });
            }
        }
    }

    fn simulate_task_failure(task: &CoordinationTask, prior_failures: u32) -> bool {
        let normalized = task.description.to_ascii_lowercase();
        normalized.contains("force-fail")
            || (normalized.contains("fail-once") && prior_failures == 0)
    }
}

#[derive(Debug, Error)]
pub enum ReplayError {
    #[error("store error: {0}")]
    Store(String),
    #[error("sandbox error: {0}")]
    Sandbox(String),
    #[error("validation error: {0}")]
    Validation(String),
}

#[async_trait]
pub trait ReplayExecutor: Send + Sync {
    async fn try_replay(
        &self,
        input: &SelectorInput,
        policy: &SandboxPolicy,
        validation: &ValidationPlan,
    ) -> Result<ReplayDecision, ReplayError>;

    async fn try_replay_for_run(
        &self,
        run_id: &RunId,
        input: &SelectorInput,
        policy: &SandboxPolicy,
        validation: &ValidationPlan,
    ) -> Result<ReplayDecision, ReplayError> {
        let _ = run_id;
        self.try_replay(input, policy, validation).await
    }
}

pub struct StoreReplayExecutor {
    pub sandbox: Arc<dyn Sandbox>,
    pub validator: Arc<dyn Validator>,
    pub store: Arc<dyn EvolutionStore>,
    pub selector: Arc<dyn Selector>,
    pub governor: Arc<dyn Governor>,
    pub economics: Option<Arc<Mutex<EvuLedger>>>,
    pub remote_publishers: Option<Arc<Mutex<BTreeMap<String, String>>>>,
    pub stake_policy: StakePolicy,
}

struct ReplayCandidates {
    candidates: Vec<GeneCandidate>,
    exact_match: bool,
}

#[async_trait]
impl ReplayExecutor for StoreReplayExecutor {
    async fn try_replay(
        &self,
        input: &SelectorInput,
        policy: &SandboxPolicy,
        validation: &ValidationPlan,
    ) -> Result<ReplayDecision, ReplayError> {
        self.try_replay_inner(None, input, policy, validation).await
    }

    async fn try_replay_for_run(
        &self,
        run_id: &RunId,
        input: &SelectorInput,
        policy: &SandboxPolicy,
        validation: &ValidationPlan,
    ) -> Result<ReplayDecision, ReplayError> {
        self.try_replay_inner(Some(run_id), input, policy, validation)
            .await
    }
}

impl StoreReplayExecutor {
    fn collect_replay_candidates(&self, input: &SelectorInput) -> ReplayCandidates {
        self.apply_confidence_revalidation();
        let mut selector_input = input.clone();
        if self.economics.is_some() && self.remote_publishers.is_some() {
            selector_input.limit = selector_input.limit.max(4);
        }
        let mut candidates = self.selector.select(&selector_input);
        self.rerank_with_reputation_bias(&mut candidates);
        let mut exact_match = false;
        if candidates.is_empty() {
            let mut exact_candidates = exact_match_candidates(self.store.as_ref(), input);
            self.rerank_with_reputation_bias(&mut exact_candidates);
            if !exact_candidates.is_empty() {
                candidates = exact_candidates;
                exact_match = true;
            }
        }
        if candidates.is_empty() {
            let mut remote_candidates =
                quarantined_remote_exact_match_candidates(self.store.as_ref(), input);
            self.rerank_with_reputation_bias(&mut remote_candidates);
            if !remote_candidates.is_empty() {
                candidates = remote_candidates;
                exact_match = true;
            }
        }
        candidates.truncate(input.limit.max(1));
        ReplayCandidates {
            candidates,
            exact_match,
        }
    }

    fn build_select_evidence(
        &self,
        input: &SelectorInput,
        candidates: &[GeneCandidate],
        exact_match: bool,
    ) -> ReplaySelectEvidence {
        let cold_start_penalty = if exact_match {
            COLD_START_LOOKUP_PENALTY
        } else {
            0.0
        };
        let candidate_rows = candidates
            .iter()
            .enumerate()
            .map(|(idx, candidate)| {
                let top_capsule = candidate.capsules.first();
                let environment_match_factor = top_capsule
                    .map(|capsule| replay_environment_match_factor(&input.env, &capsule.env));
                let final_score = candidate.score * (1.0 - cold_start_penalty);
                ReplayCandidateEvidence {
                    rank: idx + 1,
                    gene_id: candidate.gene.id.clone(),
                    capsule_id: top_capsule.map(|capsule| capsule.id.clone()),
                    match_quality: candidate.score,
                    confidence: top_capsule.map(|capsule| capsule.confidence),
                    environment_match_factor,
                    cold_start_penalty,
                    final_score,
                }
            })
            .collect::<Vec<_>>();

        ReplaySelectEvidence {
            exact_match_lookup: exact_match,
            selected_gene_id: candidate_rows
                .first()
                .map(|candidate| candidate.gene_id.clone()),
            selected_capsule_id: candidate_rows
                .first()
                .and_then(|candidate| candidate.capsule_id.clone()),
            candidates: candidate_rows,
        }
    }

    fn apply_confidence_revalidation(&self) {
        let Ok(projection) = projection_snapshot(self.store.as_ref()) else {
            return;
        };
        for target in stale_replay_revalidation_targets(&projection, Utc::now()) {
            let reason = format!(
                "confidence decayed to {:.3}; revalidation required before replay",
                target.decayed_confidence
            );
            let confidence_decay_ratio = if target.peak_confidence > 0.0 {
                (target.decayed_confidence / target.peak_confidence).clamp(0.0, 1.0)
            } else {
                0.0
            };
            if self
                .store
                .append_event(EvolutionEvent::PromotionEvaluated {
                    gene_id: target.gene_id.clone(),
                    state: AssetState::Quarantined,
                    reason: reason.clone(),
                    reason_code: TransitionReasonCode::RevalidationConfidenceDecay,
                    evidence: Some(TransitionEvidence {
                        replay_attempts: None,
                        replay_successes: None,
                        replay_success_rate: None,
                        environment_match_factor: None,
                        decayed_confidence: Some(target.decayed_confidence),
                        confidence_decay_ratio: Some(confidence_decay_ratio),
                        summary: Some(format!(
                            "phase=confidence_revalidation; decayed_confidence={:.3}; confidence_decay_ratio={:.3}",
                            target.decayed_confidence, confidence_decay_ratio
                        )),
                    }),
                })
                .is_err()
            {
                continue;
            }
            for capsule_id in target.capsule_ids {
                if self
                    .store
                    .append_event(EvolutionEvent::CapsuleQuarantined { capsule_id })
                    .is_err()
                {
                    break;
                }
            }
        }
    }

    fn build_replay_economics_evidence(
        &self,
        input: &SelectorInput,
        candidate: Option<&GeneCandidate>,
        source_sender_id: Option<&str>,
        success: bool,
        reason_code: ReplayRoiReasonCode,
        reason: &str,
    ) -> ReplayRoiEvidence {
        let (task_class_id, task_label) =
            replay_descriptor_from_candidate_or_input(candidate, input);
        let signal_source = candidate
            .map(|best| best.gene.signals.as_slice())
            .unwrap_or(input.signals.as_slice());
        let baseline_tokens = estimated_reasoning_tokens(signal_source);
        let reasoning_avoided_tokens = if success { baseline_tokens } else { 0 };
        let replay_fallback_cost = if success { 0 } else { baseline_tokens };
        let asset_origin =
            candidate.and_then(|best| strategy_metadata_value(&best.gene.strategy, "asset_origin"));
        let mut context_dimensions = vec![
            format!(
                "outcome={}",
                if success {
                    "replay_hit"
                } else {
                    "planner_fallback"
                }
            ),
            format!("reason={reason}"),
            format!("task_class_id={task_class_id}"),
            format!("task_label={task_label}"),
        ];
        if let Some(asset_origin) = asset_origin.as_deref() {
            context_dimensions.push(format!("asset_origin={asset_origin}"));
        }
        if let Some(source_sender_id) = source_sender_id {
            context_dimensions.push(format!("source_sender_id={source_sender_id}"));
        }
        ReplayRoiEvidence {
            success,
            reason_code,
            task_class_id,
            task_label,
            reasoning_avoided_tokens,
            replay_fallback_cost,
            replay_roi: compute_replay_roi(reasoning_avoided_tokens, replay_fallback_cost),
            asset_origin,
            source_sender_id: source_sender_id.map(ToOwned::to_owned),
            context_dimensions,
        }
    }

    fn record_replay_economics(
        &self,
        replay_run_id: Option<&RunId>,
        candidate: Option<&GeneCandidate>,
        capsule_id: Option<&str>,
        evidence: ReplayRoiEvidence,
    ) -> Result<(), ReplayError> {
        self.store
            .append_event(EvolutionEvent::ReplayEconomicsRecorded {
                gene_id: candidate.map(|best| best.gene.id.clone()),
                capsule_id: capsule_id.map(ToOwned::to_owned),
                replay_run_id: replay_run_id.cloned(),
                evidence,
            })
            .map_err(|err| ReplayError::Store(err.to_string()))?;
        Ok(())
    }

    async fn try_replay_inner(
        &self,
        replay_run_id: Option<&RunId>,
        input: &SelectorInput,
        policy: &SandboxPolicy,
        validation: &ValidationPlan,
    ) -> Result<ReplayDecision, ReplayError> {
        let ReplayCandidates {
            candidates,
            exact_match,
        } = self.collect_replay_candidates(input);
        let mut detect_evidence = replay_detect_evidence_from_input(input);
        let select_evidence = self.build_select_evidence(input, &candidates, exact_match);
        let Some(best) = candidates.into_iter().next() else {
            detect_evidence
                .mismatch_reasons
                .push("no_candidate_after_select".to_string());
            let economics_evidence = self.build_replay_economics_evidence(
                input,
                None,
                None,
                false,
                ReplayRoiReasonCode::ReplayMissNoMatchingGene,
                "no matching gene",
            );
            self.record_replay_economics(replay_run_id, None, None, economics_evidence.clone())?;
            return Ok(ReplayDecision {
                used_capsule: false,
                capsule_id: None,
                fallback_to_planner: true,
                reason: "no matching gene".into(),
                detect_evidence,
                select_evidence,
                economics_evidence,
            });
        };
        let (detected_task_class_id, detected_task_label) =
            replay_descriptor_from_candidate_or_input(Some(&best), input);
        detect_evidence.task_class_id = detected_task_class_id;
        detect_evidence.task_label = detected_task_label;
        detect_evidence.matched_signals =
            matched_replay_signals(&input.signals, &best.gene.signals);
        if !exact_match && best.score < 0.82 {
            detect_evidence
                .mismatch_reasons
                .push("score_below_threshold".to_string());
            let reason = format!("best gene score {:.3} below replay threshold", best.score);
            let economics_evidence = self.build_replay_economics_evidence(
                input,
                Some(&best),
                None,
                false,
                ReplayRoiReasonCode::ReplayMissScoreBelowThreshold,
                &reason,
            );
            self.record_replay_economics(
                replay_run_id,
                Some(&best),
                None,
                economics_evidence.clone(),
            )?;
            return Ok(ReplayDecision {
                used_capsule: false,
                capsule_id: None,
                fallback_to_planner: true,
                reason,
                detect_evidence,
                select_evidence,
                economics_evidence,
            });
        }

        let Some(capsule) = best.capsules.first().cloned() else {
            detect_evidence
                .mismatch_reasons
                .push("candidate_has_no_capsule".to_string());
            let economics_evidence = self.build_replay_economics_evidence(
                input,
                Some(&best),
                None,
                false,
                ReplayRoiReasonCode::ReplayMissCandidateHasNoCapsule,
                "candidate gene has no capsule",
            );
            self.record_replay_economics(
                replay_run_id,
                Some(&best),
                None,
                economics_evidence.clone(),
            )?;
            return Ok(ReplayDecision {
                used_capsule: false,
                capsule_id: None,
                fallback_to_planner: true,
                reason: "candidate gene has no capsule".into(),
                detect_evidence,
                select_evidence,
                economics_evidence,
            });
        };
        let remote_publisher = self.publisher_for_capsule(&capsule.id);

        let Some(mutation) = find_declared_mutation(self.store.as_ref(), &capsule.mutation_id)
            .map_err(|err| ReplayError::Store(err.to_string()))?
        else {
            detect_evidence
                .mismatch_reasons
                .push("mutation_payload_missing".to_string());
            let economics_evidence = self.build_replay_economics_evidence(
                input,
                Some(&best),
                remote_publisher.as_deref(),
                false,
                ReplayRoiReasonCode::ReplayMissMutationPayloadMissing,
                "mutation payload missing from store",
            );
            self.record_replay_economics(
                replay_run_id,
                Some(&best),
                Some(&capsule.id),
                economics_evidence.clone(),
            )?;
            return Ok(ReplayDecision {
                used_capsule: false,
                capsule_id: None,
                fallback_to_planner: true,
                reason: "mutation payload missing from store".into(),
                detect_evidence,
                select_evidence,
                economics_evidence,
            });
        };

        let receipt = match self.sandbox.apply(&mutation, policy).await {
            Ok(receipt) => receipt,
            Err(err) => {
                self.record_reuse_settlement(remote_publisher.as_deref(), false);
                let reason = format!("replay patch apply failed: {err}");
                let economics_evidence = self.build_replay_economics_evidence(
                    input,
                    Some(&best),
                    remote_publisher.as_deref(),
                    false,
                    ReplayRoiReasonCode::ReplayMissPatchApplyFailed,
                    &reason,
                );
                self.record_replay_economics(
                    replay_run_id,
                    Some(&best),
                    Some(&capsule.id),
                    economics_evidence.clone(),
                )?;
                detect_evidence
                    .mismatch_reasons
                    .push("patch_apply_failed".to_string());
                return Ok(ReplayDecision {
                    used_capsule: false,
                    capsule_id: Some(capsule.id.clone()),
                    fallback_to_planner: true,
                    reason,
                    detect_evidence,
                    select_evidence,
                    economics_evidence,
                });
            }
        };

        let report = self
            .validator
            .run(&receipt, validation)
            .await
            .map_err(|err| ReplayError::Validation(err.to_string()))?;
        if !report.success {
            self.record_replay_validation_failure(&best, &capsule, validation, &report)?;
            self.record_reuse_settlement(remote_publisher.as_deref(), false);
            let economics_evidence = self.build_replay_economics_evidence(
                input,
                Some(&best),
                remote_publisher.as_deref(),
                false,
                ReplayRoiReasonCode::ReplayMissValidationFailed,
                "replay validation failed",
            );
            self.record_replay_economics(
                replay_run_id,
                Some(&best),
                Some(&capsule.id),
                economics_evidence.clone(),
            )?;
            detect_evidence
                .mismatch_reasons
                .push("validation_failed".to_string());
            return Ok(ReplayDecision {
                used_capsule: false,
                capsule_id: Some(capsule.id.clone()),
                fallback_to_planner: true,
                reason: "replay validation failed".into(),
                detect_evidence,
                select_evidence,
                economics_evidence,
            });
        }

        let requires_shadow_progression = remote_publisher.is_some()
            && matches!(
                capsule.state,
                AssetState::Quarantined | AssetState::ShadowValidated
            );
        if requires_shadow_progression {
            self.store
                .append_event(EvolutionEvent::ValidationPassed {
                    mutation_id: capsule.mutation_id.clone(),
                    report: report.to_snapshot(&validation.profile),
                    gene_id: Some(best.gene.id.clone()),
                })
                .map_err(|err| ReplayError::Store(err.to_string()))?;
            let evidence = self.shadow_transition_evidence(&best.gene.id, &capsule, &input.env)?;
            let (target_state, reason_code, reason, promote_now, phase) =
                if matches!(best.gene.state, AssetState::Quarantined) {
                    (
                        AssetState::ShadowValidated,
                        TransitionReasonCode::PromotionShadowValidationPassed,
                        "remote asset passed first local replay and entered shadow validation"
                            .into(),
                        false,
                        "quarantine_to_shadow",
                    )
                } else if shadow_promotion_gate_passed(&evidence) {
                    (
                        AssetState::Promoted,
                        TransitionReasonCode::PromotionRemoteReplayValidated,
                        "shadow validation thresholds satisfied; remote asset promoted".into(),
                        true,
                        "shadow_to_promoted",
                    )
                } else {
                    (
                        AssetState::ShadowValidated,
                        TransitionReasonCode::ShadowCollectingReplayEvidence,
                        "shadow validation collecting additional replay evidence".into(),
                        false,
                        "shadow_hold",
                    )
                };
            self.store
                .append_event(EvolutionEvent::PromotionEvaluated {
                    gene_id: best.gene.id.clone(),
                    state: target_state.clone(),
                    reason,
                    reason_code,
                    evidence: Some(evidence.to_transition_evidence(shadow_evidence_summary(
                        &evidence,
                        promote_now,
                        phase,
                    ))),
                })
                .map_err(|err| ReplayError::Store(err.to_string()))?;
            if promote_now {
                self.store
                    .append_event(EvolutionEvent::GenePromoted {
                        gene_id: best.gene.id.clone(),
                    })
                    .map_err(|err| ReplayError::Store(err.to_string()))?;
            }
            self.store
                .append_event(EvolutionEvent::CapsuleReleased {
                    capsule_id: capsule.id.clone(),
                    state: target_state,
                })
                .map_err(|err| ReplayError::Store(err.to_string()))?;
        }

        self.store
            .append_event(EvolutionEvent::CapsuleReused {
                capsule_id: capsule.id.clone(),
                gene_id: capsule.gene_id.clone(),
                run_id: capsule.run_id.clone(),
                replay_run_id: replay_run_id.cloned(),
            })
            .map_err(|err| ReplayError::Store(err.to_string()))?;
        self.record_reuse_settlement(remote_publisher.as_deref(), true);
        let reason = if exact_match {
            "replayed via cold-start lookup".to_string()
        } else {
            "replayed via selector".to_string()
        };
        let economics_evidence = self.build_replay_economics_evidence(
            input,
            Some(&best),
            remote_publisher.as_deref(),
            true,
            ReplayRoiReasonCode::ReplayHit,
            &reason,
        );
        self.record_replay_economics(
            replay_run_id,
            Some(&best),
            Some(&capsule.id),
            economics_evidence.clone(),
        )?;

        Ok(ReplayDecision {
            used_capsule: true,
            capsule_id: Some(capsule.id),
            fallback_to_planner: false,
            reason,
            detect_evidence,
            select_evidence,
            economics_evidence,
        })
    }

    fn rerank_with_reputation_bias(&self, candidates: &mut [GeneCandidate]) {
        let Some(ledger) = self.economics.as_ref() else {
            return;
        };
        let reputation_bias = ledger
            .lock()
            .ok()
            .map(|locked| locked.selector_reputation_bias())
            .unwrap_or_default();
        if reputation_bias.is_empty() {
            return;
        }
        let required_assets = candidates
            .iter()
            .filter_map(|candidate| {
                candidate
                    .capsules
                    .first()
                    .map(|capsule| capsule.id.as_str())
            })
            .collect::<Vec<_>>();
        let publisher_map = self.remote_publishers_snapshot(&required_assets);
        if publisher_map.is_empty() {
            return;
        }
        candidates.sort_by(|left, right| {
            effective_candidate_score(right, &publisher_map, &reputation_bias)
                .partial_cmp(&effective_candidate_score(
                    left,
                    &publisher_map,
                    &reputation_bias,
                ))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left.gene.id.cmp(&right.gene.id))
        });
    }

    fn publisher_for_capsule(&self, capsule_id: &str) -> Option<String> {
        self.remote_publishers_snapshot(&[capsule_id])
            .get(capsule_id)
            .cloned()
    }

    fn remote_publishers_snapshot(&self, required_assets: &[&str]) -> BTreeMap<String, String> {
        let cached = self
            .remote_publishers
            .as_ref()
            .and_then(|remote_publishers| {
                remote_publishers.lock().ok().map(|locked| locked.clone())
            })
            .unwrap_or_default();
        if !cached.is_empty()
            && required_assets
                .iter()
                .all(|asset_id| cached.contains_key(*asset_id))
        {
            return cached;
        }

        let persisted = remote_publishers_by_asset_from_store(self.store.as_ref());
        if persisted.is_empty() {
            return cached;
        }

        let mut merged = cached;
        for (asset_id, sender_id) in persisted {
            merged.entry(asset_id).or_insert(sender_id);
        }

        if let Some(remote_publishers) = self.remote_publishers.as_ref() {
            if let Ok(mut locked) = remote_publishers.lock() {
                for (asset_id, sender_id) in &merged {
                    locked.entry(asset_id.clone()).or_insert(sender_id.clone());
                }
            }
        }

        merged
    }

    fn record_reuse_settlement(&self, publisher_id: Option<&str>, success: bool) {
        let Some(publisher_id) = publisher_id else {
            return;
        };
        let Some(ledger) = self.economics.as_ref() else {
            return;
        };
        if let Ok(mut locked) = ledger.lock() {
            locked.settle_remote_reuse(publisher_id, success, &self.stake_policy);
        }
    }

    fn record_replay_validation_failure(
        &self,
        best: &GeneCandidate,
        capsule: &Capsule,
        validation: &ValidationPlan,
        report: &ValidationReport,
    ) -> Result<(), ReplayError> {
        let projection = projection_snapshot(self.store.as_ref())
            .map_err(|err| ReplayError::Store(err.to_string()))?;
        let confidence_context = Self::confidence_context(&projection, &best.gene.id);

        self.store
            .append_event(EvolutionEvent::ValidationFailed {
                mutation_id: capsule.mutation_id.clone(),
                report: report.to_snapshot(&validation.profile),
                gene_id: Some(best.gene.id.clone()),
            })
            .map_err(|err| ReplayError::Store(err.to_string()))?;

        let replay_failures = self.replay_failure_count(&best.gene.id)?;
        let source_sender_id = self.publisher_for_capsule(&capsule.id);
        let governor_decision = self.governor.evaluate(GovernorInput {
            candidate_source: if source_sender_id.is_some() {
                CandidateSource::Remote
            } else {
                CandidateSource::Local
            },
            success_count: 0,
            blast_radius: BlastRadius {
                files_changed: capsule.outcome.changed_files.len(),
                lines_changed: capsule.outcome.lines_changed,
            },
            replay_failures,
            recent_mutation_ages_secs: Vec::new(),
            current_confidence: confidence_context.current_confidence,
            historical_peak_confidence: confidence_context.historical_peak_confidence,
            confidence_last_updated_secs: confidence_context.confidence_last_updated_secs,
        });

        if matches!(governor_decision.target_state, AssetState::Revoked) {
            self.store
                .append_event(EvolutionEvent::PromotionEvaluated {
                    gene_id: best.gene.id.clone(),
                    state: AssetState::Revoked,
                    reason: governor_decision.reason.clone(),
                    reason_code: governor_decision.reason_code.clone(),
                    evidence: Some(confidence_context.to_transition_evidence(
                        "replay_failure_revocation",
                        Some(replay_failures),
                        None,
                        None,
                        None,
                        Some(replay_failure_revocation_summary(
                            replay_failures,
                            confidence_context.current_confidence,
                            confidence_context.historical_peak_confidence,
                            source_sender_id.as_deref(),
                        )),
                    )),
                })
                .map_err(|err| ReplayError::Store(err.to_string()))?;
            self.store
                .append_event(EvolutionEvent::GeneRevoked {
                    gene_id: best.gene.id.clone(),
                    reason: governor_decision.reason,
                })
                .map_err(|err| ReplayError::Store(err.to_string()))?;
            for related in &best.capsules {
                self.store
                    .append_event(EvolutionEvent::CapsuleQuarantined {
                        capsule_id: related.id.clone(),
                    })
                    .map_err(|err| ReplayError::Store(err.to_string()))?;
            }
        }

        Ok(())
    }

    fn confidence_context(
        projection: &EvolutionProjection,
        gene_id: &str,
    ) -> ConfidenceTransitionContext {
        let peak_confidence = projection
            .capsules
            .iter()
            .filter(|capsule| capsule.gene_id == gene_id)
            .map(|capsule| capsule.confidence)
            .fold(0.0_f32, f32::max);
        let age_secs = projection
            .last_updated_at
            .get(gene_id)
            .and_then(|timestamp| Self::seconds_since_timestamp(timestamp, Utc::now()));
        ConfidenceTransitionContext {
            current_confidence: peak_confidence,
            historical_peak_confidence: peak_confidence,
            confidence_last_updated_secs: age_secs,
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

    fn replay_failure_count(&self, gene_id: &str) -> Result<u64, ReplayError> {
        Ok(self
            .store
            .scan(1)
            .map_err(|err| ReplayError::Store(err.to_string()))?
            .into_iter()
            .filter(|stored| {
                matches!(
                    &stored.event,
                    EvolutionEvent::ValidationFailed {
                        gene_id: Some(current_gene_id),
                        ..
                    } if current_gene_id == gene_id
                )
            })
            .count() as u64)
    }

    fn shadow_transition_evidence(
        &self,
        gene_id: &str,
        capsule: &Capsule,
        input_env: &EnvFingerprint,
    ) -> Result<ShadowTransitionEvidence, ReplayError> {
        let events = self
            .store
            .scan(1)
            .map_err(|err| ReplayError::Store(err.to_string()))?;
        let (replay_attempts, replay_successes) = events.iter().fold(
            (0_u64, 0_u64),
            |(attempts, successes), stored| match &stored.event {
                EvolutionEvent::ValidationPassed {
                    gene_id: Some(current_gene_id),
                    ..
                } if current_gene_id == gene_id => (attempts + 1, successes + 1),
                EvolutionEvent::ValidationFailed {
                    gene_id: Some(current_gene_id),
                    ..
                } if current_gene_id == gene_id => (attempts + 1, successes),
                _ => (attempts, successes),
            },
        );
        let replay_success_rate = safe_ratio(replay_successes, replay_attempts) as f32;
        let environment_match_factor = replay_environment_match_factor(input_env, &capsule.env);
        let projection = projection_snapshot(self.store.as_ref())
            .map_err(|err| ReplayError::Store(err.to_string()))?;
        let age_secs = projection
            .last_updated_at
            .get(gene_id)
            .and_then(|timestamp| Self::seconds_since_timestamp(timestamp, Utc::now()));
        let decayed_confidence = decayed_replay_confidence(capsule.confidence, age_secs);
        let confidence_decay_ratio = if capsule.confidence > 0.0 {
            (decayed_confidence / capsule.confidence).clamp(0.0, 1.0)
        } else {
            0.0
        };

        Ok(ShadowTransitionEvidence {
            replay_attempts,
            replay_successes,
            replay_success_rate,
            environment_match_factor,
            decayed_confidence,
            confidence_decay_ratio,
        })
    }
}

#[derive(Clone, Debug)]
struct ShadowTransitionEvidence {
    replay_attempts: u64,
    replay_successes: u64,
    replay_success_rate: f32,
    environment_match_factor: f32,
    decayed_confidence: f32,
    confidence_decay_ratio: f32,
}

impl ShadowTransitionEvidence {
    fn to_transition_evidence(&self, summary: String) -> TransitionEvidence {
        TransitionEvidence {
            replay_attempts: Some(self.replay_attempts),
            replay_successes: Some(self.replay_successes),
            replay_success_rate: Some(self.replay_success_rate),
            environment_match_factor: Some(self.environment_match_factor),
            decayed_confidence: Some(self.decayed_confidence),
            confidence_decay_ratio: Some(self.confidence_decay_ratio),
            summary: Some(summary),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct ConfidenceTransitionContext {
    current_confidence: f32,
    historical_peak_confidence: f32,
    confidence_last_updated_secs: Option<u64>,
}

impl ConfidenceTransitionContext {
    fn decayed_confidence(self) -> f32 {
        decayed_replay_confidence(self.current_confidence, self.confidence_last_updated_secs)
    }

    fn confidence_decay_ratio(self) -> Option<f32> {
        if self.historical_peak_confidence > 0.0 {
            Some((self.decayed_confidence() / self.historical_peak_confidence).clamp(0.0, 1.0))
        } else {
            None
        }
    }

    fn to_transition_evidence(
        self,
        phase: &str,
        replay_attempts: Option<u64>,
        replay_successes: Option<u64>,
        replay_success_rate: Option<f32>,
        environment_match_factor: Option<f32>,
        extra_summary: Option<String>,
    ) -> TransitionEvidence {
        let decayed_confidence = self.decayed_confidence();
        let confidence_decay_ratio = self.confidence_decay_ratio();
        let age_secs = self
            .confidence_last_updated_secs
            .map(|age| age.to_string())
            .unwrap_or_else(|| "unknown".into());
        let mut summary = format!(
            "phase={phase}; current_confidence={:.3}; decayed_confidence={:.3}; historical_peak_confidence={:.3}; confidence_last_updated_secs={age_secs}",
            self.current_confidence, decayed_confidence, self.historical_peak_confidence
        );
        if let Some(ratio) = confidence_decay_ratio {
            summary.push_str(&format!("; confidence_decay_ratio={ratio:.3}"));
        }
        if let Some(extra_summary) = extra_summary {
            summary.push_str("; ");
            summary.push_str(&extra_summary);
        }

        TransitionEvidence {
            replay_attempts,
            replay_successes,
            replay_success_rate,
            environment_match_factor,
            decayed_confidence: Some(decayed_confidence),
            confidence_decay_ratio,
            summary: Some(summary),
        }
    }
}

fn shadow_promotion_gate_passed(evidence: &ShadowTransitionEvidence) -> bool {
    evidence.replay_attempts >= SHADOW_PROMOTION_MIN_REPLAY_ATTEMPTS
        && evidence.replay_success_rate >= SHADOW_PROMOTION_MIN_SUCCESS_RATE
        && evidence.environment_match_factor >= SHADOW_PROMOTION_MIN_ENV_MATCH
        && evidence.decayed_confidence >= SHADOW_PROMOTION_MIN_DECAYED_CONFIDENCE
}

fn shadow_evidence_summary(
    evidence: &ShadowTransitionEvidence,
    promoted: bool,
    phase: &str,
) -> String {
    format!(
        "phase={phase}; replay_attempts={}; replay_successes={}; replay_success_rate={:.3}; environment_match_factor={:.3}; decayed_confidence={:.3}; confidence_decay_ratio={:.3}; promote={promoted}",
        evidence.replay_attempts,
        evidence.replay_successes,
        evidence.replay_success_rate,
        evidence.environment_match_factor,
        evidence.decayed_confidence,
        evidence.confidence_decay_ratio,
    )
}

fn confidence_transition_evidence_for_governor(
    confidence_context: ConfidenceTransitionContext,
    governor_decision: &GovernorDecision,
    success_count: u64,
) -> Option<TransitionEvidence> {
    match governor_decision.reason_code {
        TransitionReasonCode::DowngradeConfidenceRegression => {
            Some(confidence_context.to_transition_evidence(
                "confidence_regression",
                None,
                Some(success_count),
                None,
                None,
                Some(format!("target_state={:?}", governor_decision.target_state)),
            ))
        }
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq)]
struct ConfidenceRevalidationTarget {
    gene_id: String,
    capsule_ids: Vec<String>,
    peak_confidence: f32,
    decayed_confidence: f32,
}

fn stale_replay_revalidation_targets(
    projection: &EvolutionProjection,
    now: DateTime<Utc>,
) -> Vec<ConfidenceRevalidationTarget> {
    projection
        .genes
        .iter()
        .filter(|gene| gene.state == AssetState::Promoted)
        .filter_map(|gene| {
            let promoted_capsules = projection
                .capsules
                .iter()
                .filter(|capsule| {
                    capsule.gene_id == gene.id && capsule.state == AssetState::Promoted
                })
                .collect::<Vec<_>>();
            if promoted_capsules.is_empty() {
                return None;
            }
            let age_secs = projection
                .last_updated_at
                .get(&gene.id)
                .and_then(|timestamp| seconds_since_timestamp_for_confidence(timestamp, now));
            let decayed_confidence = promoted_capsules
                .iter()
                .map(|capsule| decayed_replay_confidence(capsule.confidence, age_secs))
                .fold(0.0_f32, f32::max);
            if decayed_confidence >= MIN_REPLAY_CONFIDENCE {
                return None;
            }
            let peak_confidence = promoted_capsules
                .iter()
                .map(|capsule| capsule.confidence)
                .fold(0.0_f32, f32::max);
            Some(ConfidenceRevalidationTarget {
                gene_id: gene.id.clone(),
                capsule_ids: promoted_capsules
                    .into_iter()
                    .map(|capsule| capsule.id.clone())
                    .collect(),
                peak_confidence,
                decayed_confidence,
            })
        })
        .collect()
}

fn seconds_since_timestamp_for_confidence(timestamp: &str, now: DateTime<Utc>) -> Option<u64> {
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

#[derive(Debug, Error)]
pub enum EvoKernelError {
    #[error("sandbox error: {0}")]
    Sandbox(String),
    #[error("validation error: {0}")]
    Validation(String),
    #[error("validation failed")]
    ValidationFailed(ValidationReport),
    #[error("store error: {0}")]
    Store(String),
}

#[derive(Clone, Debug)]
pub struct CaptureOutcome {
    pub capsule: Capsule,
    pub gene: Gene,
    pub governor_decision: GovernorDecision,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImportOutcome {
    pub imported_asset_ids: Vec<String>,
    pub accepted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_token: Option<String>,
    #[serde(default)]
    pub sync_audit: SyncAudit,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct EvolutionMetricsSnapshot {
    pub replay_attempts_total: u64,
    pub replay_success_total: u64,
    pub replay_success_rate: f64,
    pub confidence_revalidations_total: u64,
    pub replay_reasoning_avoided_total: u64,
    pub reasoning_avoided_tokens_total: u64,
    pub replay_fallback_cost_total: u64,
    pub replay_roi: f64,
    pub replay_task_classes: Vec<ReplayTaskClassMetrics>,
    pub replay_sources: Vec<ReplaySourceRoiMetrics>,
    pub mutation_declared_total: u64,
    pub promoted_mutations_total: u64,
    pub promotion_ratio: f64,
    pub gene_revocations_total: u64,
    pub mutation_velocity_last_hour: u64,
    pub revoke_frequency_last_hour: u64,
    pub promoted_genes: u64,
    pub promoted_capsules: u64,
    pub last_event_seq: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvolutionHealthSnapshot {
    pub status: String,
    pub last_event_seq: u64,
    pub promoted_genes: u64,
    pub promoted_capsules: u64,
}

#[derive(Clone)]
pub struct EvolutionNetworkNode {
    pub store: Arc<dyn EvolutionStore>,
}

impl EvolutionNetworkNode {
    pub fn new(store: Arc<dyn EvolutionStore>) -> Self {
        Self { store }
    }

    pub fn with_default_store() -> Self {
        Self {
            store: Arc::new(JsonlEvolutionStore::new(default_store_root())),
        }
    }

    pub fn accept_publish_request(
        &self,
        request: &PublishRequest,
    ) -> Result<ImportOutcome, EvoKernelError> {
        let requested_cursor = resolve_requested_cursor(
            &request.sender_id,
            request.since_cursor.as_deref(),
            request.resume_token.as_deref(),
        )?;
        import_remote_envelope_into_store(
            self.store.as_ref(),
            &EvolutionEnvelope::publish(request.sender_id.clone(), request.assets.clone()),
            None,
            requested_cursor,
        )
    }

    pub fn ensure_builtin_experience_assets(
        &self,
        sender_id: impl Into<String>,
    ) -> Result<ImportOutcome, EvoKernelError> {
        ensure_builtin_experience_assets_in_store(self.store.as_ref(), sender_id.into())
    }

    pub fn record_reported_experience(
        &self,
        sender_id: impl Into<String>,
        gene_id: impl Into<String>,
        signals: Vec<String>,
        strategy: Vec<String>,
        validation: Vec<String>,
    ) -> Result<ImportOutcome, EvoKernelError> {
        record_reported_experience_in_store(
            self.store.as_ref(),
            sender_id.into(),
            gene_id.into(),
            signals,
            strategy,
            validation,
        )
    }

    pub fn publish_local_assets(
        &self,
        sender_id: impl Into<String>,
    ) -> Result<EvolutionEnvelope, EvoKernelError> {
        export_promoted_assets_from_store(self.store.as_ref(), sender_id)
    }

    pub fn fetch_assets(
        &self,
        responder_id: impl Into<String>,
        query: &FetchQuery,
    ) -> Result<FetchResponse, EvoKernelError> {
        fetch_assets_from_store(self.store.as_ref(), responder_id, query)
    }

    pub fn revoke_assets(&self, notice: &RevokeNotice) -> Result<RevokeNotice, EvoKernelError> {
        revoke_assets_in_store(self.store.as_ref(), notice)
    }

    pub fn metrics_snapshot(&self) -> Result<EvolutionMetricsSnapshot, EvoKernelError> {
        evolution_metrics_snapshot(self.store.as_ref())
    }

    pub fn replay_roi_release_gate_summary(
        &self,
        window_seconds: u64,
    ) -> Result<ReplayRoiWindowSummary, EvoKernelError> {
        replay_roi_release_gate_summary(self.store.as_ref(), window_seconds)
    }

    pub fn render_replay_roi_release_gate_summary_json(
        &self,
        window_seconds: u64,
    ) -> Result<String, EvoKernelError> {
        let summary = self.replay_roi_release_gate_summary(window_seconds)?;
        serde_json::to_string_pretty(&summary)
            .map_err(|err| EvoKernelError::Validation(err.to_string()))
    }

    pub fn replay_roi_release_gate_contract(
        &self,
        window_seconds: u64,
        thresholds: ReplayRoiReleaseGateThresholds,
    ) -> Result<ReplayRoiReleaseGateContract, EvoKernelError> {
        let summary = self.replay_roi_release_gate_summary(window_seconds)?;
        Ok(replay_roi_release_gate_contract(&summary, thresholds))
    }

    pub fn render_replay_roi_release_gate_contract_json(
        &self,
        window_seconds: u64,
        thresholds: ReplayRoiReleaseGateThresholds,
    ) -> Result<String, EvoKernelError> {
        let contract = self.replay_roi_release_gate_contract(window_seconds, thresholds)?;
        serde_json::to_string_pretty(&contract)
            .map_err(|err| EvoKernelError::Validation(err.to_string()))
    }

    pub fn render_metrics_prometheus(&self) -> Result<String, EvoKernelError> {
        self.metrics_snapshot().map(|snapshot| {
            let health = evolution_health_snapshot(&snapshot);
            render_evolution_metrics_prometheus(&snapshot, &health)
        })
    }

    pub fn health_snapshot(&self) -> Result<EvolutionHealthSnapshot, EvoKernelError> {
        self.metrics_snapshot()
            .map(|snapshot| evolution_health_snapshot(&snapshot))
    }
}

pub struct EvoKernel<S: KernelState> {
    pub kernel: Arc<Kernel<S>>,
    pub sandbox: Arc<dyn Sandbox>,
    pub validator: Arc<dyn Validator>,
    pub store: Arc<dyn EvolutionStore>,
    pub selector: Arc<dyn Selector>,
    pub governor: Arc<dyn Governor>,
    pub economics: Arc<Mutex<EvuLedger>>,
    pub remote_publishers: Arc<Mutex<BTreeMap<String, String>>>,
    pub stake_policy: StakePolicy,
    pub sandbox_policy: SandboxPolicy,
    pub validation_plan: ValidationPlan,
}

impl<S: KernelState> EvoKernel<S> {
    fn recent_prior_mutation_ages_secs(
        &self,
        exclude_mutation_id: Option<&str>,
    ) -> Result<Vec<u64>, EvolutionError> {
        let now = Utc::now();
        let mut ages = self
            .store
            .scan(1)?
            .into_iter()
            .filter_map(|stored| match stored.event {
                EvolutionEvent::MutationDeclared { mutation }
                    if exclude_mutation_id != Some(mutation.intent.id.as_str()) =>
                {
                    Self::seconds_since_timestamp(&stored.timestamp, now)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        ages.sort_unstable();
        Ok(ages)
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

    pub fn new(
        kernel: Arc<Kernel<S>>,
        sandbox: Arc<dyn Sandbox>,
        validator: Arc<dyn Validator>,
        store: Arc<dyn EvolutionStore>,
    ) -> Self {
        let selector: Arc<dyn Selector> = Arc::new(StoreBackedSelector::new(store.clone()));
        Self {
            kernel,
            sandbox,
            validator,
            store,
            selector,
            governor: Arc::new(DefaultGovernor::default()),
            economics: Arc::new(Mutex::new(EvuLedger::default())),
            remote_publishers: Arc::new(Mutex::new(BTreeMap::new())),
            stake_policy: StakePolicy::default(),
            sandbox_policy: SandboxPolicy::oris_default(),
            validation_plan: ValidationPlan::oris_default(),
        }
    }

    pub fn with_selector(mut self, selector: Arc<dyn Selector>) -> Self {
        self.selector = selector;
        self
    }

    pub fn with_sandbox_policy(mut self, policy: SandboxPolicy) -> Self {
        self.sandbox_policy = policy;
        self
    }

    pub fn with_governor(mut self, governor: Arc<dyn Governor>) -> Self {
        self.governor = governor;
        self
    }

    pub fn with_economics(mut self, economics: Arc<Mutex<EvuLedger>>) -> Self {
        self.economics = economics;
        self
    }

    pub fn with_stake_policy(mut self, policy: StakePolicy) -> Self {
        self.stake_policy = policy;
        self
    }

    pub fn with_validation_plan(mut self, plan: ValidationPlan) -> Self {
        self.validation_plan = plan;
        self
    }

    pub fn select_candidates(&self, input: &SelectorInput) -> Vec<GeneCandidate> {
        let executor = StoreReplayExecutor {
            sandbox: self.sandbox.clone(),
            validator: self.validator.clone(),
            store: self.store.clone(),
            selector: self.selector.clone(),
            governor: self.governor.clone(),
            economics: Some(self.economics.clone()),
            remote_publishers: Some(self.remote_publishers.clone()),
            stake_policy: self.stake_policy.clone(),
        };
        executor.collect_replay_candidates(input).candidates
    }

    pub fn bootstrap_if_empty(&self, run_id: &RunId) -> Result<BootstrapReport, EvoKernelError> {
        let projection = projection_snapshot(self.store.as_ref())?;
        if !projection.genes.is_empty() {
            return Ok(BootstrapReport::default());
        }

        let templates = built_in_seed_templates();
        for template in &templates {
            let mutation = build_seed_mutation(template);
            let extracted = extract_seed_signals(template);
            let gene = build_bootstrap_gene(template, &extracted)
                .map_err(|err| EvoKernelError::Validation(err.to_string()))?;
            let capsule = build_bootstrap_capsule(run_id, template, &mutation, &gene)
                .map_err(|err| EvoKernelError::Validation(err.to_string()))?;

            self.store
                .append_event(EvolutionEvent::MutationDeclared {
                    mutation: mutation.clone(),
                })
                .map_err(store_err)?;
            self.store
                .append_event(EvolutionEvent::SignalsExtracted {
                    mutation_id: mutation.intent.id.clone(),
                    hash: extracted.hash.clone(),
                    signals: extracted.values.clone(),
                })
                .map_err(store_err)?;
            self.store
                .append_event(EvolutionEvent::GeneProjected { gene: gene.clone() })
                .map_err(store_err)?;
            self.store
                .append_event(EvolutionEvent::PromotionEvaluated {
                    gene_id: gene.id.clone(),
                    state: AssetState::Quarantined,
                    reason: "bootstrap seeds require local validation before replay".into(),
                    reason_code: TransitionReasonCode::DowngradeBootstrapRequiresLocalValidation,
                    evidence: None,
                })
                .map_err(store_err)?;
            self.store
                .append_event(EvolutionEvent::CapsuleCommitted {
                    capsule: capsule.clone(),
                })
                .map_err(store_err)?;
            self.store
                .append_event(EvolutionEvent::CapsuleQuarantined {
                    capsule_id: capsule.id,
                })
                .map_err(store_err)?;
        }

        Ok(BootstrapReport {
            seeded: true,
            genes_added: templates.len(),
            capsules_added: templates.len(),
        })
    }

    pub async fn capture_successful_mutation(
        &self,
        run_id: &RunId,
        mutation: PreparedMutation,
    ) -> Result<Capsule, EvoKernelError> {
        Ok(self
            .capture_mutation_with_governor(run_id, mutation)
            .await?
            .capsule)
    }

    pub async fn capture_mutation_with_governor(
        &self,
        run_id: &RunId,
        mutation: PreparedMutation,
    ) -> Result<CaptureOutcome, EvoKernelError> {
        self.store
            .append_event(EvolutionEvent::MutationDeclared {
                mutation: mutation.clone(),
            })
            .map_err(store_err)?;

        let receipt = match self.sandbox.apply(&mutation, &self.sandbox_policy).await {
            Ok(receipt) => receipt,
            Err(err) => {
                let message = err.to_string();
                let contract = mutation_needed_contract_for_error_message(&message);
                self.store
                    .append_event(EvolutionEvent::MutationRejected {
                        mutation_id: mutation.intent.id.clone(),
                        reason: contract.failure_reason,
                        reason_code: Some(
                            mutation_needed_reason_code_key(contract.reason_code).to_string(),
                        ),
                        recovery_hint: Some(contract.recovery_hint),
                        fail_closed: contract.fail_closed,
                    })
                    .map_err(store_err)?;
                return Err(EvoKernelError::Sandbox(message));
            }
        };

        self.store
            .append_event(EvolutionEvent::MutationApplied {
                mutation_id: mutation.intent.id.clone(),
                patch_hash: receipt.patch_hash.clone(),
                changed_files: receipt
                    .changed_files
                    .iter()
                    .map(|path| path.to_string_lossy().to_string())
                    .collect(),
            })
            .map_err(store_err)?;

        let report = match self.validator.run(&receipt, &self.validation_plan).await {
            Ok(report) => report,
            Err(err) => {
                let message = format!("mutation-needed validation execution error: {err}");
                let contract = mutation_needed_contract_for_error_message(&message);
                self.store
                    .append_event(EvolutionEvent::MutationRejected {
                        mutation_id: mutation.intent.id.clone(),
                        reason: contract.failure_reason,
                        reason_code: Some(
                            mutation_needed_reason_code_key(contract.reason_code).to_string(),
                        ),
                        recovery_hint: Some(contract.recovery_hint),
                        fail_closed: contract.fail_closed,
                    })
                    .map_err(store_err)?;
                return Err(EvoKernelError::Validation(message));
            }
        };
        if !report.success {
            self.store
                .append_event(EvolutionEvent::ValidationFailed {
                    mutation_id: mutation.intent.id.clone(),
                    report: report.to_snapshot(&self.validation_plan.profile),
                    gene_id: None,
                })
                .map_err(store_err)?;
            let contract = mutation_needed_contract_for_validation_failure(
                &self.validation_plan.profile,
                &report,
            );
            self.store
                .append_event(EvolutionEvent::MutationRejected {
                    mutation_id: mutation.intent.id.clone(),
                    reason: contract.failure_reason,
                    reason_code: Some(
                        mutation_needed_reason_code_key(contract.reason_code).to_string(),
                    ),
                    recovery_hint: Some(contract.recovery_hint),
                    fail_closed: contract.fail_closed,
                })
                .map_err(store_err)?;
            return Err(EvoKernelError::ValidationFailed(report));
        }

        self.store
            .append_event(EvolutionEvent::ValidationPassed {
                mutation_id: mutation.intent.id.clone(),
                report: report.to_snapshot(&self.validation_plan.profile),
                gene_id: None,
            })
            .map_err(store_err)?;

        let extracted_signals = extract_deterministic_signals(&SignalExtractionInput {
            patch_diff: mutation.artifact.payload.clone(),
            intent: mutation.intent.intent.clone(),
            expected_effect: mutation.intent.expected_effect.clone(),
            declared_signals: mutation.intent.signals.clone(),
            changed_files: receipt
                .changed_files
                .iter()
                .map(|path| path.to_string_lossy().to_string())
                .collect(),
            validation_success: report.success,
            validation_logs: report.logs.clone(),
            stage_outputs: report
                .stages
                .iter()
                .flat_map(|stage| [stage.stdout.clone(), stage.stderr.clone()])
                .filter(|value| !value.is_empty())
                .collect(),
        });
        self.store
            .append_event(EvolutionEvent::SignalsExtracted {
                mutation_id: mutation.intent.id.clone(),
                hash: extracted_signals.hash.clone(),
                signals: extracted_signals.values.clone(),
            })
            .map_err(store_err)?;

        let projection = projection_snapshot(self.store.as_ref())?;
        let blast_radius = compute_blast_radius(&mutation.artifact.payload);
        let recent_mutation_ages_secs = self
            .recent_prior_mutation_ages_secs(Some(mutation.intent.id.as_str()))
            .map_err(store_err)?;
        let mut gene = derive_gene(
            &mutation,
            &receipt,
            &self.validation_plan.profile,
            &extracted_signals.values,
        );
        let confidence_context = StoreReplayExecutor::confidence_context(&projection, &gene.id);
        let success_count = projection
            .genes
            .iter()
            .find(|existing| existing.id == gene.id)
            .map(|existing| {
                projection
                    .capsules
                    .iter()
                    .filter(|capsule| capsule.gene_id == existing.id)
                    .count() as u64
            })
            .unwrap_or(0)
            + 1;
        let governor_decision = self.governor.evaluate(GovernorInput {
            candidate_source: CandidateSource::Local,
            success_count,
            blast_radius: blast_radius.clone(),
            replay_failures: 0,
            recent_mutation_ages_secs,
            current_confidence: confidence_context.current_confidence,
            historical_peak_confidence: confidence_context.historical_peak_confidence,
            confidence_last_updated_secs: confidence_context.confidence_last_updated_secs,
        });

        gene.state = governor_decision.target_state.clone();
        self.store
            .append_event(EvolutionEvent::GeneProjected { gene: gene.clone() })
            .map_err(store_err)?;
        self.store
            .append_event(EvolutionEvent::PromotionEvaluated {
                gene_id: gene.id.clone(),
                state: governor_decision.target_state.clone(),
                reason: governor_decision.reason.clone(),
                reason_code: governor_decision.reason_code.clone(),
                evidence: confidence_transition_evidence_for_governor(
                    confidence_context,
                    &governor_decision,
                    success_count,
                ),
            })
            .map_err(store_err)?;
        if matches!(governor_decision.target_state, AssetState::Promoted) {
            self.store
                .append_event(EvolutionEvent::GenePromoted {
                    gene_id: gene.id.clone(),
                })
                .map_err(store_err)?;
        }
        if matches!(governor_decision.target_state, AssetState::Revoked) {
            self.store
                .append_event(EvolutionEvent::GeneRevoked {
                    gene_id: gene.id.clone(),
                    reason: governor_decision.reason.clone(),
                })
                .map_err(store_err)?;
        }
        if let Some(spec_id) = &mutation.intent.spec_id {
            self.store
                .append_event(EvolutionEvent::SpecLinked {
                    mutation_id: mutation.intent.id.clone(),
                    spec_id: spec_id.clone(),
                })
                .map_err(store_err)?;
        }

        let mut capsule = build_capsule(
            run_id,
            &mutation,
            &receipt,
            &report,
            &self.validation_plan.profile,
            &gene,
            &blast_radius,
        )
        .map_err(|err| EvoKernelError::Validation(err.to_string()))?;
        capsule.state = governor_decision.target_state.clone();
        self.store
            .append_event(EvolutionEvent::CapsuleCommitted {
                capsule: capsule.clone(),
            })
            .map_err(store_err)?;
        if matches!(governor_decision.target_state, AssetState::Quarantined) {
            self.store
                .append_event(EvolutionEvent::CapsuleQuarantined {
                    capsule_id: capsule.id.clone(),
                })
                .map_err(store_err)?;
        }

        Ok(CaptureOutcome {
            capsule,
            gene,
            governor_decision,
        })
    }

    pub async fn capture_from_proposal(
        &self,
        run_id: &RunId,
        proposal: &AgentMutationProposal,
        diff_payload: String,
        base_revision: Option<String>,
    ) -> Result<CaptureOutcome, EvoKernelError> {
        let intent = MutationIntent {
            id: next_id("proposal"),
            intent: proposal.intent.clone(),
            target: MutationTarget::Paths {
                allow: proposal.files.clone(),
            },
            expected_effect: proposal.expected_effect.clone(),
            risk: RiskLevel::Low,
            signals: proposal.files.clone(),
            spec_id: None,
        };
        self.capture_mutation_with_governor(
            run_id,
            prepare_mutation(intent, diff_payload, base_revision),
        )
        .await
    }

    pub fn feedback_for_agent(outcome: &CaptureOutcome) -> ExecutionFeedback {
        ExecutionFeedback {
            accepted: !matches!(outcome.governor_decision.target_state, AssetState::Revoked),
            asset_state: Some(format!("{:?}", outcome.governor_decision.target_state)),
            summary: outcome.governor_decision.reason.clone(),
        }
    }

    pub fn replay_feedback_for_agent(
        signals: &[String],
        decision: &ReplayDecision,
    ) -> ReplayFeedback {
        let (fallback_task_class_id, fallback_task_label) = replay_task_descriptor(signals);
        let task_class_id = if decision.detect_evidence.task_class_id.is_empty() {
            fallback_task_class_id
        } else {
            decision.detect_evidence.task_class_id.clone()
        };
        let task_label = if decision.detect_evidence.task_label.is_empty() {
            fallback_task_label
        } else {
            decision.detect_evidence.task_label.clone()
        };
        let planner_directive = if decision.used_capsule {
            ReplayPlannerDirective::SkipPlanner
        } else {
            ReplayPlannerDirective::PlanFallback
        };
        let reasoning_steps_avoided = u64::from(decision.used_capsule);
        let reason_code_hint = decision
            .detect_evidence
            .mismatch_reasons
            .first()
            .and_then(|reason| infer_replay_fallback_reason_code(reason));
        let fallback_contract = normalize_replay_fallback_contract(
            &planner_directive,
            decision
                .fallback_to_planner
                .then_some(decision.reason.as_str()),
            reason_code_hint,
            None,
            None,
            None,
        );
        let summary = if decision.used_capsule {
            format!("reused prior capsule for task class '{task_label}'; skip planner")
        } else {
            format!(
                "planner fallback required for task class '{task_label}': {}",
                decision.reason
            )
        };

        ReplayFeedback {
            used_capsule: decision.used_capsule,
            capsule_id: decision.capsule_id.clone(),
            planner_directive,
            reasoning_steps_avoided,
            fallback_reason: fallback_contract
                .as_ref()
                .map(|contract| contract.fallback_reason.clone()),
            reason_code: fallback_contract
                .as_ref()
                .map(|contract| contract.reason_code),
            repair_hint: fallback_contract
                .as_ref()
                .map(|contract| contract.repair_hint.clone()),
            next_action: fallback_contract
                .as_ref()
                .map(|contract| contract.next_action),
            confidence: fallback_contract
                .as_ref()
                .map(|contract| contract.confidence),
            task_class_id,
            task_label,
            summary,
        }
    }

    fn mutation_needed_failure_outcome(
        &self,
        request: &SupervisedDevloopRequest,
        task_class: Option<BoundedTaskClass>,
        status: SupervisedDevloopStatus,
        contract: MutationNeededFailureContract,
        replay_outcome: Option<ReplayFeedback>,
        mutation_id_for_audit: Option<String>,
    ) -> Result<SupervisedDevloopOutcome, EvoKernelError> {
        if let Some(mutation_id) = mutation_id_for_audit {
            self.store
                .append_event(EvolutionEvent::MutationRejected {
                    mutation_id,
                    reason: contract.failure_reason.clone(),
                    reason_code: Some(
                        mutation_needed_reason_code_key(contract.reason_code).to_string(),
                    ),
                    recovery_hint: Some(contract.recovery_hint.clone()),
                    fail_closed: contract.fail_closed,
                })
                .map_err(store_err)?;
        }
        let status_label = match status {
            SupervisedDevloopStatus::AwaitingApproval => "awaiting_approval",
            SupervisedDevloopStatus::RejectedByPolicy => "rejected_by_policy",
            SupervisedDevloopStatus::FailedClosed => "failed_closed",
            SupervisedDevloopStatus::Executed => "executed",
        };
        let reason_code_key = mutation_needed_reason_code_key(contract.reason_code);
        let execution_decision = supervised_execution_decision_from_status(status);
        let validation_outcome = supervised_validation_outcome_from_status(status);
        let fallback_reason = replay_outcome
            .as_ref()
            .and_then(|feedback| feedback.fallback_reason.clone());
        let evidence_summary = supervised_execution_evidence_summary(
            execution_decision,
            task_class.as_ref(),
            validation_outcome,
            fallback_reason.as_deref(),
            Some(reason_code_key),
        );
        Ok(SupervisedDevloopOutcome {
            task_id: request.task.id.clone(),
            task_class,
            status,
            execution_decision,
            replay_outcome,
            fallback_reason: fallback_reason.clone(),
            validation_outcome,
            evidence_summary,
            reason_code: Some(supervised_reason_code_from_mutation_needed(
                contract.reason_code,
            )),
            recovery_hint: Some(contract.recovery_hint.clone()),
            execution_feedback: None,
            failure_contract: Some(contract.clone()),
            summary: format!(
                "supervised devloop {status_label} task '{}' [{reason_code_key}]: {}",
                request.task.id, contract.failure_reason
            ),
        })
    }

    pub async fn run_supervised_devloop(
        &self,
        run_id: &RunId,
        request: &SupervisedDevloopRequest,
        diff_payload: String,
        base_revision: Option<String>,
    ) -> Result<SupervisedDevloopOutcome, EvoKernelError> {
        let audit_mutation_id = mutation_needed_audit_mutation_id(request);
        let proposal_contract = self.supervised_devloop_mutation_proposal_contract(request);
        if proposal_contract.fail_closed {
            let task_class = proposal_contract
                .proposal_scope
                .as_ref()
                .map(|scope| scope.task_class.clone());
            let contract = mutation_needed_contract_from_proposal_contract(&proposal_contract);
            let status = mutation_needed_status_from_reason_code(contract.reason_code);
            return self.mutation_needed_failure_outcome(
                request,
                task_class,
                status,
                contract,
                None,
                Some(audit_mutation_id),
            );
        }

        let task_class = proposal_contract
            .proposal_scope
            .as_ref()
            .map(|scope| scope.task_class.clone());
        let Some(task_class) = task_class else {
            let contract = normalize_mutation_needed_failure_contract(
                Some(&format!(
                    "supervised devloop rejected task '{}' because it is an unsupported task outside the bounded scope",
                    request.task.id
                )),
                Some(MutationNeededFailureReasonCode::PolicyDenied),
            );
            return self.mutation_needed_failure_outcome(
                request,
                None,
                SupervisedDevloopStatus::RejectedByPolicy,
                contract,
                None,
                Some(audit_mutation_id),
            );
        };

        if !request.approval.approved {
            return Ok(SupervisedDevloopOutcome {
                task_id: request.task.id.clone(),
                task_class: Some(task_class.clone()),
                status: SupervisedDevloopStatus::AwaitingApproval,
                execution_decision: SupervisedExecutionDecision::AwaitingApproval,
                replay_outcome: None,
                fallback_reason: None,
                validation_outcome: SupervisedValidationOutcome::NotRun,
                evidence_summary: supervised_execution_evidence_summary(
                    SupervisedExecutionDecision::AwaitingApproval,
                    Some(&task_class),
                    SupervisedValidationOutcome::NotRun,
                    None,
                    Some("awaiting_human_approval"),
                ),
                reason_code: Some(SupervisedExecutionReasonCode::AwaitingHumanApproval),
                recovery_hint: Some(
                    "Grant explicit human approval before supervised execution can proceed."
                        .to_string(),
                ),
                execution_feedback: None,
                failure_contract: None,
                summary: format!(
                    "supervised devloop paused task '{}' until explicit human approval is granted",
                    request.task.id
                ),
            });
        }

        let replay_outcome = self
            .supervised_devloop_replay_outcome(run_id, request, &diff_payload)
            .await?;
        if let Some(replay_feedback) = replay_outcome.as_ref() {
            if replay_feedback.used_capsule {
                return Ok(SupervisedDevloopOutcome {
                    task_id: request.task.id.clone(),
                    task_class: Some(task_class.clone()),
                    status: SupervisedDevloopStatus::Executed,
                    execution_decision: SupervisedExecutionDecision::ReplayHit,
                    replay_outcome: Some(replay_feedback.clone()),
                    fallback_reason: None,
                    validation_outcome: SupervisedValidationOutcome::Passed,
                    evidence_summary: supervised_execution_evidence_summary(
                        SupervisedExecutionDecision::ReplayHit,
                        Some(&task_class),
                        SupervisedValidationOutcome::Passed,
                        None,
                        Some("replay_hit"),
                    ),
                    reason_code: Some(SupervisedExecutionReasonCode::ReplayHit),
                    recovery_hint: None,
                    execution_feedback: Some(ExecutionFeedback {
                        accepted: true,
                        asset_state: Some("replayed".to_string()),
                        summary: replay_feedback.summary.clone(),
                    }),
                    failure_contract: None,
                    summary: format!(
                        "supervised devloop reused replay capsule for task '{}' after explicit approval",
                        request.task.id
                    ),
                });
            }

            if let Some(contract) =
                supervised_devloop_fail_closed_contract_from_replay(replay_feedback)
            {
                let status = mutation_needed_status_from_reason_code(contract.reason_code);
                return self.mutation_needed_failure_outcome(
                    request,
                    Some(task_class),
                    status,
                    contract,
                    Some(replay_feedback.clone()),
                    None,
                );
            }
        }

        if diff_payload.len() > MUTATION_NEEDED_MAX_DIFF_BYTES {
            let contract = normalize_mutation_needed_failure_contract(
                Some(&format!(
                    "mutation-needed diff payload exceeds bounded byte budget (size={}, max={})",
                    diff_payload.len(),
                    MUTATION_NEEDED_MAX_DIFF_BYTES
                )),
                Some(MutationNeededFailureReasonCode::PolicyDenied),
            );
            return self.mutation_needed_failure_outcome(
                request,
                Some(task_class),
                SupervisedDevloopStatus::RejectedByPolicy,
                contract,
                replay_outcome.clone(),
                Some(audit_mutation_id),
            );
        }

        let blast_radius = compute_blast_radius(&diff_payload);
        if blast_radius.lines_changed > MUTATION_NEEDED_MAX_CHANGED_LINES {
            let contract = normalize_mutation_needed_failure_contract(
                Some(&format!(
                    "mutation-needed patch exceeds bounded changed-line budget (lines_changed={}, max={})",
                    blast_radius.lines_changed,
                    MUTATION_NEEDED_MAX_CHANGED_LINES
                )),
                Some(MutationNeededFailureReasonCode::UnsafePatch),
            );
            return self.mutation_needed_failure_outcome(
                request,
                Some(task_class),
                SupervisedDevloopStatus::FailedClosed,
                contract,
                replay_outcome.clone(),
                Some(audit_mutation_id),
            );
        }

        if self.sandbox_policy.max_duration_ms > MUTATION_NEEDED_MAX_SANDBOX_DURATION_MS {
            let contract = normalize_mutation_needed_failure_contract(
                Some(&format!(
                    "mutation-needed sandbox duration budget exceeds bounded policy (configured={}ms, max={}ms)",
                    self.sandbox_policy.max_duration_ms,
                    MUTATION_NEEDED_MAX_SANDBOX_DURATION_MS
                )),
                Some(MutationNeededFailureReasonCode::PolicyDenied),
            );
            return self.mutation_needed_failure_outcome(
                request,
                Some(task_class),
                SupervisedDevloopStatus::RejectedByPolicy,
                contract,
                replay_outcome.clone(),
                Some(audit_mutation_id),
            );
        }

        let validation_budget_ms = validation_plan_timeout_budget_ms(&self.validation_plan);
        if validation_budget_ms > MUTATION_NEEDED_MAX_VALIDATION_BUDGET_MS {
            let contract = normalize_mutation_needed_failure_contract(
                Some(&format!(
                    "mutation-needed validation timeout budget exceeds bounded policy (configured={}ms, max={}ms)",
                    validation_budget_ms,
                    MUTATION_NEEDED_MAX_VALIDATION_BUDGET_MS
                )),
                Some(MutationNeededFailureReasonCode::PolicyDenied),
            );
            return self.mutation_needed_failure_outcome(
                request,
                Some(task_class),
                SupervisedDevloopStatus::RejectedByPolicy,
                contract,
                replay_outcome.clone(),
                Some(audit_mutation_id),
            );
        }

        let capture = match self
            .capture_from_proposal(run_id, &request.proposal, diff_payload, base_revision)
            .await
        {
            Ok(capture) => capture,
            Err(EvoKernelError::Sandbox(message)) => {
                let contract = mutation_needed_contract_for_error_message(&message);
                let status = mutation_needed_status_from_reason_code(contract.reason_code);
                return self.mutation_needed_failure_outcome(
                    request,
                    Some(task_class),
                    status,
                    contract,
                    replay_outcome.clone(),
                    None,
                );
            }
            Err(EvoKernelError::ValidationFailed(report)) => {
                let contract = mutation_needed_contract_for_validation_failure(
                    &self.validation_plan.profile,
                    &report,
                );
                let status = mutation_needed_status_from_reason_code(contract.reason_code);
                return self.mutation_needed_failure_outcome(
                    request,
                    Some(task_class),
                    status,
                    contract,
                    replay_outcome.clone(),
                    None,
                );
            }
            Err(EvoKernelError::Validation(message)) => {
                let contract = mutation_needed_contract_for_error_message(&message);
                let status = mutation_needed_status_from_reason_code(contract.reason_code);
                return self.mutation_needed_failure_outcome(
                    request,
                    Some(task_class),
                    status,
                    contract,
                    replay_outcome.clone(),
                    None,
                );
            }
            Err(err) => return Err(err),
        };
        let approver = request
            .approval
            .approver
            .as_deref()
            .unwrap_or("unknown approver");

        Ok(SupervisedDevloopOutcome {
            task_id: request.task.id.clone(),
            task_class: Some(task_class.clone()),
            status: SupervisedDevloopStatus::Executed,
            execution_decision: SupervisedExecutionDecision::PlannerFallback,
            replay_outcome: replay_outcome.clone(),
            fallback_reason: replay_outcome
                .as_ref()
                .and_then(|feedback| feedback.fallback_reason.clone()),
            validation_outcome: SupervisedValidationOutcome::Passed,
            evidence_summary: supervised_execution_evidence_summary(
                SupervisedExecutionDecision::PlannerFallback,
                Some(&task_class),
                SupervisedValidationOutcome::Passed,
                replay_outcome
                    .as_ref()
                    .and_then(|feedback| feedback.fallback_reason.as_deref()),
                Some("replay_fallback"),
            ),
            reason_code: Some(SupervisedExecutionReasonCode::ReplayFallback),
            recovery_hint: replay_outcome
                .as_ref()
                .and_then(|feedback| feedback.repair_hint.clone()),
            execution_feedback: Some(Self::feedback_for_agent(&capture)),
            failure_contract: None,
            summary: format!(
                "supervised devloop executed task '{}' with explicit approval from {approver}",
                request.task.id
            ),
        })
    }

    pub fn prepare_supervised_delivery(
        &self,
        request: &SupervisedDevloopRequest,
        outcome: &SupervisedDevloopOutcome,
    ) -> Result<SupervisedDeliveryContract, EvoKernelError> {
        let audit_mutation_id = mutation_needed_audit_mutation_id(request);
        let approval_state = supervised_delivery_approval_state(&request.approval);
        if !matches!(approval_state, SupervisedDeliveryApprovalState::Approved) {
            let contract = supervised_delivery_denied_contract(
                request,
                SupervisedDeliveryReasonCode::AwaitingApproval,
                "supervised delivery requires explicit approved supervision with a named approver",
                Some(
                    "Grant explicit human approval and record the approver before preparing delivery artifacts.",
                ),
                approval_state,
            );
            self.record_delivery_rejection(&audit_mutation_id, &contract)?;
            return Ok(contract);
        }

        let Some(task_class) = outcome.task_class.as_ref() else {
            let contract = supervised_delivery_denied_contract(
                request,
                SupervisedDeliveryReasonCode::UnsupportedTaskScope,
                "supervised delivery rejected because the executed task has no bounded task class",
                Some(
                    "Execute a bounded docs-scoped supervised task before preparing branch and PR artifacts.",
                ),
                approval_state,
            );
            self.record_delivery_rejection(&audit_mutation_id, &contract)?;
            return Ok(contract);
        };

        if !matches!(outcome.status, SupervisedDevloopStatus::Executed) {
            let contract = supervised_delivery_denied_contract(
                request,
                SupervisedDeliveryReasonCode::InconsistentDeliveryEvidence,
                "supervised delivery rejected because execution did not complete successfully",
                Some(
                    "Only prepare delivery artifacts from a successfully executed supervised devloop outcome.",
                ),
                approval_state,
            );
            self.record_delivery_rejection(&audit_mutation_id, &contract)?;
            return Ok(contract);
        }

        let Some(feedback) = outcome.execution_feedback.as_ref() else {
            let contract = supervised_delivery_denied_contract(
                request,
                SupervisedDeliveryReasonCode::DeliveryEvidenceMissing,
                "supervised delivery rejected because execution feedback is missing",
                Some(
                    "Re-run supervised execution and retain validation evidence before preparing delivery artifacts.",
                ),
                approval_state,
            );
            self.record_delivery_rejection(&audit_mutation_id, &contract)?;
            return Ok(contract);
        };

        if !feedback.accepted {
            let contract = supervised_delivery_denied_contract(
                request,
                SupervisedDeliveryReasonCode::ValidationEvidenceMissing,
                "supervised delivery rejected because execution feedback is not accepted",
                Some(
                    "Resolve validation failures and only prepare delivery artifacts from accepted execution results.",
                ),
                approval_state,
            );
            self.record_delivery_rejection(&audit_mutation_id, &contract)?;
            return Ok(contract);
        }

        if validate_bounded_docs_files(&request.proposal.files).is_err()
            && validate_bounded_cargo_dep_files(&request.proposal.files).is_err()
            && validate_bounded_lint_files(&request.proposal.files).is_err()
        {
            let contract = supervised_delivery_denied_contract(
                request,
                SupervisedDeliveryReasonCode::UnsupportedTaskScope,
                "supervised delivery rejected because proposal files are outside the bounded docs policy",
                Some(
                    "Restrict delivery preparation to one to three docs/*.md files that were executed under supervision.",
                ),
                approval_state,
            );
            self.record_delivery_rejection(&audit_mutation_id, &contract)?;
            return Ok(contract);
        }

        let branch_name = supervised_delivery_branch_name(&request.task.id, task_class);
        let pr_title = supervised_delivery_pr_title(request);
        let pr_summary = supervised_delivery_pr_summary(request, outcome, feedback);
        let approver = request
            .approval
            .approver
            .as_deref()
            .unwrap_or("unknown approver");
        let delivery_summary = format!(
            "prepared bounded branch and PR artifacts for supervised task '{}' with approver {}",
            request.task.id, approver
        );
        let contract = SupervisedDeliveryContract {
            delivery_summary: delivery_summary.clone(),
            branch_name: Some(branch_name.clone()),
            pr_title: Some(pr_title.clone()),
            pr_summary: Some(pr_summary.clone()),
            delivery_status: SupervisedDeliveryStatus::Prepared,
            approval_state,
            reason_code: SupervisedDeliveryReasonCode::DeliveryPrepared,
            fail_closed: false,
            recovery_hint: None,
        };

        self.store
            .append_event(EvolutionEvent::DeliveryPrepared {
                task_id: request.task.id.clone(),
                branch_name,
                pr_title,
                pr_summary,
                delivery_summary,
                delivery_status: delivery_status_key(contract.delivery_status).to_string(),
                approval_state: delivery_approval_state_key(contract.approval_state).to_string(),
                reason_code: delivery_reason_code_key(contract.reason_code).to_string(),
            })
            .map_err(store_err)?;

        Ok(contract)
    }

    pub fn evaluate_self_evolution_acceptance_gate(
        &self,
        input: &SelfEvolutionAcceptanceGateInput,
    ) -> Result<SelfEvolutionAcceptanceGateContract, EvoKernelError> {
        let approval_evidence =
            self_evolution_approval_evidence(&input.proposal_contract, &input.supervised_request);
        let delivery_outcome = self_evolution_delivery_outcome(&input.delivery_contract);
        let reason_code_matrix = self_evolution_reason_code_matrix(input);

        let selection_candidate_class = match input.selection_decision.candidate_class.as_ref() {
            Some(candidate_class)
                if input.selection_decision.selected
                    && matches!(
                        input.selection_decision.reason_code,
                        Some(SelfEvolutionSelectionReasonCode::Accepted)
                    ) =>
            {
                candidate_class
            }
            _ => {
                let contract = acceptance_gate_fail_contract(
                    "acceptance gate rejected because selection evidence is missing or fail-closed",
                    SelfEvolutionAcceptanceGateReasonCode::MissingSelectionEvidence,
                    Some(
                        "Select an accepted bounded self-evolution candidate before evaluating the closed-loop gate.",
                    ),
                    approval_evidence,
                    delivery_outcome,
                    reason_code_matrix,
                );
                self.record_acceptance_gate_result(input, &contract)?;
                return Ok(contract);
            }
        };

        let proposal_scope = match input.proposal_contract.proposal_scope.as_ref() {
            Some(scope)
                if !input.proposal_contract.fail_closed
                    && matches!(
                        input.proposal_contract.reason_code,
                        MutationProposalContractReasonCode::Accepted
                    ) =>
            {
                scope
            }
            _ => {
                let contract = acceptance_gate_fail_contract(
                    "acceptance gate rejected because proposal evidence is missing or fail-closed",
                    SelfEvolutionAcceptanceGateReasonCode::MissingProposalEvidence,
                    Some(
                        "Prepare an accepted bounded mutation proposal before evaluating the closed-loop gate.",
                    ),
                    approval_evidence,
                    delivery_outcome,
                    reason_code_matrix,
                );
                self.record_acceptance_gate_result(input, &contract)?;
                return Ok(contract);
            }
        };

        if !input.proposal_contract.approval_required
            || !approval_evidence.approved
            || approval_evidence.approver.is_none()
            || !input
                .proposal_contract
                .expected_evidence
                .contains(&MutationProposalEvidence::HumanApproval)
        {
            let contract = acceptance_gate_fail_contract(
                "acceptance gate rejected because explicit approval evidence is incomplete",
                SelfEvolutionAcceptanceGateReasonCode::MissingApprovalEvidence,
                Some(
                    "Record explicit human approval with a named approver before evaluating the closed-loop gate.",
                ),
                approval_evidence,
                delivery_outcome,
                reason_code_matrix,
            );
            self.record_acceptance_gate_result(input, &contract)?;
            return Ok(contract);
        }

        let execution_feedback_accepted = input
            .execution_outcome
            .execution_feedback
            .as_ref()
            .is_some_and(|feedback| feedback.accepted);
        if !matches!(
            input.execution_outcome.status,
            SupervisedDevloopStatus::Executed
        ) || !matches!(
            input.execution_outcome.validation_outcome,
            SupervisedValidationOutcome::Passed
        ) || !execution_feedback_accepted
            || input.execution_outcome.reason_code.is_none()
        {
            let contract = acceptance_gate_fail_contract(
                "acceptance gate rejected because execution evidence is missing or fail-closed",
                SelfEvolutionAcceptanceGateReasonCode::MissingExecutionEvidence,
                Some(
                    "Run supervised execution to a validated accepted outcome before evaluating the closed-loop gate.",
                ),
                approval_evidence,
                delivery_outcome,
                reason_code_matrix,
            );
            self.record_acceptance_gate_result(input, &contract)?;
            return Ok(contract);
        }

        if input.delivery_contract.fail_closed
            || !matches!(
                input.delivery_contract.delivery_status,
                SupervisedDeliveryStatus::Prepared
            )
            || !matches!(
                input.delivery_contract.approval_state,
                SupervisedDeliveryApprovalState::Approved
            )
            || !matches!(
                input.delivery_contract.reason_code,
                SupervisedDeliveryReasonCode::DeliveryPrepared
            )
            || input.delivery_contract.branch_name.is_none()
            || input.delivery_contract.pr_title.is_none()
            || input.delivery_contract.pr_summary.is_none()
        {
            let contract = acceptance_gate_fail_contract(
                "acceptance gate rejected because delivery evidence is missing or fail-closed",
                SelfEvolutionAcceptanceGateReasonCode::MissingDeliveryEvidence,
                Some(
                    "Prepare bounded delivery artifacts successfully before evaluating the closed-loop gate.",
                ),
                approval_evidence,
                delivery_outcome,
                reason_code_matrix,
            );
            self.record_acceptance_gate_result(input, &contract)?;
            return Ok(contract);
        }

        let expected_evidence = [
            MutationProposalEvidence::HumanApproval,
            MutationProposalEvidence::BoundedScope,
            MutationProposalEvidence::ValidationPass,
            MutationProposalEvidence::ExecutionAudit,
        ];
        if proposal_scope.task_class != *selection_candidate_class
            || input.execution_outcome.task_class.as_ref() != Some(&proposal_scope.task_class)
            || proposal_scope.target_files != input.supervised_request.proposal.files
            || !expected_evidence
                .iter()
                .all(|evidence| input.proposal_contract.expected_evidence.contains(evidence))
            || !reason_code_matrix_consistent(&reason_code_matrix, &input.execution_outcome)
        {
            let contract = acceptance_gate_fail_contract(
                "acceptance gate rejected because stage reason codes or bounded evidence drifted across the closed-loop path",
                SelfEvolutionAcceptanceGateReasonCode::InconsistentReasonCodeMatrix,
                Some(
                    "Reconcile selection, proposal, execution, and delivery contracts so the bounded closed-loop evidence remains internally consistent.",
                ),
                approval_evidence,
                delivery_outcome,
                reason_code_matrix,
            );
            self.record_acceptance_gate_result(input, &contract)?;
            return Ok(contract);
        }

        let contract = SelfEvolutionAcceptanceGateContract {
            acceptance_gate_summary: format!(
                "accepted supervised closed-loop self-evolution task '{}' for issue #{} as internally consistent and auditable",
                input.supervised_request.task.id, input.selection_decision.issue_number
            ),
            audit_consistency_result: SelfEvolutionAuditConsistencyResult::Consistent,
            approval_evidence,
            delivery_outcome,
            reason_code_matrix,
            fail_closed: false,
            reason_code: SelfEvolutionAcceptanceGateReasonCode::Accepted,
            recovery_hint: None,
        };
        self.record_acceptance_gate_result(input, &contract)?;
        Ok(contract)
    }

    async fn supervised_devloop_replay_outcome(
        &self,
        run_id: &RunId,
        request: &SupervisedDevloopRequest,
        diff_payload: &str,
    ) -> Result<Option<ReplayFeedback>, EvoKernelError> {
        let selector_input = supervised_devloop_selector_input(request, diff_payload);
        let decision = self
            .replay_or_fallback_for_run(run_id, selector_input)
            .await?;
        Ok(Some(Self::replay_feedback_for_agent(
            &decision.detect_evidence.matched_signals,
            &decision,
        )))
    }

    /// Autonomous candidate intake: classify raw diagnostic signals without a
    /// caller‐supplied issue number, deduplicate across the batch, and return
    /// an [`AutonomousIntakeOutput`] with accepted and denied candidates.
    ///
    /// This is the entry point for `EVO26-AUTO-01` — it does **not** generate
    /// mutation proposals or trigger any task planning.
    pub fn discover_autonomous_candidates(
        &self,
        input: &AutonomousIntakeInput,
    ) -> AutonomousIntakeOutput {
        if input.raw_signals.is_empty() {
            let deny = deny_discovered_candidate(
                autonomous_dedupe_key(input.candidate_source, &input.raw_signals),
                input.candidate_source,
                Vec::new(),
                AutonomousIntakeReasonCode::UnknownFailClosed,
            );
            return AutonomousIntakeOutput {
                candidates: vec![deny],
                accepted_count: 0,
                denied_count: 1,
            };
        }

        let normalized = normalize_autonomous_signals(&input.raw_signals);
        let dedupe_key = autonomous_dedupe_key(input.candidate_source, &normalized);

        // Check for a duplicate inside the active evolution store window.
        if autonomous_is_duplicate_in_store(&self.store, &dedupe_key) {
            let deny = deny_discovered_candidate(
                dedupe_key,
                input.candidate_source,
                normalized,
                AutonomousIntakeReasonCode::DuplicateCandidate,
            );
            return AutonomousIntakeOutput {
                candidates: vec![deny],
                accepted_count: 0,
                denied_count: 1,
            };
        }

        let Some(candidate_class) =
            classify_autonomous_signals(input.candidate_source, &normalized)
        else {
            let reason = if normalized.is_empty() {
                AutonomousIntakeReasonCode::UnknownFailClosed
            } else {
                AutonomousIntakeReasonCode::AmbiguousSignal
            };
            let deny =
                deny_discovered_candidate(dedupe_key, input.candidate_source, normalized, reason);
            return AutonomousIntakeOutput {
                candidates: vec![deny],
                accepted_count: 0,
                denied_count: 1,
            };
        };

        let summary = format!(
            "autonomous candidate from {:?} ({:?}): {} signal(s)",
            input.candidate_source,
            candidate_class,
            normalized.len()
        );
        let candidate = accept_discovered_candidate(
            dedupe_key,
            input.candidate_source,
            candidate_class,
            normalized,
            Some(&summary),
        );
        AutonomousIntakeOutput {
            accepted_count: 1,
            denied_count: 0,
            candidates: vec![candidate],
        }
    }

    /// Bounded task planning for an autonomous candidate: assigns risk tier,
    /// feasibility score, validation budget, and expected evidence, then
    /// approves or denies the candidate for proposal generation.
    ///
    /// This is the `EVO26-AUTO-02` entry point. It does **not** generate a
    /// mutation proposal — it only produces an auditable `AutonomousTaskPlan`.
    pub fn plan_autonomous_candidate(&self, candidate: &DiscoveredCandidate) -> AutonomousTaskPlan {
        autonomous_plan_for_candidate(candidate)
    }

    /// Autonomous mutation proposal generation from an approved `AutonomousTaskPlan`.
    ///
    /// Generates a bounded, machine-readable `AutonomousMutationProposal` from an
    /// approved plan. Unapproved plans, missing scope, or weak evidence sets produce
    /// a denied fail-closed proposal.
    ///
    /// This is the `EVO26-AUTO-03` entry point. It does **not** execute the mutation.
    pub fn propose_autonomous_mutation(
        &self,
        plan: &AutonomousTaskPlan,
    ) -> AutonomousMutationProposal {
        autonomous_proposal_for_plan(plan)
    }

    pub fn select_self_evolution_candidate(
        &self,
        request: &SelfEvolutionCandidateIntakeRequest,
    ) -> Result<SelfEvolutionSelectionDecision, EvoKernelError> {
        let normalized_state = request.state.trim().to_ascii_lowercase();
        if normalized_state != "open" {
            let reason_code = if normalized_state == "closed" {
                SelfEvolutionSelectionReasonCode::IssueClosed
            } else {
                SelfEvolutionSelectionReasonCode::UnknownFailClosed
            };
            return Ok(reject_self_evolution_selection_decision(
                request.issue_number,
                reason_code,
                None,
                None,
            ));
        }

        let normalized_labels = normalized_selection_labels(&request.labels);
        if normalized_labels.contains("duplicate")
            || normalized_labels.contains("invalid")
            || normalized_labels.contains("wontfix")
        {
            return Ok(reject_self_evolution_selection_decision(
                request.issue_number,
                SelfEvolutionSelectionReasonCode::ExcludedByLabel,
                Some(&format!(
                    "self-evolution candidate rejected because issue #{} carries an excluded label",
                    request.issue_number
                )),
                None,
            ));
        }

        if !normalized_labels.contains("area/evolution") {
            return Ok(reject_self_evolution_selection_decision(
                request.issue_number,
                SelfEvolutionSelectionReasonCode::MissingEvolutionLabel,
                None,
                None,
            ));
        }

        if !normalized_labels.contains("type/feature") {
            return Ok(reject_self_evolution_selection_decision(
                request.issue_number,
                SelfEvolutionSelectionReasonCode::MissingFeatureLabel,
                None,
                None,
            ));
        }

        let Some(task_class) = classify_self_evolution_candidate_request(request) else {
            return Ok(reject_self_evolution_selection_decision(
                request.issue_number,
                SelfEvolutionSelectionReasonCode::UnsupportedCandidateScope,
                Some(&format!(
                    "self-evolution candidate rejected because issue #{} declares unsupported candidate scope",
                    request.issue_number
                )),
                None,
            ));
        };

        Ok(accept_self_evolution_selection_decision(
            request.issue_number,
            task_class,
            Some(&format!(
                "selected GitHub issue #{} for bounded self-evolution intake",
                request.issue_number
            )),
        ))
    }

    pub fn prepare_self_evolution_mutation_proposal(
        &self,
        request: &SelfEvolutionCandidateIntakeRequest,
    ) -> Result<SelfEvolutionMutationProposalContract, EvoKernelError> {
        let selection = self.select_self_evolution_candidate(request)?;
        let expected_evidence = default_mutation_proposal_expected_evidence();
        let validation_budget = mutation_proposal_validation_budget(&self.validation_plan);
        let proposal = AgentMutationProposal {
            intent: format!(
                "Resolve GitHub issue #{}: {}",
                request.issue_number,
                request.title.trim()
            ),
            files: request.candidate_hint_paths.clone(),
            expected_effect: format!(
                "Address bounded self-evolution candidate issue #{} within the approved docs scope",
                request.issue_number
            ),
        };

        if !selection.selected {
            return Ok(SelfEvolutionMutationProposalContract {
                mutation_proposal: proposal,
                proposal_scope: None,
                validation_budget,
                approval_required: true,
                expected_evidence,
                summary: format!(
                    "self-evolution mutation proposal rejected for GitHub issue #{}",
                    request.issue_number
                ),
                failure_reason: selection.failure_reason.clone(),
                recovery_hint: selection.recovery_hint.clone(),
                reason_code: proposal_reason_code_from_selection(&selection),
                fail_closed: true,
            });
        }

        if expected_evidence.is_empty() {
            return Ok(SelfEvolutionMutationProposalContract {
                mutation_proposal: proposal,
                proposal_scope: None,
                validation_budget,
                approval_required: true,
                expected_evidence,
                summary: format!(
                    "self-evolution mutation proposal rejected for GitHub issue #{} because expected evidence is missing",
                    request.issue_number
                ),
                failure_reason: Some(
                    "self-evolution mutation proposal rejected because expected evidence was not declared"
                        .to_string(),
                ),
                recovery_hint: Some(
                    "Declare the expected approval, validation, and audit evidence before retrying proposal preparation."
                        .to_string(),
                ),
                reason_code: MutationProposalContractReasonCode::ExpectedEvidenceMissing,
                fail_closed: true,
            });
        }

        match validate_bounded_docs_files(&request.candidate_hint_paths) {
            Ok(target_files) => Ok(SelfEvolutionMutationProposalContract {
                mutation_proposal: proposal,
                proposal_scope: selection.candidate_class.clone().map(|task_class| {
                    MutationProposalScope {
                        task_class,
                        target_files,
                    }
                }),
                validation_budget,
                approval_required: true,
                expected_evidence,
                summary: format!(
                    "self-evolution mutation proposal prepared for GitHub issue #{}",
                    request.issue_number
                ),
                failure_reason: None,
                recovery_hint: None,
                reason_code: MutationProposalContractReasonCode::Accepted,
                fail_closed: false,
            }),
            Err(reason_code) => Ok(SelfEvolutionMutationProposalContract {
                mutation_proposal: proposal,
                proposal_scope: None,
                validation_budget,
                approval_required: true,
                expected_evidence,
                summary: format!(
                    "self-evolution mutation proposal rejected for GitHub issue #{} due to invalid proposal scope",
                    request.issue_number
                ),
                failure_reason: Some(format!(
                    "self-evolution mutation proposal rejected because issue #{} declares an invalid bounded docs scope",
                    request.issue_number
                )),
                recovery_hint: Some(
                    "Restrict target files to one to three unique docs/*.md paths before retrying proposal preparation."
                        .to_string(),
                ),
                reason_code,
                fail_closed: true,
            }),
        }
    }

    fn supervised_devloop_mutation_proposal_contract(
        &self,
        request: &SupervisedDevloopRequest,
    ) -> SelfEvolutionMutationProposalContract {
        let validation_budget = mutation_proposal_validation_budget(&self.validation_plan);
        let expected_evidence = default_mutation_proposal_expected_evidence();

        if expected_evidence.is_empty() {
            return SelfEvolutionMutationProposalContract {
                mutation_proposal: request.proposal.clone(),
                proposal_scope: None,
                validation_budget,
                approval_required: true,
                expected_evidence,
                summary: format!(
                    "supervised devloop rejected task '{}' because expected evidence was not declared",
                    request.task.id
                ),
                failure_reason: Some(
                    "supervised devloop mutation proposal rejected because expected evidence was not declared"
                        .to_string(),
                ),
                recovery_hint: Some(
                    "Declare human approval, bounded scope, validation, and audit evidence before execution."
                        .to_string(),
                ),
                reason_code: MutationProposalContractReasonCode::ExpectedEvidenceMissing,
                fail_closed: true,
            };
        }

        if validation_budget.validation_timeout_ms > MUTATION_NEEDED_MAX_VALIDATION_BUDGET_MS {
            return SelfEvolutionMutationProposalContract {
                mutation_proposal: request.proposal.clone(),
                proposal_scope: supervised_devloop_mutation_proposal_scope(request).ok(),
                validation_budget: validation_budget.clone(),
                approval_required: true,
                expected_evidence,
                summary: format!(
                    "supervised devloop rejected task '{}' because the declared validation budget exceeds bounded policy",
                    request.task.id
                ),
                failure_reason: Some(format!(
                    "supervised devloop mutation proposal rejected because validation budget exceeds bounded policy (configured={}ms, max={}ms)",
                    validation_budget.validation_timeout_ms,
                    MUTATION_NEEDED_MAX_VALIDATION_BUDGET_MS
                )),
                recovery_hint: Some(
                    "Reduce the validation timeout budget to the bounded policy before execution."
                        .to_string(),
                ),
                reason_code: MutationProposalContractReasonCode::ValidationBudgetExceeded,
                fail_closed: true,
            };
        }

        match supervised_devloop_mutation_proposal_scope(request) {
            Ok(proposal_scope) => {
                if !matches!(
                    proposal_scope.task_class,
                    BoundedTaskClass::DocsSingleFile | BoundedTaskClass::DocsMultiFile
                ) {
                    return SelfEvolutionMutationProposalContract {
                        mutation_proposal: request.proposal.clone(),
                        proposal_scope: None,
                        validation_budget,
                        approval_required: true,
                        expected_evidence,
                        summary: format!(
                            "supervised devloop rejected task '{}' before execution because the task class is outside the current docs-only bounded scope",
                            request.task.id
                        ),
                        failure_reason: Some(format!(
                            "supervised devloop rejected task '{}' because it is an unsupported task outside the bounded scope",
                            request.task.id
                        )),
                        recovery_hint: Some(
                            "Restrict proposal files to one to three unique docs/*.md paths before execution."
                                .to_string(),
                        ),
                        reason_code: MutationProposalContractReasonCode::UnsupportedTaskClass,
                        fail_closed: true,
                    };
                }

                SelfEvolutionMutationProposalContract {
                    mutation_proposal: request.proposal.clone(),
                    proposal_scope: Some(proposal_scope),
                    validation_budget,
                    approval_required: true,
                    expected_evidence,
                    summary: format!(
                        "supervised devloop mutation proposal prepared for task '{}'",
                        request.task.id
                    ),
                    failure_reason: None,
                    recovery_hint: None,
                    reason_code: MutationProposalContractReasonCode::Accepted,
                    fail_closed: false,
                }
            }
            Err(reason_code) => {
                let failure_reason = match reason_code {
                    MutationProposalContractReasonCode::MissingTargetFiles => format!(
                        "supervised devloop rejected task '{}' because the mutation proposal does not declare any target files",
                        request.task.id
                    ),
                    MutationProposalContractReasonCode::UnsupportedTaskClass
                    | MutationProposalContractReasonCode::OutOfBoundsPath => format!(
                        "supervised devloop rejected task '{}' because it is an unsupported task outside the bounded scope",
                        request.task.id
                    ),
                    _ => format!(
                        "supervised devloop mutation proposal rejected before execution for task '{}'",
                        request.task.id
                    ),
                };
                SelfEvolutionMutationProposalContract {
                    mutation_proposal: request.proposal.clone(),
                    proposal_scope: None,
                    validation_budget,
                    approval_required: true,
                    expected_evidence,
                    summary: format!(
                        "supervised devloop rejected task '{}' before execution because the mutation proposal is malformed or out of bounds",
                        request.task.id
                    ),
                    failure_reason: Some(failure_reason),
                    recovery_hint: Some(
                        "Restrict proposal files to one to three unique docs/*.md paths before execution."
                            .to_string(),
                    ),
                    reason_code,
                    fail_closed: true,
                }
            }
        }
    }

    pub fn coordinate(&self, plan: CoordinationPlan) -> CoordinationResult {
        MultiAgentCoordinator::new().coordinate(plan)
    }

    pub fn export_promoted_assets(
        &self,
        sender_id: impl Into<String>,
    ) -> Result<EvolutionEnvelope, EvoKernelError> {
        let sender_id = sender_id.into();
        let envelope = export_promoted_assets_from_store(self.store.as_ref(), sender_id.clone())?;
        if !envelope.assets.is_empty() {
            let mut ledger = self
                .economics
                .lock()
                .map_err(|_| EvoKernelError::Validation("economics ledger lock poisoned".into()))?;
            if ledger
                .reserve_publish_stake(&sender_id, &self.stake_policy)
                .is_none()
            {
                return Err(EvoKernelError::Validation(
                    "insufficient EVU for remote publish".into(),
                ));
            }
        }
        Ok(envelope)
    }

    pub fn import_remote_envelope(
        &self,
        envelope: &EvolutionEnvelope,
    ) -> Result<ImportOutcome, EvoKernelError> {
        import_remote_envelope_into_store(
            self.store.as_ref(),
            envelope,
            Some(self.remote_publishers.as_ref()),
            None,
        )
    }

    pub fn fetch_assets(
        &self,
        responder_id: impl Into<String>,
        query: &FetchQuery,
    ) -> Result<FetchResponse, EvoKernelError> {
        fetch_assets_from_store(self.store.as_ref(), responder_id, query)
    }

    pub fn revoke_assets(&self, notice: &RevokeNotice) -> Result<RevokeNotice, EvoKernelError> {
        revoke_assets_in_store(self.store.as_ref(), notice)
    }

    pub async fn replay_or_fallback(
        &self,
        input: SelectorInput,
    ) -> Result<ReplayDecision, EvoKernelError> {
        let replay_run_id = next_id("replay");
        self.replay_or_fallback_for_run(&replay_run_id, input).await
    }

    pub async fn replay_or_fallback_for_run(
        &self,
        run_id: &RunId,
        input: SelectorInput,
    ) -> Result<ReplayDecision, EvoKernelError> {
        let executor = StoreReplayExecutor {
            sandbox: self.sandbox.clone(),
            validator: self.validator.clone(),
            store: self.store.clone(),
            selector: self.selector.clone(),
            governor: self.governor.clone(),
            economics: Some(self.economics.clone()),
            remote_publishers: Some(self.remote_publishers.clone()),
            stake_policy: self.stake_policy.clone(),
        };
        executor
            .try_replay_for_run(run_id, &input, &self.sandbox_policy, &self.validation_plan)
            .await
            .map_err(|err| EvoKernelError::Validation(err.to_string()))
    }

    pub fn economics_signal(&self, node_id: &str) -> Option<EconomicsSignal> {
        self.economics.lock().ok()?.governor_signal(node_id)
    }

    pub fn selector_reputation_bias(&self) -> BTreeMap<String, f32> {
        self.economics
            .lock()
            .ok()
            .map(|locked| locked.selector_reputation_bias())
            .unwrap_or_default()
    }

    pub fn metrics_snapshot(&self) -> Result<EvolutionMetricsSnapshot, EvoKernelError> {
        evolution_metrics_snapshot(self.store.as_ref())
    }

    pub fn replay_roi_release_gate_summary(
        &self,
        window_seconds: u64,
    ) -> Result<ReplayRoiWindowSummary, EvoKernelError> {
        replay_roi_release_gate_summary(self.store.as_ref(), window_seconds)
    }

    pub fn render_replay_roi_release_gate_summary_json(
        &self,
        window_seconds: u64,
    ) -> Result<String, EvoKernelError> {
        let summary = self.replay_roi_release_gate_summary(window_seconds)?;
        serde_json::to_string_pretty(&summary)
            .map_err(|err| EvoKernelError::Validation(err.to_string()))
    }

    pub fn replay_roi_release_gate_contract(
        &self,
        window_seconds: u64,
        thresholds: ReplayRoiReleaseGateThresholds,
    ) -> Result<ReplayRoiReleaseGateContract, EvoKernelError> {
        let summary = self.replay_roi_release_gate_summary(window_seconds)?;
        Ok(replay_roi_release_gate_contract(&summary, thresholds))
    }

    pub fn render_replay_roi_release_gate_contract_json(
        &self,
        window_seconds: u64,
        thresholds: ReplayRoiReleaseGateThresholds,
    ) -> Result<String, EvoKernelError> {
        let contract = self.replay_roi_release_gate_contract(window_seconds, thresholds)?;
        serde_json::to_string_pretty(&contract)
            .map_err(|err| EvoKernelError::Validation(err.to_string()))
    }

    pub fn render_metrics_prometheus(&self) -> Result<String, EvoKernelError> {
        self.metrics_snapshot().map(|snapshot| {
            let health = evolution_health_snapshot(&snapshot);
            render_evolution_metrics_prometheus(&snapshot, &health)
        })
    }

    pub fn health_snapshot(&self) -> Result<EvolutionHealthSnapshot, EvoKernelError> {
        self.metrics_snapshot()
            .map(|snapshot| evolution_health_snapshot(&snapshot))
    }
}

pub fn prepare_mutation(
    intent: MutationIntent,
    diff_payload: String,
    base_revision: Option<String>,
) -> PreparedMutation {
    PreparedMutation {
        intent,
        artifact: MutationArtifact {
            encoding: ArtifactEncoding::UnifiedDiff,
            content_hash: compute_artifact_hash(&diff_payload),
            payload: diff_payload,
            base_revision,
        },
    }
}

pub fn prepare_mutation_from_spec(
    plan: CompiledMutationPlan,
    diff_payload: String,
    base_revision: Option<String>,
) -> PreparedMutation {
    prepare_mutation(plan.mutation_intent, diff_payload, base_revision)
}

pub fn default_evolution_store() -> Arc<dyn EvolutionStore> {
    Arc::new(oris_evolution::JsonlEvolutionStore::new(
        default_store_root(),
    ))
}

fn built_in_seed_templates() -> Vec<SeedTemplate> {
    vec![
        SeedTemplate {
            id: "bootstrap-readme".into(),
            intent: "Seed a baseline README recovery pattern".into(),
            signals: vec!["bootstrap readme".into(), "missing readme".into()],
            diff_payload: "\
diff --git a/README.md b/README.md
new file mode 100644
index 0000000..1111111
--- /dev/null
+++ b/README.md
@@ -0,0 +1,3 @@
+# Oris
+Bootstrap documentation seed
+"
            .into(),
            validation_profile: "bootstrap-seed".into(),
        },
        SeedTemplate {
            id: "bootstrap-test-fix".into(),
            intent: "Seed a deterministic test stabilization pattern".into(),
            signals: vec!["bootstrap test fix".into(), "failing tests".into()],
            diff_payload: "\
diff --git a/src/lib.rs b/src/lib.rs
index 1111111..2222222 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1 +1,2 @@
 pub fn demo() -> usize { 1 }
+pub fn normalize_test_output() -> bool { true }
"
            .into(),
            validation_profile: "bootstrap-seed".into(),
        },
        SeedTemplate {
            id: "bootstrap-refactor".into(),
            intent: "Seed a low-risk refactor capsule".into(),
            signals: vec!["bootstrap refactor".into(), "small refactor".into()],
            diff_payload: "\
diff --git a/src/lib.rs b/src/lib.rs
index 2222222..3333333 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1 +1,3 @@
 pub fn demo() -> usize { 1 }
+
+fn extract_strategy_key(input: &str) -> &str { input }
"
            .into(),
            validation_profile: "bootstrap-seed".into(),
        },
        SeedTemplate {
            id: "bootstrap-logging".into(),
            intent: "Seed a baseline structured logging mutation".into(),
            signals: vec!["bootstrap logging".into(), "structured logs".into()],
            diff_payload: "\
diff --git a/src/lib.rs b/src/lib.rs
index 3333333..4444444 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1 +1,3 @@
 pub fn demo() -> usize { 1 }
+
+fn emit_bootstrap_log() { println!(\"bootstrap-log\"); }
"
            .into(),
            validation_profile: "bootstrap-seed".into(),
        },
    ]
}

fn build_seed_mutation(template: &SeedTemplate) -> PreparedMutation {
    let changed_files = seed_changed_files(&template.diff_payload);
    let target = if changed_files.is_empty() {
        MutationTarget::WorkspaceRoot
    } else {
        MutationTarget::Paths {
            allow: changed_files,
        }
    };
    prepare_mutation(
        MutationIntent {
            id: stable_hash_json(&("bootstrap-mutation", &template.id))
                .unwrap_or_else(|_| format!("bootstrap-mutation-{}", template.id)),
            intent: template.intent.clone(),
            target,
            expected_effect: format!("seed {}", template.id),
            risk: RiskLevel::Low,
            signals: template.signals.clone(),
            spec_id: None,
        },
        template.diff_payload.clone(),
        None,
    )
}

fn extract_seed_signals(template: &SeedTemplate) -> SignalExtractionOutput {
    let mut signals = BTreeSet::new();
    for declared in &template.signals {
        if let Some(phrase) = normalize_signal_phrase(declared) {
            signals.insert(phrase);
        }
        extend_signal_tokens(&mut signals, declared);
    }
    extend_signal_tokens(&mut signals, &template.intent);
    extend_signal_tokens(&mut signals, &template.diff_payload);
    for changed_file in seed_changed_files(&template.diff_payload) {
        extend_signal_tokens(&mut signals, &changed_file);
    }
    let values = signals.into_iter().take(32).collect::<Vec<_>>();
    let hash =
        stable_hash_json(&values).unwrap_or_else(|_| compute_artifact_hash(&values.join("\n")));
    SignalExtractionOutput { values, hash }
}

fn seed_changed_files(diff_payload: &str) -> Vec<String> {
    let mut changed_files = BTreeSet::new();
    for line in diff_payload.lines() {
        if let Some(path) = line.strip_prefix("+++ b/") {
            let normalized = path.trim();
            if !normalized.is_empty() {
                changed_files.insert(normalized.to_string());
            }
        }
    }
    changed_files.into_iter().collect()
}

fn build_bootstrap_gene(
    template: &SeedTemplate,
    extracted: &SignalExtractionOutput,
) -> Result<Gene, EvolutionError> {
    let mut strategy = vec![template.id.clone(), "bootstrap".into()];
    let (task_class_id, task_label) = replay_task_descriptor(&extracted.values);
    ensure_strategy_metadata(&mut strategy, "task_class", &task_class_id);
    ensure_strategy_metadata(&mut strategy, "task_label", &task_label);
    let id = stable_hash_json(&(
        "bootstrap-gene",
        &template.id,
        &extracted.values,
        &template.validation_profile,
    ))?;
    Ok(Gene {
        id,
        signals: extracted.values.clone(),
        strategy,
        validation: vec![template.validation_profile.clone()],
        state: AssetState::Quarantined,
        task_class_id: None,
    })
}

fn build_bootstrap_capsule(
    run_id: &RunId,
    template: &SeedTemplate,
    mutation: &PreparedMutation,
    gene: &Gene,
) -> Result<Capsule, EvolutionError> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());
    let env = current_env_fingerprint(&cwd);
    let diff_hash = mutation.artifact.content_hash.clone();
    let changed_files = seed_changed_files(&template.diff_payload);
    let validator_hash = stable_hash_json(&(
        "bootstrap-validator",
        &template.id,
        &template.validation_profile,
        &diff_hash,
    ))?;
    let id = stable_hash_json(&(
        "bootstrap-capsule",
        &template.id,
        run_id,
        &gene.id,
        &diff_hash,
        &env,
    ))?;
    Ok(Capsule {
        id,
        gene_id: gene.id.clone(),
        mutation_id: mutation.intent.id.clone(),
        run_id: run_id.clone(),
        diff_hash,
        confidence: 0.0,
        env,
        outcome: Outcome {
            success: false,
            validation_profile: template.validation_profile.clone(),
            validation_duration_ms: 0,
            changed_files,
            validator_hash,
            lines_changed: compute_blast_radius(&template.diff_payload).lines_changed,
            replay_verified: false,
        },
        state: AssetState::Quarantined,
    })
}

fn derive_gene(
    mutation: &PreparedMutation,
    receipt: &SandboxReceipt,
    validation_profile: &str,
    extracted_signals: &[String],
) -> Gene {
    let mut strategy = BTreeSet::new();
    for file in &receipt.changed_files {
        if let Some(component) = file.components().next() {
            strategy.insert(component.as_os_str().to_string_lossy().to_string());
        }
    }
    for token in mutation
        .artifact
        .payload
        .split(|ch: char| !ch.is_ascii_alphanumeric())
    {
        if token.len() == 5
            && token.starts_with('E')
            && token[1..].chars().all(|ch| ch.is_ascii_digit())
        {
            strategy.insert(token.to_string());
        }
    }
    for token in mutation.intent.intent.split_whitespace().take(8) {
        strategy.insert(token.to_ascii_lowercase());
    }
    let mut strategy = strategy.into_iter().collect::<Vec<_>>();
    let descriptor_signals = if mutation
        .intent
        .signals
        .iter()
        .any(|signal| normalize_signal_phrase(signal).is_some())
    {
        mutation.intent.signals.as_slice()
    } else {
        extracted_signals
    };
    let (task_class_id, task_label) = replay_task_descriptor(descriptor_signals);
    ensure_strategy_metadata(&mut strategy, "task_class", &task_class_id);
    ensure_strategy_metadata(&mut strategy, "task_label", &task_label);
    let id = stable_hash_json(&(extracted_signals, &strategy, validation_profile))
        .unwrap_or_else(|_| next_id("gene"));
    Gene {
        id,
        signals: extracted_signals.to_vec(),
        strategy,
        validation: vec![validation_profile.to_string()],
        state: AssetState::Promoted,
        task_class_id: None,
    }
}

fn build_capsule(
    run_id: &RunId,
    mutation: &PreparedMutation,
    receipt: &SandboxReceipt,
    report: &ValidationReport,
    validation_profile: &str,
    gene: &Gene,
    blast_radius: &BlastRadius,
) -> Result<Capsule, EvolutionError> {
    let env = current_env_fingerprint(&receipt.workdir);
    let validator_hash = stable_hash_json(report)?;
    let diff_hash = mutation.artifact.content_hash.clone();
    let id = stable_hash_json(&(run_id, &gene.id, &diff_hash, &mutation.intent.id))?;
    Ok(Capsule {
        id,
        gene_id: gene.id.clone(),
        mutation_id: mutation.intent.id.clone(),
        run_id: run_id.clone(),
        diff_hash,
        confidence: 0.7,
        env,
        outcome: oris_evolution::Outcome {
            success: true,
            validation_profile: validation_profile.to_string(),
            validation_duration_ms: report.duration_ms,
            changed_files: receipt
                .changed_files
                .iter()
                .map(|path| path.to_string_lossy().to_string())
                .collect(),
            validator_hash,
            lines_changed: blast_radius.lines_changed,
            replay_verified: false,
        },
        state: AssetState::Promoted,
    })
}

fn current_env_fingerprint(workdir: &Path) -> EnvFingerprint {
    let rustc_version = Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_else(|| "rustc unknown".into());
    let cargo_lock_hash = fs::read(workdir.join("Cargo.lock"))
        .ok()
        .map(|bytes| {
            let value = String::from_utf8_lossy(&bytes);
            compute_artifact_hash(&value)
        })
        .unwrap_or_else(|| "missing-cargo-lock".into());
    let target_triple = format!(
        "{}-unknown-{}",
        std::env::consts::ARCH,
        std::env::consts::OS
    );
    EnvFingerprint {
        rustc_version,
        cargo_lock_hash,
        target_triple,
        os: std::env::consts::OS.to_string(),
    }
}

fn extend_signal_tokens(out: &mut BTreeSet<String>, input: &str) {
    for raw in input.split(|ch: char| !ch.is_ascii_alphanumeric()) {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let normalized = if is_rust_error_code(trimmed) {
            let mut chars = trimmed.chars();
            let prefix = chars
                .next()
                .map(|ch| ch.to_ascii_uppercase())
                .unwrap_or('E');
            format!("{prefix}{}", chars.as_str())
        } else {
            trimmed.to_ascii_lowercase()
        };
        if normalized.len() < 3 {
            continue;
        }
        out.insert(normalized);
    }
}

fn normalize_signal_phrase(input: &str) -> Option<String> {
    let mut seen = BTreeSet::new();
    let mut normalized_tokens = Vec::new();
    for raw in input.split(|ch: char| !ch.is_ascii_alphanumeric()) {
        let Some(token) = canonical_replay_signal_token(raw) else {
            continue;
        };
        if seen.insert(token.clone()) {
            normalized_tokens.push(token);
        }
    }
    let normalized = normalized_tokens.join(" ");
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn canonical_replay_signal_token(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let normalized = if is_rust_error_code(trimmed) {
        let mut chars = trimmed.chars();
        let prefix = chars
            .next()
            .map(|ch| ch.to_ascii_uppercase())
            .unwrap_or('E');
        format!("{prefix}{}", chars.as_str())
    } else {
        trimmed.to_ascii_lowercase()
    };
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

fn replay_task_descriptor(signals: &[String]) -> (String, String) {
    let normalized = signals
        .iter()
        .filter_map(|signal| normalize_signal_phrase(signal))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if normalized.is_empty() {
        return ("unknown".into(), "unknown".into());
    }
    let task_label = normalized
        .iter()
        .filter(|value| !is_validation_summary_phrase(value))
        .max_by_key(|value| {
            let token_count = value.split_whitespace().count();
            (
                value.chars().any(|ch| ch.is_ascii_alphabetic()),
                token_count >= 2,
                token_count,
                value.len(),
            )
        })
        .cloned()
        .unwrap_or_else(|| normalized[0].clone());
    let task_class_id = stable_hash_json(&normalized)
        .unwrap_or_else(|_| compute_artifact_hash(&normalized.join("\n")));
    (task_class_id, task_label)
}

fn is_validation_summary_phrase(value: &str) -> bool {
    let tokens = value.split_whitespace().collect::<BTreeSet<_>>();
    tokens == BTreeSet::from(["validation", "passed"])
        || tokens == BTreeSet::from(["validation", "failed"])
}

fn normalized_signal_values(signals: &[String]) -> Vec<String> {
    signals
        .iter()
        .filter_map(|signal| normalize_signal_phrase(signal))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
}

fn matched_replay_signals(input_signals: &[String], candidate_signals: &[String]) -> Vec<String> {
    let normalized_input = normalized_signal_values(input_signals);
    if normalized_input.is_empty() {
        return Vec::new();
    }
    let normalized_candidate = normalized_signal_values(candidate_signals);
    if normalized_candidate.is_empty() {
        return normalized_input;
    }
    let matched = normalized_input
        .iter()
        .filter(|signal| {
            normalized_candidate
                .iter()
                .any(|candidate| candidate.contains(signal.as_str()) || signal.contains(candidate))
        })
        .cloned()
        .collect::<Vec<_>>();
    if matched.is_empty() {
        normalized_input
    } else {
        matched
    }
}

fn replay_detect_evidence_from_input(input: &SelectorInput) -> ReplayDetectEvidence {
    let (task_class_id, task_label) = replay_task_descriptor(&input.signals);
    ReplayDetectEvidence {
        task_class_id,
        task_label,
        matched_signals: normalized_signal_values(&input.signals),
        mismatch_reasons: Vec::new(),
    }
}

fn replay_descriptor_from_candidate_or_input(
    candidate: Option<&GeneCandidate>,
    input: &SelectorInput,
) -> (String, String) {
    if let Some(candidate) = candidate {
        let task_class_id = strategy_metadata_value(&candidate.gene.strategy, "task_class");
        let task_label = strategy_metadata_value(&candidate.gene.strategy, "task_label");
        if let Some(task_class_id) = task_class_id {
            return (
                task_class_id.clone(),
                task_label.unwrap_or_else(|| task_class_id.clone()),
            );
        }
        return replay_task_descriptor(&candidate.gene.signals);
    }
    replay_task_descriptor(&input.signals)
}

fn estimated_reasoning_tokens(signals: &[String]) -> u64 {
    let normalized = signals
        .iter()
        .filter_map(|signal| normalize_signal_phrase(signal))
        .collect::<BTreeSet<_>>();
    let signal_count = normalized.len() as u64;
    REPLAY_REASONING_TOKEN_FLOOR + REPLAY_REASONING_TOKEN_SIGNAL_WEIGHT * signal_count.max(1)
}

fn compute_replay_roi(reasoning_avoided_tokens: u64, replay_fallback_cost: u64) -> f64 {
    let total = reasoning_avoided_tokens + replay_fallback_cost;
    if total == 0 {
        return 0.0;
    }
    (reasoning_avoided_tokens as f64 - replay_fallback_cost as f64) / total as f64
}

fn is_rust_error_code(value: &str) -> bool {
    value.len() == 5
        && matches!(value.as_bytes().first(), Some(b'e') | Some(b'E'))
        && value[1..].chars().all(|ch| ch.is_ascii_digit())
}

fn supervised_execution_decision_from_status(
    status: SupervisedDevloopStatus,
) -> SupervisedExecutionDecision {
    match status {
        SupervisedDevloopStatus::AwaitingApproval => SupervisedExecutionDecision::AwaitingApproval,
        SupervisedDevloopStatus::RejectedByPolicy => SupervisedExecutionDecision::RejectedByPolicy,
        SupervisedDevloopStatus::FailedClosed => SupervisedExecutionDecision::FailedClosed,
        SupervisedDevloopStatus::Executed => SupervisedExecutionDecision::PlannerFallback,
    }
}

fn supervised_validation_outcome_from_status(
    status: SupervisedDevloopStatus,
) -> SupervisedValidationOutcome {
    match status {
        SupervisedDevloopStatus::AwaitingApproval | SupervisedDevloopStatus::RejectedByPolicy => {
            SupervisedValidationOutcome::NotRun
        }
        SupervisedDevloopStatus::FailedClosed => SupervisedValidationOutcome::FailedClosed,
        SupervisedDevloopStatus::Executed => SupervisedValidationOutcome::Passed,
    }
}

fn supervised_reason_code_from_mutation_needed(
    reason_code: MutationNeededFailureReasonCode,
) -> SupervisedExecutionReasonCode {
    match reason_code {
        MutationNeededFailureReasonCode::PolicyDenied => {
            SupervisedExecutionReasonCode::PolicyDenied
        }
        MutationNeededFailureReasonCode::ValidationFailed => {
            SupervisedExecutionReasonCode::ValidationFailed
        }
        MutationNeededFailureReasonCode::UnsafePatch => SupervisedExecutionReasonCode::UnsafePatch,
        MutationNeededFailureReasonCode::Timeout => SupervisedExecutionReasonCode::Timeout,
        MutationNeededFailureReasonCode::MutationPayloadMissing => {
            SupervisedExecutionReasonCode::MutationPayloadMissing
        }
        MutationNeededFailureReasonCode::UnknownFailClosed => {
            SupervisedExecutionReasonCode::UnknownFailClosed
        }
    }
}

fn supervised_execution_evidence_summary(
    decision: SupervisedExecutionDecision,
    task_class: Option<&BoundedTaskClass>,
    validation_outcome: SupervisedValidationOutcome,
    fallback_reason: Option<&str>,
    reason_code: Option<&str>,
) -> String {
    let mut parts = vec![
        format!("decision={decision:?}"),
        format!("validation={validation_outcome:?}"),
        format!(
            "task_class={}",
            task_class
                .map(|value| format!("{value:?}"))
                .unwrap_or_else(|| "none".to_string())
        ),
    ];
    if let Some(reason_code) = reason_code {
        parts.push(format!("reason_code={reason_code}"));
    }
    if let Some(fallback_reason) = fallback_reason {
        parts.push(format!("fallback_reason={fallback_reason}"));
    }
    parts.join("; ")
}

fn supervised_devloop_selector_input(
    request: &SupervisedDevloopRequest,
    diff_payload: &str,
) -> SelectorInput {
    let extracted = extract_deterministic_signals(&SignalExtractionInput {
        patch_diff: diff_payload.to_string(),
        intent: request.proposal.intent.clone(),
        expected_effect: request.proposal.expected_effect.clone(),
        declared_signals: vec![
            request.proposal.intent.clone(),
            request.proposal.expected_effect.clone(),
        ],
        changed_files: request.proposal.files.clone(),
        validation_success: true,
        validation_logs: String::new(),
        stage_outputs: Vec::new(),
    });
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    SelectorInput {
        signals: extracted.values,
        env: current_env_fingerprint(&cwd),
        spec_id: None,
        limit: 1,
    }
}

fn supervised_devloop_fail_closed_contract_from_replay(
    replay_feedback: &ReplayFeedback,
) -> Option<MutationNeededFailureContract> {
    let reason_code = replay_feedback.reason_code?;
    let failure_reason = replay_feedback
        .fallback_reason
        .as_deref()
        .unwrap_or("replay-assisted supervised execution failed closed");
    match reason_code {
        ReplayFallbackReasonCode::NoCandidateAfterSelect
        | ReplayFallbackReasonCode::ScoreBelowThreshold
        | ReplayFallbackReasonCode::CandidateHasNoCapsule => None,
        ReplayFallbackReasonCode::MutationPayloadMissing => {
            Some(normalize_mutation_needed_failure_contract(
                Some(failure_reason),
                Some(MutationNeededFailureReasonCode::MutationPayloadMissing),
            ))
        }
        ReplayFallbackReasonCode::PatchApplyFailed => {
            Some(normalize_mutation_needed_failure_contract(
                Some(failure_reason),
                Some(MutationNeededFailureReasonCode::UnsafePatch),
            ))
        }
        ReplayFallbackReasonCode::ValidationFailed => {
            Some(normalize_mutation_needed_failure_contract(
                Some(failure_reason),
                Some(MutationNeededFailureReasonCode::ValidationFailed),
            ))
        }
        ReplayFallbackReasonCode::UnmappedFallbackReason => {
            Some(normalize_mutation_needed_failure_contract(
                Some(failure_reason),
                Some(MutationNeededFailureReasonCode::UnknownFailClosed),
            ))
        }
    }
}

fn validation_plan_timeout_budget_ms(plan: &ValidationPlan) -> u64 {
    plan.stages.iter().fold(0_u64, |acc, stage| match stage {
        ValidationStage::Command { timeout_ms, .. } => acc.saturating_add(*timeout_ms),
    })
}

fn mutation_needed_reason_code_key(reason_code: MutationNeededFailureReasonCode) -> &'static str {
    match reason_code {
        MutationNeededFailureReasonCode::PolicyDenied => "policy_denied",
        MutationNeededFailureReasonCode::ValidationFailed => "validation_failed",
        MutationNeededFailureReasonCode::UnsafePatch => "unsafe_patch",
        MutationNeededFailureReasonCode::Timeout => "timeout",
        MutationNeededFailureReasonCode::MutationPayloadMissing => "mutation_payload_missing",
        MutationNeededFailureReasonCode::UnknownFailClosed => "unknown_fail_closed",
    }
}

fn mutation_needed_status_from_reason_code(
    reason_code: MutationNeededFailureReasonCode,
) -> SupervisedDevloopStatus {
    if matches!(reason_code, MutationNeededFailureReasonCode::PolicyDenied) {
        SupervisedDevloopStatus::RejectedByPolicy
    } else {
        SupervisedDevloopStatus::FailedClosed
    }
}

fn mutation_needed_contract_for_validation_failure(
    profile: &str,
    report: &ValidationReport,
) -> MutationNeededFailureContract {
    let lower_logs = report.logs.to_ascii_lowercase();
    if lower_logs.contains("timed out") {
        normalize_mutation_needed_failure_contract(
            Some(&format!(
                "mutation-needed validation command timed out under profile '{profile}'"
            )),
            Some(MutationNeededFailureReasonCode::Timeout),
        )
    } else {
        normalize_mutation_needed_failure_contract(
            Some(&format!(
                "mutation-needed validation failed under profile '{profile}'"
            )),
            Some(MutationNeededFailureReasonCode::ValidationFailed),
        )
    }
}

fn mutation_needed_contract_for_error_message(message: &str) -> MutationNeededFailureContract {
    let reason_code = infer_mutation_needed_failure_reason_code(message);
    normalize_mutation_needed_failure_contract(Some(message), reason_code)
}

fn mutation_needed_audit_mutation_id(request: &SupervisedDevloopRequest) -> String {
    stable_hash_json(&(
        "mutation-needed-audit",
        &request.task.id,
        &request.proposal.intent,
        &request.proposal.files,
    ))
    .map(|hash| format!("mutation-needed-{hash}"))
    .unwrap_or_else(|_| format!("mutation-needed-{}", request.task.id))
}

fn supervised_delivery_approval_state(
    approval: &oris_agent_contract::HumanApproval,
) -> SupervisedDeliveryApprovalState {
    if approval.approved
        && approval
            .approver
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
    {
        SupervisedDeliveryApprovalState::Approved
    } else {
        SupervisedDeliveryApprovalState::MissingExplicitApproval
    }
}

fn supervised_delivery_denied_contract(
    request: &SupervisedDevloopRequest,
    reason_code: SupervisedDeliveryReasonCode,
    failure_reason: &str,
    recovery_hint: Option<&str>,
    approval_state: SupervisedDeliveryApprovalState,
) -> SupervisedDeliveryContract {
    SupervisedDeliveryContract {
        delivery_summary: format!(
            "supervised delivery denied for task '{}' [{}]: {}",
            request.task.id,
            delivery_reason_code_key(reason_code),
            failure_reason
        ),
        branch_name: None,
        pr_title: None,
        pr_summary: None,
        delivery_status: SupervisedDeliveryStatus::Denied,
        approval_state,
        reason_code,
        fail_closed: true,
        recovery_hint: recovery_hint.map(|value| value.to_string()),
    }
}

fn supervised_delivery_branch_name(task_id: &str, task_class: &BoundedTaskClass) -> String {
    let prefix = match task_class {
        BoundedTaskClass::DocsSingleFile => "self-evolution/docs",
        BoundedTaskClass::DocsMultiFile => "self-evolution/docs-batch",
        BoundedTaskClass::CargoDepUpgrade => "self-evolution/dep-upgrade",
        BoundedTaskClass::LintFix => "self-evolution/lint-fix",
    };
    let slug = sanitize_delivery_component(task_id);
    truncate_delivery_field(&format!("{prefix}/{slug}"), 72)
}

fn supervised_delivery_pr_title(request: &SupervisedDevloopRequest) -> String {
    truncate_delivery_field(
        &format!("[self-evolution] {}", request.task.description.trim()),
        96,
    )
}

fn supervised_delivery_pr_summary(
    request: &SupervisedDevloopRequest,
    outcome: &SupervisedDevloopOutcome,
    feedback: &ExecutionFeedback,
) -> String {
    let files = request.proposal.files.join(", ");
    let approval_note = request.approval.note.as_deref().unwrap_or("none recorded");
    truncate_delivery_field(
        &format!(
            "task_id={}\nstatus={:?}\nfiles={}\nvalidation_summary={}\napproval_note={}",
            request.task.id, outcome.status, files, feedback.summary, approval_note
        ),
        600,
    )
}

fn sanitize_delivery_component(value: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in value.chars() {
        let normalized = if ch.is_ascii_alphanumeric() {
            last_dash = false;
            ch.to_ascii_lowercase()
        } else {
            if last_dash {
                continue;
            }
            last_dash = true;
            '-'
        };
        out.push(normalized);
    }
    out.trim_matches('-').chars().take(48).collect()
}

fn truncate_delivery_field(value: &str, max_chars: usize) -> String {
    let truncated = value.chars().take(max_chars).collect::<String>();
    if truncated.is_empty() {
        "delivery-artifact".to_string()
    } else {
        truncated
    }
}

fn delivery_reason_code_key(reason_code: SupervisedDeliveryReasonCode) -> &'static str {
    match reason_code {
        SupervisedDeliveryReasonCode::DeliveryPrepared => "delivery_prepared",
        SupervisedDeliveryReasonCode::AwaitingApproval => "awaiting_approval",
        SupervisedDeliveryReasonCode::DeliveryEvidenceMissing => "delivery_evidence_missing",
        SupervisedDeliveryReasonCode::ValidationEvidenceMissing => "validation_evidence_missing",
        SupervisedDeliveryReasonCode::UnsupportedTaskScope => "unsupported_task_scope",
        SupervisedDeliveryReasonCode::InconsistentDeliveryEvidence => {
            "inconsistent_delivery_evidence"
        }
        SupervisedDeliveryReasonCode::UnknownFailClosed => "unknown_fail_closed",
    }
}

fn delivery_status_key(status: SupervisedDeliveryStatus) -> &'static str {
    match status {
        SupervisedDeliveryStatus::Prepared => "prepared",
        SupervisedDeliveryStatus::Denied => "denied",
    }
}

fn delivery_approval_state_key(state: SupervisedDeliveryApprovalState) -> &'static str {
    match state {
        SupervisedDeliveryApprovalState::Approved => "approved",
        SupervisedDeliveryApprovalState::MissingExplicitApproval => "missing_explicit_approval",
    }
}

fn self_evolution_approval_evidence(
    proposal_contract: &SelfEvolutionMutationProposalContract,
    request: &SupervisedDevloopRequest,
) -> SelfEvolutionApprovalEvidence {
    SelfEvolutionApprovalEvidence {
        approval_required: proposal_contract.approval_required,
        approved: request.approval.approved,
        approver: non_empty_owned(request.approval.approver.as_ref()),
    }
}

fn self_evolution_delivery_outcome(
    contract: &SupervisedDeliveryContract,
) -> SelfEvolutionDeliveryOutcome {
    SelfEvolutionDeliveryOutcome {
        delivery_status: contract.delivery_status,
        approval_state: contract.approval_state,
        reason_code: contract.reason_code,
    }
}

fn self_evolution_reason_code_matrix(
    input: &SelfEvolutionAcceptanceGateInput,
) -> SelfEvolutionReasonCodeMatrix {
    SelfEvolutionReasonCodeMatrix {
        selection_reason_code: input.selection_decision.reason_code,
        proposal_reason_code: input.proposal_contract.reason_code,
        execution_reason_code: input.execution_outcome.reason_code,
        delivery_reason_code: input.delivery_contract.reason_code,
    }
}

fn acceptance_gate_fail_contract(
    summary: &str,
    reason_code: SelfEvolutionAcceptanceGateReasonCode,
    recovery_hint: Option<&str>,
    approval_evidence: SelfEvolutionApprovalEvidence,
    delivery_outcome: SelfEvolutionDeliveryOutcome,
    reason_code_matrix: SelfEvolutionReasonCodeMatrix,
) -> SelfEvolutionAcceptanceGateContract {
    SelfEvolutionAcceptanceGateContract {
        acceptance_gate_summary: summary.to_string(),
        audit_consistency_result: SelfEvolutionAuditConsistencyResult::Inconsistent,
        approval_evidence,
        delivery_outcome,
        reason_code_matrix,
        fail_closed: true,
        reason_code,
        recovery_hint: recovery_hint.map(str::to_string),
    }
}

fn reason_code_matrix_consistent(
    matrix: &SelfEvolutionReasonCodeMatrix,
    execution_outcome: &SupervisedDevloopOutcome,
) -> bool {
    matches!(
        matrix.selection_reason_code,
        Some(SelfEvolutionSelectionReasonCode::Accepted)
    ) && matches!(
        matrix.proposal_reason_code,
        MutationProposalContractReasonCode::Accepted
    ) && matches!(
        matrix.execution_reason_code,
        Some(SupervisedExecutionReasonCode::ReplayHit)
            | Some(SupervisedExecutionReasonCode::ReplayFallback)
    ) && matches!(
        matrix.delivery_reason_code,
        SupervisedDeliveryReasonCode::DeliveryPrepared
    ) && execution_reason_matches_decision(
        execution_outcome.execution_decision,
        matrix.execution_reason_code,
    )
}

fn execution_reason_matches_decision(
    decision: SupervisedExecutionDecision,
    reason_code: Option<SupervisedExecutionReasonCode>,
) -> bool {
    matches!(
        (decision, reason_code),
        (
            SupervisedExecutionDecision::ReplayHit,
            Some(SupervisedExecutionReasonCode::ReplayHit)
        ) | (
            SupervisedExecutionDecision::PlannerFallback,
            Some(SupervisedExecutionReasonCode::ReplayFallback)
        )
    )
}

fn acceptance_gate_reason_code_key(
    reason_code: SelfEvolutionAcceptanceGateReasonCode,
) -> &'static str {
    match reason_code {
        SelfEvolutionAcceptanceGateReasonCode::Accepted => "accepted",
        SelfEvolutionAcceptanceGateReasonCode::MissingSelectionEvidence => {
            "missing_selection_evidence"
        }
        SelfEvolutionAcceptanceGateReasonCode::MissingProposalEvidence => {
            "missing_proposal_evidence"
        }
        SelfEvolutionAcceptanceGateReasonCode::MissingApprovalEvidence => {
            "missing_approval_evidence"
        }
        SelfEvolutionAcceptanceGateReasonCode::MissingExecutionEvidence => {
            "missing_execution_evidence"
        }
        SelfEvolutionAcceptanceGateReasonCode::MissingDeliveryEvidence => {
            "missing_delivery_evidence"
        }
        SelfEvolutionAcceptanceGateReasonCode::InconsistentReasonCodeMatrix => {
            "inconsistent_reason_code_matrix"
        }
        SelfEvolutionAcceptanceGateReasonCode::UnknownFailClosed => "unknown_fail_closed",
    }
}

fn audit_consistency_result_key(result: SelfEvolutionAuditConsistencyResult) -> &'static str {
    match result {
        SelfEvolutionAuditConsistencyResult::Consistent => "consistent",
        SelfEvolutionAuditConsistencyResult::Inconsistent => "inconsistent",
    }
}

fn serialize_acceptance_field<T: Serialize>(value: &T) -> Result<String, EvoKernelError> {
    serde_json::to_string(value).map_err(|err| {
        EvoKernelError::Validation(format!(
            "failed to serialize acceptance gate event field: {err}"
        ))
    })
}

fn non_empty_owned(value: Option<&String>) -> Option<String> {
    value.and_then(|inner| {
        let trimmed = inner.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

impl<S: KernelState> EvoKernel<S> {
    fn record_delivery_rejection(
        &self,
        mutation_id: &str,
        contract: &SupervisedDeliveryContract,
    ) -> Result<(), EvoKernelError> {
        self.store
            .append_event(EvolutionEvent::MutationRejected {
                mutation_id: mutation_id.to_string(),
                reason: contract.delivery_summary.clone(),
                reason_code: Some(delivery_reason_code_key(contract.reason_code).to_string()),
                recovery_hint: contract.recovery_hint.clone(),
                fail_closed: contract.fail_closed,
            })
            .map(|_| ())
            .map_err(store_err)
    }

    fn record_acceptance_gate_result(
        &self,
        input: &SelfEvolutionAcceptanceGateInput,
        contract: &SelfEvolutionAcceptanceGateContract,
    ) -> Result<(), EvoKernelError> {
        self.store
            .append_event(EvolutionEvent::AcceptanceGateEvaluated {
                task_id: input.supervised_request.task.id.clone(),
                issue_number: input.selection_decision.issue_number,
                acceptance_gate_summary: contract.acceptance_gate_summary.clone(),
                audit_consistency_result: audit_consistency_result_key(
                    contract.audit_consistency_result,
                )
                .to_string(),
                approval_evidence: serialize_acceptance_field(&contract.approval_evidence)?,
                delivery_outcome: serialize_acceptance_field(&contract.delivery_outcome)?,
                reason_code_matrix: serialize_acceptance_field(&contract.reason_code_matrix)?,
                fail_closed: contract.fail_closed,
                reason_code: acceptance_gate_reason_code_key(contract.reason_code).to_string(),
            })
            .map(|_| ())
            .map_err(store_err)
    }
}

fn default_mutation_proposal_expected_evidence() -> Vec<MutationProposalEvidence> {
    vec![
        MutationProposalEvidence::HumanApproval,
        MutationProposalEvidence::BoundedScope,
        MutationProposalEvidence::ValidationPass,
        MutationProposalEvidence::ExecutionAudit,
    ]
}

fn mutation_proposal_validation_budget(
    validation_plan: &ValidationPlan,
) -> MutationProposalValidationBudget {
    MutationProposalValidationBudget {
        max_diff_bytes: MUTATION_NEEDED_MAX_DIFF_BYTES,
        max_changed_lines: MUTATION_NEEDED_MAX_CHANGED_LINES,
        validation_timeout_ms: validation_plan_timeout_budget_ms(validation_plan),
    }
}

fn proposal_reason_code_from_selection(
    selection: &SelfEvolutionSelectionDecision,
) -> MutationProposalContractReasonCode {
    match selection.reason_code {
        Some(SelfEvolutionSelectionReasonCode::Accepted) => {
            MutationProposalContractReasonCode::Accepted
        }
        Some(SelfEvolutionSelectionReasonCode::UnsupportedCandidateScope) => {
            MutationProposalContractReasonCode::OutOfBoundsPath
        }
        Some(SelfEvolutionSelectionReasonCode::UnknownFailClosed) | None => {
            MutationProposalContractReasonCode::UnknownFailClosed
        }
        Some(
            SelfEvolutionSelectionReasonCode::IssueClosed
            | SelfEvolutionSelectionReasonCode::MissingEvolutionLabel
            | SelfEvolutionSelectionReasonCode::MissingFeatureLabel
            | SelfEvolutionSelectionReasonCode::ExcludedByLabel,
        ) => MutationProposalContractReasonCode::CandidateRejected,
    }
}

fn mutation_needed_contract_from_proposal_contract(
    proposal_contract: &SelfEvolutionMutationProposalContract,
) -> MutationNeededFailureContract {
    let reason_code = match proposal_contract.reason_code {
        MutationProposalContractReasonCode::UnknownFailClosed => {
            MutationNeededFailureReasonCode::UnknownFailClosed
        }
        MutationProposalContractReasonCode::Accepted
        | MutationProposalContractReasonCode::CandidateRejected
        | MutationProposalContractReasonCode::MissingTargetFiles
        | MutationProposalContractReasonCode::OutOfBoundsPath
        | MutationProposalContractReasonCode::UnsupportedTaskClass
        | MutationProposalContractReasonCode::ValidationBudgetExceeded
        | MutationProposalContractReasonCode::ExpectedEvidenceMissing => {
            MutationNeededFailureReasonCode::PolicyDenied
        }
    };

    normalize_mutation_needed_failure_contract(
        proposal_contract
            .failure_reason
            .as_deref()
            .or(Some(proposal_contract.summary.as_str())),
        Some(reason_code),
    )
}

fn supervised_devloop_mutation_proposal_scope(
    request: &SupervisedDevloopRequest,
) -> Result<MutationProposalScope, MutationProposalContractReasonCode> {
    // Try docs classification first.
    if let Ok(target_files) = validate_bounded_docs_files(&request.proposal.files) {
        let task_class = match target_files.len() {
            1 => BoundedTaskClass::DocsSingleFile,
            2..=SUPERVISED_DEVLOOP_MAX_DOC_FILES => BoundedTaskClass::DocsMultiFile,
            _ => return Err(MutationProposalContractReasonCode::UnsupportedTaskClass),
        };
        return Ok(MutationProposalScope {
            task_class,
            target_files,
        });
    }

    // Try Cargo dependency-upgrade classification.
    if let Ok(target_files) = validate_bounded_cargo_dep_files(&request.proposal.files) {
        return Ok(MutationProposalScope {
            task_class: BoundedTaskClass::CargoDepUpgrade,
            target_files,
        });
    }

    // Try lint-fix classification.
    if let Ok(target_files) = validate_bounded_lint_files(&request.proposal.files) {
        return Ok(MutationProposalScope {
            task_class: BoundedTaskClass::LintFix,
            target_files,
        });
    }

    Err(MutationProposalContractReasonCode::UnsupportedTaskClass)
}

fn validate_bounded_docs_files(
    files: &[String],
) -> Result<Vec<String>, MutationProposalContractReasonCode> {
    if files.is_empty() {
        return Err(MutationProposalContractReasonCode::MissingTargetFiles);
    }
    if files.len() > SUPERVISED_DEVLOOP_MAX_DOC_FILES {
        return Err(MutationProposalContractReasonCode::UnsupportedTaskClass);
    }

    let mut normalized_files = Vec::with_capacity(files.len());
    let mut seen = BTreeSet::new();

    for path in files {
        let normalized = path.trim().replace('\\', "/");
        if normalized.is_empty()
            || !normalized.starts_with("docs/")
            || !normalized.ends_with(".md")
            || !seen.insert(normalized.clone())
        {
            return Err(MutationProposalContractReasonCode::OutOfBoundsPath);
        }
        normalized_files.push(normalized);
    }

    Ok(normalized_files)
}

/// Validate that all files are Cargo manifests or the workspace lock file.
/// Allows: `Cargo.toml`, `Cargo.lock`, `crates/*/Cargo.toml`.
/// Safety: max 5 files, no path traversal.
fn validate_bounded_cargo_dep_files(
    files: &[String],
) -> Result<Vec<String>, MutationProposalContractReasonCode> {
    if files.is_empty() {
        return Err(MutationProposalContractReasonCode::MissingTargetFiles);
    }
    if files.len() > SUPERVISED_DEVLOOP_MAX_CARGO_TOML_FILES {
        return Err(MutationProposalContractReasonCode::UnsupportedTaskClass);
    }

    let mut normalized_files = Vec::with_capacity(files.len());
    let mut seen = BTreeSet::new();

    for path in files {
        let normalized = path.trim().replace('\\', "/");
        if normalized.is_empty() || normalized.contains("..") {
            return Err(MutationProposalContractReasonCode::OutOfBoundsPath);
        }
        // Allow: Cargo.toml, Cargo.lock, <prefix>/Cargo.toml, <prefix>/Cargo.lock.
        let basename = normalized.split('/').next_back().unwrap_or(&normalized);
        if basename != "Cargo.toml" && basename != "Cargo.lock" {
            return Err(MutationProposalContractReasonCode::OutOfBoundsPath);
        }
        if !seen.insert(normalized.clone()) {
            return Err(MutationProposalContractReasonCode::OutOfBoundsPath);
        }
        normalized_files.push(normalized);
    }

    Ok(normalized_files)
}

/// Validate that all files are Rust source files eligible for auto-fix linting.
/// Allows: `**/*.rs` paths within `src/`, `crates/`, `examples/`.
/// Safety: max 5 files, no path traversal, no Cargo manifests.
fn validate_bounded_lint_files(
    files: &[String],
) -> Result<Vec<String>, MutationProposalContractReasonCode> {
    if files.is_empty() {
        return Err(MutationProposalContractReasonCode::MissingTargetFiles);
    }
    if files.len() > SUPERVISED_DEVLOOP_MAX_LINT_FILES {
        return Err(MutationProposalContractReasonCode::UnsupportedTaskClass);
    }

    let allowed_prefixes = ["src/", "crates/", "examples/"];

    let mut normalized_files = Vec::with_capacity(files.len());
    let mut seen = BTreeSet::new();

    for path in files {
        let normalized = path.trim().replace('\\', "/");
        if normalized.is_empty() || normalized.contains("..") {
            return Err(MutationProposalContractReasonCode::OutOfBoundsPath);
        }
        if !normalized.ends_with(".rs") {
            return Err(MutationProposalContractReasonCode::OutOfBoundsPath);
        }
        let in_allowed_prefix = allowed_prefixes
            .iter()
            .any(|prefix| normalized.starts_with(prefix));
        if !in_allowed_prefix {
            return Err(MutationProposalContractReasonCode::OutOfBoundsPath);
        }
        if !seen.insert(normalized.clone()) {
            return Err(MutationProposalContractReasonCode::OutOfBoundsPath);
        }
        normalized_files.push(normalized);
    }

    Ok(normalized_files)
}

fn normalized_supervised_devloop_docs_files(files: &[String]) -> Option<Vec<String>> {
    validate_bounded_docs_files(files).ok()
}

fn classify_self_evolution_candidate_request(
    request: &SelfEvolutionCandidateIntakeRequest,
) -> Option<BoundedTaskClass> {
    normalized_supervised_devloop_docs_files(&request.candidate_hint_paths).and_then(|files| {
        match files.len() {
            1 => Some(BoundedTaskClass::DocsSingleFile),
            2..=SUPERVISED_DEVLOOP_MAX_DOC_FILES => Some(BoundedTaskClass::DocsMultiFile),
            _ => None,
        }
    })
}

fn normalized_selection_labels(labels: &[String]) -> BTreeSet<String> {
    labels
        .iter()
        .map(|label| label.trim().to_ascii_lowercase())
        .filter(|label| !label.is_empty())
        .collect()
}

/// Normalise raw signal tokens: trim, lowercase, remove empties, sort, dedup.
fn normalize_autonomous_signals(raw: &[String]) -> Vec<String> {
    let mut out: Vec<String> = raw
        .iter()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Compute a deterministic dedupe key from the signal source + normalised signal tokens.
fn autonomous_dedupe_key(source: AutonomousCandidateSource, signals: &[String]) -> String {
    stable_hash_json(&(source, signals))
        .unwrap_or_else(|_| compute_artifact_hash(&format!("{source:?}{}", signals.join("|"))))
}

/// Map signal source + tokens to a `BoundedTaskClass`, or `None` when ambiguous / unsupported.
fn classify_autonomous_signals(
    source: AutonomousCandidateSource,
    signals: &[String],
) -> Option<BoundedTaskClass> {
    use AutonomousCandidateSource::*;
    match source {
        CompileRegression | TestRegression | CiFailure => {
            // A non‑empty normalised signal set from a compile/test CI source maps to LintFix
            // (the narrowest bounded class that covers both lint and compile‑error remediation).
            if signals.is_empty() {
                None
            } else {
                Some(BoundedTaskClass::LintFix)
            }
        }
        LintRegression => {
            if signals.is_empty() {
                None
            } else {
                Some(BoundedTaskClass::LintFix)
            }
        }
        RuntimeIncident => None, // Incidents are not yet mapped to a bounded class.
    }
}

/// Return `true` if an equivalent candidate (same dedupe key) already exists in the store
/// by matching against previously extracted signal hashes.
fn autonomous_is_duplicate_in_store(store: &Arc<dyn EvolutionStore>, dedupe_key: &str) -> bool {
    let Ok(events) = store.scan(0) else {
        return false;
    };
    for stored in events {
        if let EvolutionEvent::SignalsExtracted { hash, .. } = &stored.event {
            if hash == dedupe_key {
                return true;
            }
        }
    }
    false
}

/// Produce an `AutonomousTaskPlan` from an accepted `DiscoveredCandidate`.
/// Denied or missing class candidates are denied fail-closed.
fn autonomous_plan_for_candidate(candidate: &DiscoveredCandidate) -> AutonomousTaskPlan {
    let plan_id = stable_hash_json(&("plan-v1", &candidate.dedupe_key))
        .unwrap_or_else(|_| compute_artifact_hash(&candidate.dedupe_key));

    if !candidate.accepted {
        return deny_autonomous_task_plan(
            plan_id,
            candidate.dedupe_key.clone(),
            AutonomousRiskTier::High,
            AutonomousPlanReasonCode::DeniedNoEvidence,
        );
    }

    let Some(task_class) = candidate.candidate_class.clone() else {
        return deny_autonomous_task_plan(
            plan_id,
            candidate.dedupe_key.clone(),
            AutonomousRiskTier::High,
            AutonomousPlanReasonCode::DeniedUnsupportedClass,
        );
    };

    let (risk_tier, feasibility_score, validation_budget, expected_evidence) =
        autonomous_planning_params_for_class(task_class.clone());

    // Deny high-risk work before proposal generation.
    if risk_tier >= AutonomousRiskTier::High {
        return deny_autonomous_task_plan(
            plan_id,
            candidate.dedupe_key.clone(),
            risk_tier,
            AutonomousPlanReasonCode::DeniedHighRisk,
        );
    }

    // Deny if feasibility is below the policy floor of 40.
    if feasibility_score < 40 {
        return deny_autonomous_task_plan(
            plan_id,
            candidate.dedupe_key.clone(),
            risk_tier,
            AutonomousPlanReasonCode::DeniedLowFeasibility,
        );
    }

    let summary = format!(
        "autonomous task plan approved for {task_class:?} ({risk_tier:?} risk, \
         feasibility={feasibility_score}, budget={validation_budget})"
    );
    approve_autonomous_task_plan(
        plan_id,
        candidate.dedupe_key.clone(),
        task_class,
        risk_tier,
        feasibility_score,
        validation_budget,
        expected_evidence,
        Some(&summary),
    )
}

/// Returns `(risk_tier, feasibility_score, validation_budget, expected_evidence)`.
fn autonomous_planning_params_for_class(
    task_class: BoundedTaskClass,
) -> (AutonomousRiskTier, u8, u8, Vec<String>) {
    match task_class {
        BoundedTaskClass::LintFix => (
            AutonomousRiskTier::Low,
            85,
            2,
            vec![
                "cargo fmt --all -- --check".to_string(),
                "cargo clippy targeted output".to_string(),
            ],
        ),
        BoundedTaskClass::DocsSingleFile => (
            AutonomousRiskTier::Low,
            90,
            1,
            vec!["docs review diff".to_string()],
        ),
        BoundedTaskClass::DocsMultiFile => (
            AutonomousRiskTier::Medium,
            75,
            2,
            vec![
                "docs review diff".to_string(),
                "link validation".to_string(),
            ],
        ),
        BoundedTaskClass::CargoDepUpgrade => (
            AutonomousRiskTier::Medium,
            70,
            3,
            vec![
                "cargo audit".to_string(),
                "cargo test regression".to_string(),
                "cargo build all features".to_string(),
            ],
        ),
    }
}

/// Produce an `AutonomousMutationProposal` from an approved `AutonomousTaskPlan`.
/// Unapproved plans, unsupported classes, or empty evidence sets produce a
/// denied fail-closed proposal.
fn autonomous_proposal_for_plan(plan: &AutonomousTaskPlan) -> AutonomousMutationProposal {
    let proposal_id = stable_hash_json(&("proposal-v1", &plan.plan_id))
        .unwrap_or_else(|_| compute_artifact_hash(&plan.plan_id));

    if !plan.approved {
        return deny_autonomous_mutation_proposal(
            proposal_id,
            plan.plan_id.clone(),
            plan.dedupe_key.clone(),
            AutonomousProposalReasonCode::DeniedPlanNotApproved,
        );
    }

    let Some(task_class) = plan.task_class.clone() else {
        return deny_autonomous_mutation_proposal(
            proposal_id,
            plan.plan_id.clone(),
            plan.dedupe_key.clone(),
            AutonomousProposalReasonCode::DeniedNoTargetScope,
        );
    };

    let (target_paths, scope_rationale, max_files, rollback_conditions) =
        autonomous_proposal_scope_for_class(&task_class);

    if plan.expected_evidence.is_empty() {
        return deny_autonomous_mutation_proposal(
            proposal_id,
            plan.plan_id.clone(),
            plan.dedupe_key.clone(),
            AutonomousProposalReasonCode::DeniedWeakEvidence,
        );
    }

    let scope = AutonomousProposalScope {
        target_paths,
        scope_rationale,
        max_files,
    };

    // Low-risk bounded classes are auto-approved; others require human review.
    let approval_mode = if plan.risk_tier == AutonomousRiskTier::Low {
        AutonomousApprovalMode::AutoApproved
    } else {
        AutonomousApprovalMode::RequiresHumanReview
    };

    let summary = format!(
        "autonomous mutation proposal for {task_class:?} ({:?} approval, {} evidence items)",
        approval_mode,
        plan.expected_evidence.len()
    );

    approve_autonomous_mutation_proposal(
        proposal_id,
        plan.plan_id.clone(),
        plan.dedupe_key.clone(),
        scope,
        plan.expected_evidence.clone(),
        rollback_conditions,
        approval_mode,
        Some(&summary),
    )
}

/// Returns `(target_paths, scope_rationale, max_files, rollback_conditions)` for a task class.
fn autonomous_proposal_scope_for_class(
    task_class: &BoundedTaskClass,
) -> (Vec<String>, String, u8, Vec<String>) {
    match task_class {
        BoundedTaskClass::LintFix => (
            vec!["crates/**/*.rs".to_string()],
            "lint and compile fixes are bounded to source files only".to_string(),
            5,
            vec![
                "revert if cargo fmt --all -- --check fails".to_string(),
                "revert if any test regresses".to_string(),
            ],
        ),
        BoundedTaskClass::DocsSingleFile => (
            vec!["docs/**/*.md".to_string(), "crates/**/*.rs".to_string()],
            "doc fixes are bounded to a single documentation or source file".to_string(),
            1,
            vec!["revert if docs review diff shows unrelated changes".to_string()],
        ),
        BoundedTaskClass::DocsMultiFile => (
            vec!["docs/**/*.md".to_string()],
            "multi-file doc updates are bounded to the docs directory".to_string(),
            5,
            vec![
                "revert if docs review diff shows non-doc changes".to_string(),
                "revert if link validation fails".to_string(),
            ],
        ),
        BoundedTaskClass::CargoDepUpgrade => (
            vec!["Cargo.toml".to_string(), "Cargo.lock".to_string()],
            "dependency upgrades are bounded to manifest and lock files only".to_string(),
            2,
            vec![
                "revert if cargo audit reports new vulnerability".to_string(),
                "revert if any test regresses after upgrade".to_string(),
                "revert if cargo build all features fails".to_string(),
            ],
        ),
    }
}

fn find_declared_mutation(
    store: &dyn EvolutionStore,
    mutation_id: &MutationId,
) -> Result<Option<PreparedMutation>, EvolutionError> {
    for stored in store.scan(1)? {
        if let EvolutionEvent::MutationDeclared { mutation } = stored.event {
            if &mutation.intent.id == mutation_id {
                return Ok(Some(mutation));
            }
        }
    }
    Ok(None)
}

fn exact_match_candidates(store: &dyn EvolutionStore, input: &SelectorInput) -> Vec<GeneCandidate> {
    let Ok(projection) = projection_snapshot(store) else {
        return Vec::new();
    };
    let capsules = projection.capsules.clone();
    let spec_ids_by_gene = projection.spec_ids_by_gene.clone();
    let requested_spec_id = input
        .spec_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let signal_set = input
        .signals
        .iter()
        .map(|signal| signal.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    let mut candidates = projection
        .genes
        .into_iter()
        .filter_map(|gene| {
            if gene.state != AssetState::Promoted {
                return None;
            }
            if let Some(spec_id) = requested_spec_id {
                let matches_spec = spec_ids_by_gene
                    .get(&gene.id)
                    .map(|values| {
                        values
                            .iter()
                            .any(|value| value.eq_ignore_ascii_case(spec_id))
                    })
                    .unwrap_or(false);
                if !matches_spec {
                    return None;
                }
            }
            let gene_signals = gene
                .signals
                .iter()
                .map(|signal| signal.to_ascii_lowercase())
                .collect::<BTreeSet<_>>();
            if gene_signals == signal_set {
                let mut matched_capsules = capsules
                    .iter()
                    .filter(|capsule| {
                        capsule.gene_id == gene.id && capsule.state == AssetState::Promoted
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                matched_capsules.sort_by(|left, right| {
                    replay_environment_match_factor(&input.env, &right.env)
                        .partial_cmp(&replay_environment_match_factor(&input.env, &left.env))
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| {
                            right
                                .confidence
                                .partial_cmp(&left.confidence)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        })
                        .then_with(|| left.id.cmp(&right.id))
                });
                if matched_capsules.is_empty() {
                    None
                } else {
                    let score = matched_capsules
                        .first()
                        .map(|capsule| replay_environment_match_factor(&input.env, &capsule.env))
                        .unwrap_or(0.0);
                    Some(GeneCandidate {
                        gene,
                        score,
                        capsules: matched_capsules,
                    })
                }
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.gene.id.cmp(&right.gene.id))
    });
    candidates
}

fn quarantined_remote_exact_match_candidates(
    store: &dyn EvolutionStore,
    input: &SelectorInput,
) -> Vec<GeneCandidate> {
    let remote_asset_ids = store
        .scan(1)
        .ok()
        .map(|events| {
            events
                .into_iter()
                .filter_map(|stored| match stored.event {
                    EvolutionEvent::RemoteAssetImported {
                        source: CandidateSource::Remote,
                        asset_ids,
                        ..
                    } => Some(asset_ids),
                    _ => None,
                })
                .flatten()
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    if remote_asset_ids.is_empty() {
        return Vec::new();
    }

    let Ok(projection) = projection_snapshot(store) else {
        return Vec::new();
    };
    let capsules = projection.capsules.clone();
    let spec_ids_by_gene = projection.spec_ids_by_gene.clone();
    let requested_spec_id = input
        .spec_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let normalized_signals = input
        .signals
        .iter()
        .filter_map(|signal| normalize_signal_phrase(signal))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if normalized_signals.is_empty() {
        return Vec::new();
    }
    let mut candidates = projection
        .genes
        .into_iter()
        .filter_map(|gene| {
            if !matches!(
                gene.state,
                AssetState::Promoted | AssetState::Quarantined | AssetState::ShadowValidated
            ) {
                return None;
            }
            if let Some(spec_id) = requested_spec_id {
                let matches_spec = spec_ids_by_gene
                    .get(&gene.id)
                    .map(|values| {
                        values
                            .iter()
                            .any(|value| value.eq_ignore_ascii_case(spec_id))
                    })
                    .unwrap_or(false);
                if !matches_spec {
                    return None;
                }
            }
            let normalized_gene_signals = gene
                .signals
                .iter()
                .filter_map(|candidate| normalize_signal_phrase(candidate))
                .collect::<Vec<_>>();
            let matched_query_count = normalized_signals
                .iter()
                .filter(|signal| {
                    normalized_gene_signals.iter().any(|candidate| {
                        candidate.contains(signal.as_str()) || signal.contains(candidate)
                    })
                })
                .count();
            if matched_query_count == 0 {
                return None;
            }

            let mut matched_capsules = capsules
                .iter()
                .filter(|capsule| {
                    capsule.gene_id == gene.id
                        && matches!(
                            capsule.state,
                            AssetState::Quarantined | AssetState::ShadowValidated
                        )
                        && remote_asset_ids.contains(&capsule.id)
                })
                .cloned()
                .collect::<Vec<_>>();
            matched_capsules.sort_by(|left, right| {
                replay_environment_match_factor(&input.env, &right.env)
                    .partial_cmp(&replay_environment_match_factor(&input.env, &left.env))
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| {
                        right
                            .confidence
                            .partial_cmp(&left.confidence)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .then_with(|| left.id.cmp(&right.id))
            });
            if matched_capsules.is_empty() {
                None
            } else {
                let overlap = matched_query_count as f32 / normalized_signals.len() as f32;
                let env_score = matched_capsules
                    .first()
                    .map(|capsule| replay_environment_match_factor(&input.env, &capsule.env))
                    .unwrap_or(0.0);
                Some(GeneCandidate {
                    gene,
                    score: overlap.max(env_score),
                    capsules: matched_capsules,
                })
            }
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.gene.id.cmp(&right.gene.id))
    });
    candidates
}

fn replay_environment_match_factor(input: &EnvFingerprint, candidate: &EnvFingerprint) -> f32 {
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
    let matched_fields = fields.into_iter().filter(|matched| *matched).count() as f32;
    0.5 + ((matched_fields / 4.0) * 0.5)
}

fn effective_candidate_score(
    candidate: &GeneCandidate,
    publishers_by_asset: &BTreeMap<String, String>,
    reputation_bias: &BTreeMap<String, f32>,
) -> f32 {
    let bias = candidate
        .capsules
        .first()
        .and_then(|capsule| publishers_by_asset.get(&capsule.id))
        .and_then(|publisher| reputation_bias.get(publisher))
        .copied()
        .unwrap_or(0.0)
        .clamp(0.0, 1.0);
    candidate.score * (1.0 + (bias * 0.1))
}

fn export_promoted_assets_from_store(
    store: &dyn EvolutionStore,
    sender_id: impl Into<String>,
) -> Result<EvolutionEnvelope, EvoKernelError> {
    let (events, projection) = scan_projection(store)?;
    let genes = projection
        .genes
        .into_iter()
        .filter(|gene| gene.state == AssetState::Promoted)
        .collect::<Vec<_>>();
    let capsules = projection
        .capsules
        .into_iter()
        .filter(|capsule| capsule.state == AssetState::Promoted)
        .collect::<Vec<_>>();
    let assets = replay_export_assets(&events, genes, capsules);
    Ok(EvolutionEnvelope::publish(sender_id, assets))
}

fn scan_projection(
    store: &dyn EvolutionStore,
) -> Result<(Vec<StoredEvolutionEvent>, EvolutionProjection), EvoKernelError> {
    store.scan_projection().map_err(store_err)
}

fn projection_snapshot(store: &dyn EvolutionStore) -> Result<EvolutionProjection, EvoKernelError> {
    scan_projection(store).map(|(_, projection)| projection)
}

fn replay_export_assets(
    events: &[StoredEvolutionEvent],
    genes: Vec<Gene>,
    capsules: Vec<Capsule>,
) -> Vec<NetworkAsset> {
    let mutation_ids = capsules
        .iter()
        .map(|capsule| capsule.mutation_id.clone())
        .collect::<BTreeSet<_>>();
    let mut assets = replay_export_events_for_mutations(events, &mutation_ids);
    for gene in genes {
        assets.push(NetworkAsset::Gene { gene });
    }
    for capsule in capsules {
        assets.push(NetworkAsset::Capsule { capsule });
    }
    assets
}

fn replay_export_events_for_mutations(
    events: &[StoredEvolutionEvent],
    mutation_ids: &BTreeSet<String>,
) -> Vec<NetworkAsset> {
    if mutation_ids.is_empty() {
        return Vec::new();
    }

    let mut assets = Vec::new();
    let mut seen_mutations = BTreeSet::new();
    let mut seen_spec_links = BTreeSet::new();
    for stored in events {
        match &stored.event {
            EvolutionEvent::MutationDeclared { mutation }
                if mutation_ids.contains(mutation.intent.id.as_str())
                    && seen_mutations.insert(mutation.intent.id.clone()) =>
            {
                assets.push(NetworkAsset::EvolutionEvent {
                    event: EvolutionEvent::MutationDeclared {
                        mutation: mutation.clone(),
                    },
                });
            }
            EvolutionEvent::SpecLinked {
                mutation_id,
                spec_id,
            } if mutation_ids.contains(mutation_id.as_str())
                && seen_spec_links.insert((mutation_id.clone(), spec_id.clone())) =>
            {
                assets.push(NetworkAsset::EvolutionEvent {
                    event: EvolutionEvent::SpecLinked {
                        mutation_id: mutation_id.clone(),
                        spec_id: spec_id.clone(),
                    },
                });
            }
            _ => {}
        }
    }

    assets
}

const SYNC_CURSOR_PREFIX: &str = "seq:";
const SYNC_RESUME_TOKEN_PREFIX: &str = "gep-rt1|";

#[derive(Clone, Debug)]
struct DeltaWindow {
    changed_gene_ids: BTreeSet<String>,
    changed_capsule_ids: BTreeSet<String>,
    changed_mutation_ids: BTreeSet<String>,
}

fn normalize_sync_value(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn parse_sync_cursor_seq(cursor: &str) -> Option<u64> {
    let trimmed = cursor.trim();
    if trimmed.is_empty() {
        return None;
    }
    let raw = trimmed.strip_prefix(SYNC_CURSOR_PREFIX).unwrap_or(trimmed);
    raw.parse::<u64>().ok()
}

fn format_sync_cursor(seq: u64) -> String {
    format!("{SYNC_CURSOR_PREFIX}{seq}")
}

fn encode_resume_token(sender_id: &str, cursor: &str) -> String {
    format!("{SYNC_RESUME_TOKEN_PREFIX}{sender_id}|{cursor}")
}

fn decode_resume_token(sender_id: &str, token: &str) -> Result<String, EvoKernelError> {
    let token = token.trim();
    let Some(encoded) = token.strip_prefix(SYNC_RESUME_TOKEN_PREFIX) else {
        return Ok(token.to_string());
    };
    let (token_sender, cursor) = encoded.split_once('|').ok_or_else(|| {
        EvoKernelError::Validation(
            "invalid resume_token format; expected gep-rt1|<sender>|<seq>".into(),
        )
    })?;
    if token_sender != sender_id.trim() {
        return Err(EvoKernelError::Validation(
            "resume_token sender mismatch".into(),
        ));
    }
    Ok(cursor.to_string())
}

fn resolve_requested_cursor(
    sender_id: &str,
    since_cursor: Option<&str>,
    resume_token: Option<&str>,
) -> Result<Option<String>, EvoKernelError> {
    let cursor = if let Some(token) = normalize_sync_value(resume_token) {
        Some(decode_resume_token(sender_id, &token)?)
    } else {
        normalize_sync_value(since_cursor)
    };

    let Some(cursor) = cursor else {
        return Ok(None);
    };
    let seq = parse_sync_cursor_seq(&cursor).ok_or_else(|| {
        EvoKernelError::Validation("invalid since_cursor/resume_token cursor format".into())
    })?;
    Ok(Some(format_sync_cursor(seq)))
}

fn latest_store_cursor(store: &dyn EvolutionStore) -> Result<Option<String>, EvoKernelError> {
    let events = store.scan(1).map_err(store_err)?;
    Ok(events.last().map(|stored| format_sync_cursor(stored.seq)))
}

fn delta_window(events: &[StoredEvolutionEvent], since_seq: u64) -> DeltaWindow {
    let mut changed_gene_ids = BTreeSet::new();
    let mut changed_capsule_ids = BTreeSet::new();
    let mut changed_mutation_ids = BTreeSet::new();

    for stored in events {
        if stored.seq <= since_seq {
            continue;
        }
        match &stored.event {
            EvolutionEvent::MutationDeclared { mutation } => {
                changed_mutation_ids.insert(mutation.intent.id.clone());
            }
            EvolutionEvent::SpecLinked { mutation_id, .. } => {
                changed_mutation_ids.insert(mutation_id.clone());
            }
            EvolutionEvent::GeneProjected { gene } => {
                changed_gene_ids.insert(gene.id.clone());
            }
            EvolutionEvent::GenePromoted { gene_id }
            | EvolutionEvent::GeneRevoked { gene_id, .. }
            | EvolutionEvent::PromotionEvaluated { gene_id, .. } => {
                changed_gene_ids.insert(gene_id.clone());
            }
            EvolutionEvent::CapsuleCommitted { capsule } => {
                changed_capsule_ids.insert(capsule.id.clone());
                changed_gene_ids.insert(capsule.gene_id.clone());
                changed_mutation_ids.insert(capsule.mutation_id.clone());
            }
            EvolutionEvent::CapsuleReleased { capsule_id, .. }
            | EvolutionEvent::CapsuleQuarantined { capsule_id } => {
                changed_capsule_ids.insert(capsule_id.clone());
            }
            EvolutionEvent::RemoteAssetImported { asset_ids, .. } => {
                for asset_id in asset_ids {
                    changed_gene_ids.insert(asset_id.clone());
                    changed_capsule_ids.insert(asset_id.clone());
                }
            }
            _ => {}
        }
    }

    DeltaWindow {
        changed_gene_ids,
        changed_capsule_ids,
        changed_mutation_ids,
    }
}

fn import_remote_envelope_into_store(
    store: &dyn EvolutionStore,
    envelope: &EvolutionEnvelope,
    remote_publishers: Option<&Mutex<BTreeMap<String, String>>>,
    requested_cursor: Option<String>,
) -> Result<ImportOutcome, EvoKernelError> {
    if !envelope.verify_content_hash() {
        record_manifest_validation(store, envelope, false, "invalid evolution envelope hash")?;
        return Err(EvoKernelError::Validation(
            "invalid evolution envelope hash".into(),
        ));
    }
    if let Err(reason) = envelope.verify_manifest() {
        record_manifest_validation(
            store,
            envelope,
            false,
            format!("manifest validation failed: {reason}"),
        )?;
        return Err(EvoKernelError::Validation(format!(
            "invalid evolution envelope manifest: {reason}"
        )));
    }
    record_manifest_validation(store, envelope, true, "manifest validated")?;

    let sender_id = normalized_sender_id(&envelope.sender_id);
    let (events, projection) = scan_projection(store)?;
    let mut known_gene_ids = projection
        .genes
        .into_iter()
        .map(|gene| gene.id)
        .collect::<BTreeSet<_>>();
    let mut known_capsule_ids = projection
        .capsules
        .into_iter()
        .map(|capsule| capsule.id)
        .collect::<BTreeSet<_>>();
    let mut known_mutation_ids = BTreeSet::new();
    let mut known_spec_links = BTreeSet::new();
    for stored in &events {
        match &stored.event {
            EvolutionEvent::MutationDeclared { mutation } => {
                known_mutation_ids.insert(mutation.intent.id.clone());
            }
            EvolutionEvent::SpecLinked {
                mutation_id,
                spec_id,
            } => {
                known_spec_links.insert((mutation_id.clone(), spec_id.clone()));
            }
            _ => {}
        }
    }
    let mut imported_asset_ids = Vec::new();
    let mut applied_count = 0usize;
    let mut skipped_count = 0usize;
    for asset in &envelope.assets {
        match asset {
            NetworkAsset::Gene { gene } => {
                if !known_gene_ids.insert(gene.id.clone()) {
                    skipped_count += 1;
                    continue;
                }
                imported_asset_ids.push(gene.id.clone());
                applied_count += 1;
                let mut quarantined_gene = gene.clone();
                quarantined_gene.state = AssetState::Quarantined;
                store
                    .append_event(EvolutionEvent::RemoteAssetImported {
                        source: CandidateSource::Remote,
                        asset_ids: vec![gene.id.clone()],
                        sender_id: sender_id.clone(),
                    })
                    .map_err(store_err)?;
                store
                    .append_event(EvolutionEvent::GeneProjected {
                        gene: quarantined_gene.clone(),
                    })
                    .map_err(store_err)?;
                record_remote_publisher_for_asset(remote_publishers, &envelope.sender_id, asset);
                store
                    .append_event(EvolutionEvent::PromotionEvaluated {
                        gene_id: quarantined_gene.id,
                        state: AssetState::Quarantined,
                        reason: "remote asset requires local validation before promotion".into(),
                        reason_code: TransitionReasonCode::DowngradeRemoteRequiresLocalValidation,
                        evidence: Some(TransitionEvidence {
                            replay_attempts: None,
                            replay_successes: None,
                            replay_success_rate: None,
                            environment_match_factor: None,
                            decayed_confidence: None,
                            confidence_decay_ratio: None,
                            summary: Some("phase=remote_import; source=remote; action=quarantine_before_shadow_validation".into()),
                        }),
                    })
                    .map_err(store_err)?;
            }
            NetworkAsset::Capsule { capsule } => {
                if !known_capsule_ids.insert(capsule.id.clone()) {
                    skipped_count += 1;
                    continue;
                }
                imported_asset_ids.push(capsule.id.clone());
                applied_count += 1;
                store
                    .append_event(EvolutionEvent::RemoteAssetImported {
                        source: CandidateSource::Remote,
                        asset_ids: vec![capsule.id.clone()],
                        sender_id: sender_id.clone(),
                    })
                    .map_err(store_err)?;
                let mut quarantined = capsule.clone();
                quarantined.state = AssetState::Quarantined;
                store
                    .append_event(EvolutionEvent::CapsuleCommitted {
                        capsule: quarantined.clone(),
                    })
                    .map_err(store_err)?;
                record_remote_publisher_for_asset(remote_publishers, &envelope.sender_id, asset);
                store
                    .append_event(EvolutionEvent::CapsuleQuarantined {
                        capsule_id: quarantined.id,
                    })
                    .map_err(store_err)?;
            }
            NetworkAsset::EvolutionEvent { event } => {
                let should_append = match event {
                    EvolutionEvent::MutationDeclared { mutation } => {
                        known_mutation_ids.insert(mutation.intent.id.clone())
                    }
                    EvolutionEvent::SpecLinked {
                        mutation_id,
                        spec_id,
                    } => known_spec_links.insert((mutation_id.clone(), spec_id.clone())),
                    _ if should_import_remote_event(event) => true,
                    _ => false,
                };
                if should_append {
                    store.append_event(event.clone()).map_err(store_err)?;
                    applied_count += 1;
                } else {
                    skipped_count += 1;
                }
            }
        }
    }
    let next_cursor = latest_store_cursor(store)?;
    let resume_token = next_cursor.as_ref().and_then(|cursor| {
        normalized_sender_id(&envelope.sender_id).map(|sender| encode_resume_token(&sender, cursor))
    });

    Ok(ImportOutcome {
        imported_asset_ids,
        accepted: true,
        next_cursor: next_cursor.clone(),
        resume_token,
        sync_audit: SyncAudit {
            batch_id: next_id("sync-import"),
            requested_cursor,
            scanned_count: envelope.assets.len(),
            applied_count,
            skipped_count,
            failed_count: 0,
            failure_reasons: Vec::new(),
        },
    })
}

const EVOMAP_SNAPSHOT_ROOT: &str = "assets/gep/evomap_snapshot";
const EVOMAP_SNAPSHOT_GENES_FILE: &str = "genes.json";
const EVOMAP_SNAPSHOT_CAPSULES_FILE: &str = "capsules.json";
const EVOMAP_BUILTIN_RUN_ID: &str = "builtin-evomap-seed";

#[derive(Debug, Deserialize)]
struct EvoMapGeneDocument {
    #[serde(default)]
    genes: Vec<EvoMapGeneAsset>,
}

#[derive(Debug, Deserialize)]
struct EvoMapGeneAsset {
    id: String,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    signals_match: Vec<Value>,
    #[serde(default)]
    strategy: Vec<String>,
    #[serde(default)]
    validation: Vec<String>,
    #[serde(default)]
    constraints: Option<EvoMapConstraintAsset>,
    #[serde(default)]
    model_name: Option<String>,
    #[serde(default)]
    schema_version: Option<String>,
    #[serde(default)]
    compatibility: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, Default)]
struct EvoMapConstraintAsset {
    #[serde(default)]
    max_files: Option<usize>,
    #[serde(default)]
    forbidden_paths: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct EvoMapCapsuleDocument {
    #[serde(default)]
    capsules: Vec<EvoMapCapsuleAsset>,
}

#[derive(Debug, Deserialize)]
struct EvoMapCapsuleAsset {
    id: String,
    gene: String,
    #[serde(default)]
    trigger: Vec<String>,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    diff: Option<String>,
    #[serde(default)]
    confidence: Option<f32>,
    #[serde(default)]
    outcome: Option<EvoMapOutcomeAsset>,
    #[serde(default)]
    blast_radius: Option<EvoMapBlastRadiusAsset>,
    #[serde(default)]
    content: Option<EvoMapCapsuleContentAsset>,
    #[serde(default)]
    env_fingerprint: Option<Value>,
    #[serde(default)]
    model_name: Option<String>,
    #[serde(default)]
    schema_version: Option<String>,
    #[serde(default)]
    compatibility: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, Default)]
struct EvoMapOutcomeAsset {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    score: Option<f32>,
}

#[derive(Clone, Debug, Deserialize, Default)]
struct EvoMapBlastRadiusAsset {
    #[serde(default)]
    lines: usize,
}

#[derive(Clone, Debug, Deserialize, Default)]
struct EvoMapCapsuleContentAsset {
    #[serde(default)]
    changed_files: Vec<String>,
}

#[derive(Debug)]
struct BuiltinCapsuleSeed {
    capsule: Capsule,
    mutation: PreparedMutation,
}

#[derive(Debug)]
struct BuiltinAssetBundle {
    genes: Vec<Gene>,
    capsules: Vec<BuiltinCapsuleSeed>,
}

fn built_in_experience_genes() -> Vec<Gene> {
    vec![
        Gene {
            id: "builtin-experience-docs-rewrite-v1".into(),
            signals: vec!["docs.rewrite".into(), "docs".into(), "rewrite".into()],
            strategy: vec![
                "asset_origin=builtin".into(),
                "task_class=docs.rewrite".into(),
                "task_label=Docs rewrite".into(),
                "template_id=builtin-docs-rewrite-v1".into(),
                "summary=baseline docs rewrite experience".into(),
            ],
            validation: vec!["builtin-template".into(), "origin=builtin".into()],
            state: AssetState::Promoted,
            task_class_id: None,
        },
        Gene {
            id: "builtin-experience-ci-fix-v1".into(),
            signals: vec![
                "ci.fix".into(),
                "ci".into(),
                "test".into(),
                "failure".into(),
            ],
            strategy: vec![
                "asset_origin=builtin".into(),
                "task_class=ci.fix".into(),
                "task_label=CI fix".into(),
                "template_id=builtin-ci-fix-v1".into(),
                "summary=baseline ci stabilization experience".into(),
            ],
            validation: vec!["builtin-template".into(), "origin=builtin".into()],
            state: AssetState::Promoted,
            task_class_id: None,
        },
        Gene {
            id: "builtin-experience-task-decomposition-v1".into(),
            signals: vec![
                "task.decomposition".into(),
                "task".into(),
                "decomposition".into(),
                "planning".into(),
            ],
            strategy: vec![
                "asset_origin=builtin".into(),
                "task_class=task.decomposition".into(),
                "task_label=Task decomposition".into(),
                "template_id=builtin-task-decomposition-v1".into(),
                "summary=baseline task decomposition and routing experience".into(),
            ],
            validation: vec!["builtin-template".into(), "origin=builtin".into()],
            state: AssetState::Promoted,
            task_class_id: None,
        },
        Gene {
            id: "builtin-experience-project-workflow-v1".into(),
            signals: vec![
                "project.workflow".into(),
                "project".into(),
                "workflow".into(),
                "milestone".into(),
            ],
            strategy: vec![
                "asset_origin=builtin".into(),
                "task_class=project.workflow".into(),
                "task_label=Project workflow".into(),
                "template_id=builtin-project-workflow-v1".into(),
                "summary=baseline project proposal and merge workflow experience".into(),
            ],
            validation: vec!["builtin-template".into(), "origin=builtin".into()],
            state: AssetState::Promoted,
            task_class_id: None,
        },
        Gene {
            id: "builtin-experience-service-bid-v1".into(),
            signals: vec![
                "service.bid".into(),
                "service".into(),
                "bid".into(),
                "economics".into(),
            ],
            strategy: vec![
                "asset_origin=builtin".into(),
                "task_class=service.bid".into(),
                "task_label=Service bid".into(),
                "template_id=builtin-service-bid-v1".into(),
                "summary=baseline service bidding and settlement experience".into(),
            ],
            validation: vec!["builtin-template".into(), "origin=builtin".into()],
            state: AssetState::Promoted,
            task_class_id: None,
        },
    ]
}

fn evomap_snapshot_path(file_name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(EVOMAP_SNAPSHOT_ROOT)
        .join(file_name)
}

fn read_evomap_snapshot(file_name: &str) -> Result<Option<String>, EvoKernelError> {
    let path = evomap_snapshot_path(file_name);
    if !path.exists() {
        return Ok(None);
    }
    fs::read_to_string(&path).map(Some).map_err(|err| {
        EvoKernelError::Validation(format!(
            "failed to read EvoMap snapshot {}: {err}",
            path.display()
        ))
    })
}

fn compatibility_state_from_value(value: Option<&Value>) -> Option<String> {
    let value = value?;
    if let Some(state) = value.as_str() {
        let normalized = state.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            return None;
        }
        return Some(normalized);
    }
    value
        .get("state")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|state| !state.is_empty())
        .map(|state| state.to_ascii_lowercase())
}

fn map_evomap_state(value: Option<&Value>) -> AssetState {
    match compatibility_state_from_value(value).as_deref() {
        Some("promoted") => AssetState::Promoted,
        Some("candidate") => AssetState::Candidate,
        Some("quarantined") => AssetState::Quarantined,
        Some("shadow_validated") => AssetState::ShadowValidated,
        Some("revoked") => AssetState::Revoked,
        Some("rejected") => AssetState::Archived,
        Some("archived") => AssetState::Archived,
        _ => AssetState::Candidate,
    }
}

fn value_as_signal_string(value: &Value) -> Option<String> {
    match value {
        Value::String(raw) => {
            let normalized = raw.trim();
            if normalized.is_empty() {
                None
            } else {
                Some(normalized.to_string())
            }
        }
        Value::Object(_) => {
            let serialized = serde_json::to_string(value).ok()?;
            let normalized = serialized.trim();
            if normalized.is_empty() {
                None
            } else {
                Some(normalized.to_string())
            }
        }
        Value::Null => None,
        other => {
            let rendered = other.to_string();
            let normalized = rendered.trim();
            if normalized.is_empty() {
                None
            } else {
                Some(normalized.to_string())
            }
        }
    }
}

fn parse_diff_changed_files(payload: &str) -> Vec<String> {
    let mut changed_files = BTreeSet::new();
    for line in payload.lines() {
        let line = line.trim();
        if let Some(path) = line.strip_prefix("+++ b/") {
            let path = path.trim();
            if !path.is_empty() && path != "/dev/null" {
                changed_files.insert(path.to_string());
            }
            continue;
        }
        if let Some(path) = line.strip_prefix("diff --git a/") {
            if let Some((_, right)) = path.split_once(" b/") {
                let right = right.trim();
                if !right.is_empty() {
                    changed_files.insert(right.to_string());
                }
            }
        }
    }
    changed_files.into_iter().collect()
}

fn strip_diff_code_fence(payload: &str) -> String {
    let trimmed = payload.trim();
    if !trimmed.starts_with("```") {
        return trimmed.to_string();
    }
    let mut lines = trimmed.lines().collect::<Vec<_>>();
    if lines.is_empty() {
        return String::new();
    }
    lines.remove(0);
    if lines
        .last()
        .map(|line| line.trim() == "```")
        .unwrap_or(false)
    {
        lines.pop();
    }
    lines.join("\n").trim().to_string()
}

fn synthetic_diff_for_capsule(capsule: &EvoMapCapsuleAsset) -> String {
    let file_path = format!("docs/evomap_builtin_capsules/{}.md", capsule.id);
    let mut content = Vec::new();
    content.push(format!("# EvoMap Builtin Capsule {}", capsule.id));
    if capsule.summary.trim().is_empty() {
        content.push("summary: missing".to_string());
    } else {
        content.push(format!("summary: {}", capsule.summary.trim()));
    }
    if !capsule.trigger.is_empty() {
        content.push(format!("trigger: {}", capsule.trigger.join(", ")));
    }
    content.push(format!("gene: {}", capsule.gene));
    let added = content
        .into_iter()
        .map(|line| format!("+{}", line.replace('\r', "")))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "diff --git a/{file_path} b/{file_path}\nnew file mode 100644\nindex 0000000..1111111\n--- /dev/null\n+++ b/{file_path}\n@@ -0,0 +1,{line_count} @@\n{added}\n",
        line_count = added.lines().count()
    )
}

fn normalized_diff_payload(capsule: &EvoMapCapsuleAsset) -> String {
    if let Some(raw) = capsule.diff.as_deref() {
        let normalized = strip_diff_code_fence(raw);
        if !normalized.trim().is_empty() {
            return normalized;
        }
    }
    synthetic_diff_for_capsule(capsule)
}

fn env_field(value: Option<&Value>, keys: &[&str]) -> Option<String> {
    let object = value?.as_object()?;
    keys.iter().find_map(|key| {
        object
            .get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_string())
    })
}

fn map_evomap_env_fingerprint(value: Option<&Value>) -> EnvFingerprint {
    let os =
        env_field(value, &["os", "platform", "os_release"]).unwrap_or_else(|| "unknown".into());
    let target_triple = env_field(value, &["target_triple"]).unwrap_or_else(|| {
        let arch = env_field(value, &["arch"]).unwrap_or_else(|| "unknown".into());
        format!("{arch}-unknown-{os}")
    });
    EnvFingerprint {
        rustc_version: env_field(value, &["runtime", "rustc_version", "node_version"])
            .unwrap_or_else(|| "unknown".into()),
        cargo_lock_hash: env_field(value, &["cargo_lock_hash"]).unwrap_or_else(|| "unknown".into()),
        target_triple,
        os,
    }
}

fn load_evomap_builtin_assets() -> Result<Option<BuiltinAssetBundle>, EvoKernelError> {
    let genes_raw = read_evomap_snapshot(EVOMAP_SNAPSHOT_GENES_FILE)?;
    let capsules_raw = read_evomap_snapshot(EVOMAP_SNAPSHOT_CAPSULES_FILE)?;
    let (Some(genes_raw), Some(capsules_raw)) = (genes_raw, capsules_raw) else {
        return Ok(None);
    };

    let genes_doc: EvoMapGeneDocument = serde_json::from_str(&genes_raw).map_err(|err| {
        EvoKernelError::Validation(format!("failed to parse EvoMap genes snapshot: {err}"))
    })?;
    let capsules_doc: EvoMapCapsuleDocument =
        serde_json::from_str(&capsules_raw).map_err(|err| {
            EvoKernelError::Validation(format!("failed to parse EvoMap capsules snapshot: {err}"))
        })?;

    let mut genes = Vec::new();
    let mut known_gene_ids = BTreeSet::new();
    for source in genes_doc.genes {
        let EvoMapGeneAsset {
            id,
            category,
            signals_match,
            strategy,
            validation,
            constraints,
            model_name,
            schema_version,
            compatibility,
        } = source;
        let gene_id = id.trim();
        if gene_id.is_empty() {
            return Err(EvoKernelError::Validation(
                "EvoMap snapshot gene id must not be empty".into(),
            ));
        }
        if !known_gene_ids.insert(gene_id.to_string()) {
            continue;
        }

        let mut seen_signals = BTreeSet::new();
        let mut signals = Vec::new();
        for signal in signals_match {
            let Some(normalized) = value_as_signal_string(&signal) else {
                continue;
            };
            if seen_signals.insert(normalized.clone()) {
                signals.push(normalized);
            }
        }
        if signals.is_empty() {
            signals.push(format!("gene:{}", gene_id.to_ascii_lowercase()));
        }

        let mut strategy = strategy
            .into_iter()
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .collect::<Vec<_>>();
        if strategy.is_empty() {
            strategy.push("evomap strategy missing in snapshot".into());
        }
        let constraint = constraints.unwrap_or_default();
        let compat_state = compatibility_state_from_value(compatibility.as_ref())
            .unwrap_or_else(|| "candidate".to_string());
        ensure_strategy_metadata(&mut strategy, "asset_origin", "builtin_evomap");
        ensure_strategy_metadata(
            &mut strategy,
            "evomap_category",
            category.as_deref().unwrap_or("unknown"),
        );
        ensure_strategy_metadata(
            &mut strategy,
            "evomap_constraints_max_files",
            &constraint.max_files.unwrap_or_default().to_string(),
        );
        ensure_strategy_metadata(
            &mut strategy,
            "evomap_constraints_forbidden_paths",
            &constraint.forbidden_paths.join("|"),
        );
        ensure_strategy_metadata(
            &mut strategy,
            "evomap_model_name",
            model_name.as_deref().unwrap_or("unknown"),
        );
        ensure_strategy_metadata(
            &mut strategy,
            "evomap_schema_version",
            schema_version.as_deref().unwrap_or("1.5.0"),
        );
        ensure_strategy_metadata(&mut strategy, "evomap_compatibility_state", &compat_state);

        let mut validation = validation
            .into_iter()
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .collect::<Vec<_>>();
        if validation.is_empty() {
            validation.push("evomap-builtin-seed".into());
        }

        genes.push(Gene {
            id: gene_id.to_string(),
            signals,
            strategy,
            validation,
            state: map_evomap_state(compatibility.as_ref()),
            task_class_id: None,
        });
    }

    let mut capsules = Vec::new();
    let known_gene_ids = genes
        .iter()
        .map(|gene| gene.id.clone())
        .collect::<BTreeSet<_>>();
    for source in capsules_doc.capsules {
        let EvoMapCapsuleAsset {
            id,
            gene,
            trigger,
            summary,
            diff,
            confidence,
            outcome,
            blast_radius,
            content,
            env_fingerprint,
            model_name: _model_name,
            schema_version: _schema_version,
            compatibility,
        } = source;
        let source_for_diff = EvoMapCapsuleAsset {
            id: id.clone(),
            gene: gene.clone(),
            trigger: trigger.clone(),
            summary: summary.clone(),
            diff,
            confidence,
            outcome: outcome.clone(),
            blast_radius: blast_radius.clone(),
            content: content.clone(),
            env_fingerprint: env_fingerprint.clone(),
            model_name: None,
            schema_version: None,
            compatibility: compatibility.clone(),
        };
        if !known_gene_ids.contains(gene.as_str()) {
            return Err(EvoKernelError::Validation(format!(
                "EvoMap capsule {} references unknown gene {}",
                id, gene
            )));
        }
        let normalized_diff = normalized_diff_payload(&source_for_diff);
        if normalized_diff.trim().is_empty() {
            return Err(EvoKernelError::Validation(format!(
                "EvoMap capsule {} has empty normalized diff payload",
                id
            )));
        }
        let mut changed_files = content
            .as_ref()
            .map(|content| {
                content
                    .changed_files
                    .iter()
                    .map(|item| item.trim().to_string())
                    .filter(|item| !item.is_empty())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if changed_files.is_empty() {
            changed_files = parse_diff_changed_files(&normalized_diff);
        }
        if changed_files.is_empty() {
            changed_files.push(format!("docs/evomap_builtin_capsules/{}.md", id));
        }

        let confidence = confidence
            .or_else(|| outcome.as_ref().and_then(|outcome| outcome.score))
            .unwrap_or(0.6)
            .clamp(0.0, 1.0);
        let status_success = outcome
            .as_ref()
            .and_then(|outcome| outcome.status.as_deref())
            .map(|status| status.eq_ignore_ascii_case("success"))
            .unwrap_or(true);
        let blast_radius = blast_radius.unwrap_or_default();
        let mutation_id = format!("builtin-evomap-mutation-{}", id);
        let intent = MutationIntent {
            id: mutation_id.clone(),
            intent: if summary.trim().is_empty() {
                format!("apply EvoMap capsule {}", id)
            } else {
                summary.trim().to_string()
            },
            target: MutationTarget::Paths {
                allow: changed_files.clone(),
            },
            expected_effect: format!("seed replay candidate from EvoMap capsule {}", id),
            risk: RiskLevel::Low,
            signals: if trigger.is_empty() {
                vec![format!("capsule:{}", id.to_ascii_lowercase())]
            } else {
                trigger
                    .iter()
                    .map(|signal| signal.trim().to_ascii_lowercase())
                    .filter(|signal| !signal.is_empty())
                    .collect::<Vec<_>>()
            },
            spec_id: None,
        };
        let mutation = PreparedMutation {
            intent,
            artifact: oris_evolution::MutationArtifact {
                encoding: ArtifactEncoding::UnifiedDiff,
                payload: normalized_diff.clone(),
                base_revision: None,
                content_hash: compute_artifact_hash(&normalized_diff),
            },
        };
        let capsule = Capsule {
            id: id.clone(),
            gene_id: gene.clone(),
            mutation_id,
            run_id: EVOMAP_BUILTIN_RUN_ID.to_string(),
            diff_hash: compute_artifact_hash(&normalized_diff),
            confidence,
            env: map_evomap_env_fingerprint(env_fingerprint.as_ref()),
            outcome: Outcome {
                success: status_success,
                validation_profile: "evomap-builtin-seed".into(),
                validation_duration_ms: 0,
                changed_files,
                validator_hash: "builtin-evomap".into(),
                lines_changed: blast_radius.lines,
                replay_verified: false,
            },
            state: map_evomap_state(compatibility.as_ref()),
        };
        capsules.push(BuiltinCapsuleSeed { capsule, mutation });
    }

    Ok(Some(BuiltinAssetBundle { genes, capsules }))
}

fn ensure_builtin_experience_assets_in_store(
    store: &dyn EvolutionStore,
    sender_id: String,
) -> Result<ImportOutcome, EvoKernelError> {
    let (events, projection) = scan_projection(store)?;
    let mut known_gene_ids = projection
        .genes
        .into_iter()
        .map(|gene| gene.id)
        .collect::<BTreeSet<_>>();
    let mut known_capsule_ids = projection
        .capsules
        .into_iter()
        .map(|capsule| capsule.id)
        .collect::<BTreeSet<_>>();
    let mut known_mutation_ids = BTreeSet::new();
    for stored in &events {
        if let EvolutionEvent::MutationDeclared { mutation } = &stored.event {
            known_mutation_ids.insert(mutation.intent.id.clone());
        }
    }
    let normalized_sender = normalized_sender_id(&sender_id);
    let mut imported_asset_ids = Vec::new();
    // Keep legacy compatibility templates available even when EvoMap snapshots
    // are present, so A2A compatibility fetch flows retain stable builtin IDs.
    let mut bundle = BuiltinAssetBundle {
        genes: built_in_experience_genes(),
        capsules: Vec::new(),
    };
    if let Some(snapshot_bundle) = load_evomap_builtin_assets()? {
        bundle.genes.extend(snapshot_bundle.genes);
        bundle.capsules.extend(snapshot_bundle.capsules);
    }
    let scanned_count = bundle.genes.len() + bundle.capsules.len();

    for gene in bundle.genes {
        if !known_gene_ids.insert(gene.id.clone()) {
            continue;
        }

        store
            .append_event(EvolutionEvent::RemoteAssetImported {
                source: CandidateSource::Local,
                asset_ids: vec![gene.id.clone()],
                sender_id: normalized_sender.clone(),
            })
            .map_err(store_err)?;
        store
            .append_event(EvolutionEvent::GeneProjected { gene: gene.clone() })
            .map_err(store_err)?;
        match gene.state {
            AssetState::Revoked | AssetState::Archived => {}
            AssetState::Quarantined | AssetState::ShadowValidated => {
                store
                    .append_event(EvolutionEvent::PromotionEvaluated {
                        gene_id: gene.id.clone(),
                        state: AssetState::Quarantined,
                        reason:
                            "built-in EvoMap asset requires additional validation before promotion"
                                .into(),
                        reason_code: TransitionReasonCode::DowngradeBuiltinRequiresValidation,
                        evidence: None,
                    })
                    .map_err(store_err)?;
            }
            AssetState::Promoted | AssetState::Candidate => {
                store
                    .append_event(EvolutionEvent::PromotionEvaluated {
                        gene_id: gene.id.clone(),
                        state: AssetState::Promoted,
                        reason: "built-in experience asset promoted for cold-start compatibility"
                            .into(),
                        reason_code: TransitionReasonCode::PromotionBuiltinColdStartCompatibility,
                        evidence: None,
                    })
                    .map_err(store_err)?;
                store
                    .append_event(EvolutionEvent::GenePromoted {
                        gene_id: gene.id.clone(),
                    })
                    .map_err(store_err)?;
            }
        }
        imported_asset_ids.push(gene.id.clone());
    }

    for seed in bundle.capsules {
        if !known_gene_ids.contains(seed.capsule.gene_id.as_str()) {
            return Err(EvoKernelError::Validation(format!(
                "built-in capsule {} references unknown gene {}",
                seed.capsule.id, seed.capsule.gene_id
            )));
        }
        if known_mutation_ids.insert(seed.mutation.intent.id.clone()) {
            store
                .append_event(EvolutionEvent::MutationDeclared {
                    mutation: seed.mutation.clone(),
                })
                .map_err(store_err)?;
        }
        if !known_capsule_ids.insert(seed.capsule.id.clone()) {
            continue;
        }
        store
            .append_event(EvolutionEvent::RemoteAssetImported {
                source: CandidateSource::Local,
                asset_ids: vec![seed.capsule.id.clone()],
                sender_id: normalized_sender.clone(),
            })
            .map_err(store_err)?;
        store
            .append_event(EvolutionEvent::CapsuleCommitted {
                capsule: seed.capsule.clone(),
            })
            .map_err(store_err)?;
        match seed.capsule.state {
            AssetState::Revoked | AssetState::Archived => {}
            AssetState::Quarantined | AssetState::ShadowValidated => {
                store
                    .append_event(EvolutionEvent::CapsuleQuarantined {
                        capsule_id: seed.capsule.id.clone(),
                    })
                    .map_err(store_err)?;
            }
            AssetState::Promoted | AssetState::Candidate => {
                store
                    .append_event(EvolutionEvent::CapsuleReleased {
                        capsule_id: seed.capsule.id.clone(),
                        state: AssetState::Promoted,
                    })
                    .map_err(store_err)?;
            }
        }
        imported_asset_ids.push(seed.capsule.id.clone());
    }

    let next_cursor = latest_store_cursor(store)?;
    let resume_token = next_cursor.as_ref().and_then(|cursor| {
        normalized_sender
            .as_deref()
            .map(|sender| encode_resume_token(sender, cursor))
    });
    let applied_count = imported_asset_ids.len();
    let skipped_count = scanned_count.saturating_sub(applied_count);

    Ok(ImportOutcome {
        imported_asset_ids,
        accepted: true,
        next_cursor: next_cursor.clone(),
        resume_token,
        sync_audit: SyncAudit {
            batch_id: next_id("sync-import"),
            requested_cursor: None,
            scanned_count,
            applied_count,
            skipped_count,
            failed_count: 0,
            failure_reasons: Vec::new(),
        },
    })
}

fn strategy_metadata_value(strategy: &[String], key: &str) -> Option<String> {
    strategy.iter().find_map(|entry| {
        let (entry_key, entry_value) = entry.split_once('=')?;
        if entry_key.trim().eq_ignore_ascii_case(key) {
            let normalized = entry_value.trim();
            if normalized.is_empty() {
                None
            } else {
                Some(normalized.to_string())
            }
        } else {
            None
        }
    })
}

fn ensure_strategy_metadata(strategy: &mut Vec<String>, key: &str, value: &str) {
    let normalized = value.trim();
    if normalized.is_empty() || strategy_metadata_value(strategy, key).is_some() {
        return;
    }
    strategy.push(format!("{key}={normalized}"));
}

fn enforce_reported_experience_retention(
    store: &dyn EvolutionStore,
    task_class: &str,
    keep_latest: usize,
) -> Result<(), EvoKernelError> {
    let task_class = task_class.trim();
    if task_class.is_empty() || keep_latest == 0 {
        return Ok(());
    }

    let (_, projection) = scan_projection(store)?;
    let mut candidates = projection
        .genes
        .iter()
        .filter(|gene| gene.state == AssetState::Promoted)
        .filter_map(|gene| {
            let origin = strategy_metadata_value(&gene.strategy, "asset_origin")?;
            if !origin.eq_ignore_ascii_case("reported_experience") {
                return None;
            }
            let gene_task_class = strategy_metadata_value(&gene.strategy, "task_class")?;
            if !gene_task_class.eq_ignore_ascii_case(task_class) {
                return None;
            }
            let updated_at = projection
                .last_updated_at
                .get(&gene.id)
                .cloned()
                .unwrap_or_default();
            Some((gene.id.clone(), updated_at))
        })
        .collect::<Vec<_>>();
    if candidates.len() <= keep_latest {
        return Ok(());
    }

    candidates.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| right.0.cmp(&left.0)));
    let stale_gene_ids = candidates
        .into_iter()
        .skip(keep_latest)
        .map(|(gene_id, _)| gene_id)
        .collect::<BTreeSet<_>>();
    if stale_gene_ids.is_empty() {
        return Ok(());
    }

    let reason =
        format!("reported experience retention limit exceeded for task_class={task_class}");
    for gene_id in &stale_gene_ids {
        store
            .append_event(EvolutionEvent::GeneRevoked {
                gene_id: gene_id.clone(),
                reason: reason.clone(),
            })
            .map_err(store_err)?;
    }

    let stale_capsule_ids = projection
        .capsules
        .iter()
        .filter(|capsule| stale_gene_ids.contains(&capsule.gene_id))
        .map(|capsule| capsule.id.clone())
        .collect::<BTreeSet<_>>();
    for capsule_id in stale_capsule_ids {
        store
            .append_event(EvolutionEvent::CapsuleQuarantined { capsule_id })
            .map_err(store_err)?;
    }
    Ok(())
}

fn record_reported_experience_in_store(
    store: &dyn EvolutionStore,
    sender_id: String,
    gene_id: String,
    signals: Vec<String>,
    strategy: Vec<String>,
    validation: Vec<String>,
) -> Result<ImportOutcome, EvoKernelError> {
    let gene_id = gene_id.trim();
    if gene_id.is_empty() {
        return Err(EvoKernelError::Validation(
            "reported experience gene_id must not be empty".into(),
        ));
    }

    let mut unique_signals = BTreeSet::new();
    let mut normalized_signals = Vec::new();
    for signal in signals {
        let normalized = signal.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            continue;
        }
        if unique_signals.insert(normalized.clone()) {
            normalized_signals.push(normalized);
        }
    }
    if normalized_signals.is_empty() {
        return Err(EvoKernelError::Validation(
            "reported experience signals must not be empty".into(),
        ));
    }

    let mut unique_strategy = BTreeSet::new();
    let mut normalized_strategy = Vec::new();
    for entry in strategy {
        let normalized = entry.trim().to_string();
        if normalized.is_empty() {
            continue;
        }
        if unique_strategy.insert(normalized.clone()) {
            normalized_strategy.push(normalized);
        }
    }
    if normalized_strategy.is_empty() {
        normalized_strategy.push("reported local replay experience".into());
    }
    let task_class_id = strategy_metadata_value(&normalized_strategy, "task_class")
        .or_else(|| normalized_signals.first().cloned())
        .unwrap_or_else(|| "reported-experience".into());
    let task_label = strategy_metadata_value(&normalized_strategy, "task_label")
        .or_else(|| normalized_signals.first().cloned())
        .unwrap_or_else(|| task_class_id.clone());
    ensure_strategy_metadata(
        &mut normalized_strategy,
        "asset_origin",
        "reported_experience",
    );
    ensure_strategy_metadata(&mut normalized_strategy, "task_class", &task_class_id);
    ensure_strategy_metadata(&mut normalized_strategy, "task_label", &task_label);

    let mut unique_validation = BTreeSet::new();
    let mut normalized_validation = Vec::new();
    for entry in validation {
        let normalized = entry.trim().to_string();
        if normalized.is_empty() {
            continue;
        }
        if unique_validation.insert(normalized.clone()) {
            normalized_validation.push(normalized);
        }
    }
    if normalized_validation.is_empty() {
        normalized_validation.push("a2a.tasks.report".into());
    }

    let gene = Gene {
        id: gene_id.to_string(),
        signals: normalized_signals,
        strategy: normalized_strategy,
        validation: normalized_validation,
        state: AssetState::Promoted,
        task_class_id: None,
    };
    let normalized_sender = normalized_sender_id(&sender_id);

    store
        .append_event(EvolutionEvent::RemoteAssetImported {
            source: CandidateSource::Local,
            asset_ids: vec![gene.id.clone()],
            sender_id: normalized_sender.clone(),
        })
        .map_err(store_err)?;
    store
        .append_event(EvolutionEvent::GeneProjected { gene: gene.clone() })
        .map_err(store_err)?;
    store
        .append_event(EvolutionEvent::PromotionEvaluated {
            gene_id: gene.id.clone(),
            state: AssetState::Promoted,
            reason: "trusted local report promoted reusable experience".into(),
            reason_code: TransitionReasonCode::PromotionTrustedLocalReport,
            evidence: None,
        })
        .map_err(store_err)?;
    store
        .append_event(EvolutionEvent::GenePromoted {
            gene_id: gene.id.clone(),
        })
        .map_err(store_err)?;
    enforce_reported_experience_retention(
        store,
        &task_class_id,
        REPORTED_EXPERIENCE_RETENTION_LIMIT,
    )?;

    let imported_asset_ids = vec![gene.id];
    let next_cursor = latest_store_cursor(store)?;
    let resume_token = next_cursor.as_ref().and_then(|cursor| {
        normalized_sender
            .as_deref()
            .map(|sender| encode_resume_token(sender, cursor))
    });
    Ok(ImportOutcome {
        imported_asset_ids,
        accepted: true,
        next_cursor,
        resume_token,
        sync_audit: SyncAudit {
            batch_id: next_id("sync-import"),
            requested_cursor: None,
            scanned_count: 1,
            applied_count: 1,
            skipped_count: 0,
            failed_count: 0,
            failure_reasons: Vec::new(),
        },
    })
}

fn normalized_sender_id(sender_id: &str) -> Option<String> {
    let trimmed = sender_id.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn normalized_asset_ids(asset_ids: &[String]) -> BTreeSet<String> {
    asset_ids
        .iter()
        .map(|asset_id| asset_id.trim().to_string())
        .filter(|asset_id| !asset_id.is_empty())
        .collect()
}

fn validate_remote_revoke_notice_assets(
    store: &dyn EvolutionStore,
    notice: &RevokeNotice,
) -> Result<(String, BTreeSet<String>), EvoKernelError> {
    let sender_id = normalized_sender_id(&notice.sender_id).ok_or_else(|| {
        EvoKernelError::Validation("revoke notice sender_id must not be empty".into())
    })?;
    let requested = normalized_asset_ids(&notice.asset_ids);
    if requested.is_empty() {
        return Ok((sender_id, requested));
    }

    let remote_publishers = remote_publishers_by_asset_from_store(store);
    let has_remote_assets = requested
        .iter()
        .any(|asset_id| remote_publishers.contains_key(asset_id));
    if !has_remote_assets {
        return Ok((sender_id, requested));
    }

    let unauthorized = requested
        .iter()
        .filter(|asset_id| {
            remote_publishers.get(*asset_id).map(String::as_str) != Some(sender_id.as_str())
        })
        .cloned()
        .collect::<Vec<_>>();
    if !unauthorized.is_empty() {
        return Err(EvoKernelError::Validation(format!(
            "remote revoke notice contains assets not owned by sender {sender_id}: {}",
            unauthorized.join(", ")
        )));
    }

    Ok((sender_id, requested))
}

fn replay_failure_revocation_summary(
    replay_failures: u64,
    current_confidence: f32,
    historical_peak_confidence: f32,
    source_sender_id: Option<&str>,
) -> String {
    let source_sender_id = source_sender_id.unwrap_or("unavailable");
    format!(
        "phase=replay_failure_revocation; source_sender_id={source_sender_id}; replay_failures={replay_failures}; current_confidence={current_confidence:.3}; historical_peak_confidence={historical_peak_confidence:.3}"
    )
}

fn record_manifest_validation(
    store: &dyn EvolutionStore,
    envelope: &EvolutionEnvelope,
    accepted: bool,
    reason: impl Into<String>,
) -> Result<(), EvoKernelError> {
    let manifest = envelope.manifest.as_ref();
    let sender_id = manifest
        .and_then(|value| normalized_sender_id(&value.sender_id))
        .or_else(|| normalized_sender_id(&envelope.sender_id));
    let publisher = manifest.and_then(|value| normalized_sender_id(&value.publisher));
    let asset_ids = manifest
        .map(|value| value.asset_ids.clone())
        .unwrap_or_else(|| EvolutionEnvelope::manifest_asset_ids(&envelope.assets));

    store
        .append_event(EvolutionEvent::ManifestValidated {
            accepted,
            reason: reason.into(),
            sender_id,
            publisher,
            asset_ids,
        })
        .map_err(store_err)?;
    Ok(())
}

fn record_remote_publisher_for_asset(
    remote_publishers: Option<&Mutex<BTreeMap<String, String>>>,
    sender_id: &str,
    asset: &NetworkAsset,
) {
    let Some(remote_publishers) = remote_publishers else {
        return;
    };
    let sender_id = sender_id.trim();
    if sender_id.is_empty() {
        return;
    }
    let Ok(mut publishers) = remote_publishers.lock() else {
        return;
    };
    match asset {
        NetworkAsset::Gene { gene } => {
            publishers.insert(gene.id.clone(), sender_id.to_string());
        }
        NetworkAsset::Capsule { capsule } => {
            publishers.insert(capsule.id.clone(), sender_id.to_string());
        }
        NetworkAsset::EvolutionEvent { .. } => {}
    }
}

fn remote_publishers_by_asset_from_store(store: &dyn EvolutionStore) -> BTreeMap<String, String> {
    let Ok(events) = store.scan(1) else {
        return BTreeMap::new();
    };
    remote_publishers_by_asset_from_events(&events)
}

fn remote_publishers_by_asset_from_events(
    events: &[StoredEvolutionEvent],
) -> BTreeMap<String, String> {
    let mut imported_asset_publishers = BTreeMap::<String, String>::new();
    let mut known_gene_ids = BTreeSet::<String>::new();
    let mut known_capsule_ids = BTreeSet::<String>::new();
    let mut publishers_by_asset = BTreeMap::<String, String>::new();

    for stored in events {
        match &stored.event {
            EvolutionEvent::RemoteAssetImported {
                source: CandidateSource::Remote,
                asset_ids,
                sender_id,
            } => {
                let Some(sender_id) = sender_id.as_deref().and_then(normalized_sender_id) else {
                    continue;
                };
                for asset_id in asset_ids {
                    imported_asset_publishers.insert(asset_id.clone(), sender_id.clone());
                    if known_gene_ids.contains(asset_id) || known_capsule_ids.contains(asset_id) {
                        publishers_by_asset.insert(asset_id.clone(), sender_id.clone());
                    }
                }
            }
            EvolutionEvent::GeneProjected { gene } => {
                known_gene_ids.insert(gene.id.clone());
                if let Some(sender_id) = imported_asset_publishers.get(&gene.id) {
                    publishers_by_asset.insert(gene.id.clone(), sender_id.clone());
                }
            }
            EvolutionEvent::CapsuleCommitted { capsule } => {
                known_capsule_ids.insert(capsule.id.clone());
                if let Some(sender_id) = imported_asset_publishers.get(&capsule.id) {
                    publishers_by_asset.insert(capsule.id.clone(), sender_id.clone());
                }
            }
            _ => {}
        }
    }

    publishers_by_asset
}

fn should_import_remote_event(event: &EvolutionEvent) -> bool {
    matches!(
        event,
        EvolutionEvent::MutationDeclared { .. } | EvolutionEvent::SpecLinked { .. }
    )
}

fn fetch_assets_from_store(
    store: &dyn EvolutionStore,
    responder_id: impl Into<String>,
    query: &FetchQuery,
) -> Result<FetchResponse, EvoKernelError> {
    let (events, projection) = scan_projection(store)?;
    let requested_cursor = resolve_requested_cursor(
        &query.sender_id,
        query.since_cursor.as_deref(),
        query.resume_token.as_deref(),
    )?;
    let since_seq = requested_cursor
        .as_deref()
        .and_then(parse_sync_cursor_seq)
        .unwrap_or(0);
    let normalized_signals: Vec<String> = query
        .signals
        .iter()
        .map(|signal| signal.trim().to_ascii_lowercase())
        .filter(|signal| !signal.is_empty())
        .collect();
    let matches_any_signal = |candidate: &str| {
        if normalized_signals.is_empty() {
            return true;
        }
        let candidate = candidate.to_ascii_lowercase();
        normalized_signals
            .iter()
            .any(|signal| candidate.contains(signal) || signal.contains(&candidate))
    };

    let matched_genes: Vec<Gene> = projection
        .genes
        .into_iter()
        .filter(|gene| gene.state == AssetState::Promoted)
        .filter(|gene| gene.signals.iter().any(|signal| matches_any_signal(signal)))
        .collect();
    let matched_gene_ids: BTreeSet<String> =
        matched_genes.iter().map(|gene| gene.id.clone()).collect();
    let matched_capsules: Vec<Capsule> = projection
        .capsules
        .into_iter()
        .filter(|capsule| capsule.state == AssetState::Promoted)
        .filter(|capsule| matched_gene_ids.contains(&capsule.gene_id))
        .collect();
    let all_assets = replay_export_assets(&events, matched_genes.clone(), matched_capsules.clone());
    let (selected_genes, selected_capsules) = if requested_cursor.is_some() {
        let delta = delta_window(&events, since_seq);
        let selected_capsules = matched_capsules
            .into_iter()
            .filter(|capsule| {
                delta.changed_capsule_ids.contains(&capsule.id)
                    || delta.changed_mutation_ids.contains(&capsule.mutation_id)
            })
            .collect::<Vec<_>>();
        let selected_gene_ids = selected_capsules
            .iter()
            .map(|capsule| capsule.gene_id.clone())
            .collect::<BTreeSet<_>>();
        let selected_genes = matched_genes
            .into_iter()
            .filter(|gene| {
                delta.changed_gene_ids.contains(&gene.id) || selected_gene_ids.contains(&gene.id)
            })
            .collect::<Vec<_>>();
        (selected_genes, selected_capsules)
    } else {
        (matched_genes, matched_capsules)
    };
    let assets = replay_export_assets(&events, selected_genes, selected_capsules);
    let next_cursor = events.last().map(|stored| format_sync_cursor(stored.seq));
    let resume_token = next_cursor
        .as_ref()
        .map(|cursor| encode_resume_token(&query.sender_id, cursor));
    let applied_count = assets.len();
    let skipped_count = all_assets.len().saturating_sub(applied_count);

    Ok(FetchResponse {
        sender_id: responder_id.into(),
        assets,
        next_cursor: next_cursor.clone(),
        resume_token,
        sync_audit: SyncAudit {
            batch_id: next_id("sync-fetch"),
            requested_cursor,
            scanned_count: all_assets.len(),
            applied_count,
            skipped_count,
            failed_count: 0,
            failure_reasons: Vec::new(),
        },
    })
}

fn revoke_assets_in_store(
    store: &dyn EvolutionStore,
    notice: &RevokeNotice,
) -> Result<RevokeNotice, EvoKernelError> {
    let projection = projection_snapshot(store)?;
    let (sender_id, requested) = validate_remote_revoke_notice_assets(store, notice)?;
    let mut revoked_gene_ids = BTreeSet::new();
    let mut quarantined_capsule_ids = BTreeSet::new();

    for gene in &projection.genes {
        if requested.contains(&gene.id) {
            revoked_gene_ids.insert(gene.id.clone());
        }
    }
    for capsule in &projection.capsules {
        if requested.contains(&capsule.id) {
            quarantined_capsule_ids.insert(capsule.id.clone());
            revoked_gene_ids.insert(capsule.gene_id.clone());
        }
    }
    for capsule in &projection.capsules {
        if revoked_gene_ids.contains(&capsule.gene_id) {
            quarantined_capsule_ids.insert(capsule.id.clone());
        }
    }

    for gene_id in &revoked_gene_ids {
        store
            .append_event(EvolutionEvent::GeneRevoked {
                gene_id: gene_id.clone(),
                reason: notice.reason.clone(),
            })
            .map_err(store_err)?;
    }
    for capsule_id in &quarantined_capsule_ids {
        store
            .append_event(EvolutionEvent::CapsuleQuarantined {
                capsule_id: capsule_id.clone(),
            })
            .map_err(store_err)?;
    }

    let mut affected_ids: Vec<String> = revoked_gene_ids.into_iter().collect();
    affected_ids.extend(quarantined_capsule_ids);
    affected_ids.sort();
    affected_ids.dedup();

    Ok(RevokeNotice {
        sender_id,
        asset_ids: affected_ids,
        reason: notice.reason.clone(),
    })
}

fn evolution_metrics_snapshot(
    store: &dyn EvolutionStore,
) -> Result<EvolutionMetricsSnapshot, EvoKernelError> {
    let (events, projection) = scan_projection(store)?;
    let replay = collect_replay_roi_aggregate(&events, &projection, None);
    let replay_reasoning_avoided_total = replay.replay_success_total;
    let confidence_revalidations_total = events
        .iter()
        .filter(|stored| is_confidence_revalidation_event(&stored.event))
        .count() as u64;
    let mutation_declared_total = events
        .iter()
        .filter(|stored| matches!(stored.event, EvolutionEvent::MutationDeclared { .. }))
        .count() as u64;
    let promoted_mutations_total = events
        .iter()
        .filter(|stored| matches!(stored.event, EvolutionEvent::GenePromoted { .. }))
        .count() as u64;
    let gene_revocations_total = events
        .iter()
        .filter(|stored| matches!(stored.event, EvolutionEvent::GeneRevoked { .. }))
        .count() as u64;
    let cutoff = Utc::now() - Duration::hours(1);
    let mutation_velocity_last_hour = count_recent_events(&events, cutoff, |event| {
        matches!(event, EvolutionEvent::MutationDeclared { .. })
    });
    let revoke_frequency_last_hour = count_recent_events(&events, cutoff, |event| {
        matches!(event, EvolutionEvent::GeneRevoked { .. })
    });
    let promoted_genes = projection
        .genes
        .iter()
        .filter(|gene| gene.state == AssetState::Promoted)
        .count() as u64;
    let promoted_capsules = projection
        .capsules
        .iter()
        .filter(|capsule| capsule.state == AssetState::Promoted)
        .count() as u64;

    Ok(EvolutionMetricsSnapshot {
        replay_attempts_total: replay.replay_attempts_total,
        replay_success_total: replay.replay_success_total,
        replay_success_rate: safe_ratio(replay.replay_success_total, replay.replay_attempts_total),
        confidence_revalidations_total,
        replay_reasoning_avoided_total,
        reasoning_avoided_tokens_total: replay.reasoning_avoided_tokens_total,
        replay_fallback_cost_total: replay.replay_fallback_cost_total,
        replay_roi: compute_replay_roi(
            replay.reasoning_avoided_tokens_total,
            replay.replay_fallback_cost_total,
        ),
        replay_task_classes: replay.replay_task_classes,
        replay_sources: replay.replay_sources,
        mutation_declared_total,
        promoted_mutations_total,
        promotion_ratio: safe_ratio(promoted_mutations_total, mutation_declared_total),
        gene_revocations_total,
        mutation_velocity_last_hour,
        revoke_frequency_last_hour,
        promoted_genes,
        promoted_capsules,
        last_event_seq: events.last().map(|stored| stored.seq).unwrap_or(0),
    })
}

struct ReplayRoiAggregate {
    replay_attempts_total: u64,
    replay_success_total: u64,
    replay_failure_total: u64,
    reasoning_avoided_tokens_total: u64,
    replay_fallback_cost_total: u64,
    replay_task_classes: Vec<ReplayTaskClassMetrics>,
    replay_sources: Vec<ReplaySourceRoiMetrics>,
}

fn collect_replay_roi_aggregate(
    events: &[StoredEvolutionEvent],
    projection: &EvolutionProjection,
    cutoff: Option<DateTime<Utc>>,
) -> ReplayRoiAggregate {
    let replay_evidences = events
        .iter()
        .filter(|stored| replay_event_in_scope(stored, cutoff))
        .filter_map(|stored| match &stored.event {
            EvolutionEvent::ReplayEconomicsRecorded { evidence, .. } => Some(evidence.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();

    let mut task_totals = BTreeMap::<(String, String), (u64, u64, u64, u64)>::new();
    let mut source_totals = BTreeMap::<String, (u64, u64, u64, u64)>::new();

    let (
        replay_success_total,
        replay_failure_total,
        reasoning_avoided_tokens_total,
        replay_fallback_cost_total,
    ) = if replay_evidences.is_empty() {
        let gene_task_classes = projection
            .genes
            .iter()
            .map(|gene| (gene.id.clone(), replay_task_descriptor(&gene.signals)))
            .collect::<BTreeMap<_, _>>();
        let mut replay_success_total = 0_u64;
        let mut replay_failure_total = 0_u64;

        for stored in events
            .iter()
            .filter(|stored| replay_event_in_scope(stored, cutoff))
        {
            match &stored.event {
                EvolutionEvent::CapsuleReused { gene_id, .. } => {
                    replay_success_total += 1;
                    if let Some((task_class_id, task_label)) = gene_task_classes.get(gene_id) {
                        let entry = task_totals
                            .entry((task_class_id.clone(), task_label.clone()))
                            .or_insert((0, 0, 0, 0));
                        entry.0 += 1;
                        entry.2 += REPLAY_REASONING_TOKEN_FLOOR;
                    }
                }
                event if is_replay_validation_failure(event) => {
                    replay_failure_total += 1;
                }
                _ => {}
            }
        }

        (
            replay_success_total,
            replay_failure_total,
            replay_success_total * REPLAY_REASONING_TOKEN_FLOOR,
            replay_failure_total * REPLAY_REASONING_TOKEN_FLOOR,
        )
    } else {
        let mut replay_success_total = 0_u64;
        let mut replay_failure_total = 0_u64;
        let mut reasoning_avoided_tokens_total = 0_u64;
        let mut replay_fallback_cost_total = 0_u64;

        for evidence in &replay_evidences {
            if evidence.success {
                replay_success_total += 1;
            } else {
                replay_failure_total += 1;
            }
            reasoning_avoided_tokens_total += evidence.reasoning_avoided_tokens;
            replay_fallback_cost_total += evidence.replay_fallback_cost;

            let entry = task_totals
                .entry((evidence.task_class_id.clone(), evidence.task_label.clone()))
                .or_insert((0, 0, 0, 0));
            if evidence.success {
                entry.0 += 1;
            } else {
                entry.1 += 1;
            }
            entry.2 += evidence.reasoning_avoided_tokens;
            entry.3 += evidence.replay_fallback_cost;

            if let Some(source_sender_id) = evidence.source_sender_id.as_deref() {
                let source_entry = source_totals
                    .entry(source_sender_id.to_string())
                    .or_insert((0, 0, 0, 0));
                if evidence.success {
                    source_entry.0 += 1;
                } else {
                    source_entry.1 += 1;
                }
                source_entry.2 += evidence.reasoning_avoided_tokens;
                source_entry.3 += evidence.replay_fallback_cost;
            }
        }

        (
            replay_success_total,
            replay_failure_total,
            reasoning_avoided_tokens_total,
            replay_fallback_cost_total,
        )
    };

    let replay_task_classes = task_totals
        .into_iter()
        .map(
            |(
                (task_class_id, task_label),
                (
                    replay_success_total,
                    replay_failure_total,
                    reasoning_avoided_tokens_total,
                    replay_fallback_cost_total,
                ),
            )| ReplayTaskClassMetrics {
                task_class_id,
                task_label,
                replay_success_total,
                replay_failure_total,
                reasoning_steps_avoided_total: replay_success_total,
                reasoning_avoided_tokens_total,
                replay_fallback_cost_total,
                replay_roi: compute_replay_roi(
                    reasoning_avoided_tokens_total,
                    replay_fallback_cost_total,
                ),
            },
        )
        .collect::<Vec<_>>();
    let replay_sources = source_totals
        .into_iter()
        .map(
            |(
                source_sender_id,
                (
                    replay_success_total,
                    replay_failure_total,
                    reasoning_avoided_tokens_total,
                    replay_fallback_cost_total,
                ),
            )| ReplaySourceRoiMetrics {
                source_sender_id,
                replay_success_total,
                replay_failure_total,
                reasoning_avoided_tokens_total,
                replay_fallback_cost_total,
                replay_roi: compute_replay_roi(
                    reasoning_avoided_tokens_total,
                    replay_fallback_cost_total,
                ),
            },
        )
        .collect::<Vec<_>>();

    ReplayRoiAggregate {
        replay_attempts_total: replay_success_total + replay_failure_total,
        replay_success_total,
        replay_failure_total,
        reasoning_avoided_tokens_total,
        replay_fallback_cost_total,
        replay_task_classes,
        replay_sources,
    }
}

fn replay_event_in_scope(stored: &StoredEvolutionEvent, cutoff: Option<DateTime<Utc>>) -> bool {
    match cutoff {
        Some(cutoff) => parse_event_timestamp(&stored.timestamp)
            .map(|timestamp| timestamp >= cutoff)
            .unwrap_or(false),
        None => true,
    }
}

fn replay_roi_release_gate_summary(
    store: &dyn EvolutionStore,
    window_seconds: u64,
) -> Result<ReplayRoiWindowSummary, EvoKernelError> {
    let (events, projection) = scan_projection(store)?;
    let now = Utc::now();
    let cutoff = if window_seconds == 0 {
        None
    } else {
        let seconds = i64::try_from(window_seconds).unwrap_or(i64::MAX);
        Some(now - Duration::seconds(seconds))
    };
    let replay = collect_replay_roi_aggregate(&events, &projection, cutoff);

    Ok(ReplayRoiWindowSummary {
        generated_at: now.to_rfc3339(),
        window_seconds,
        replay_attempts_total: replay.replay_attempts_total,
        replay_success_total: replay.replay_success_total,
        replay_failure_total: replay.replay_failure_total,
        reasoning_avoided_tokens_total: replay.reasoning_avoided_tokens_total,
        replay_fallback_cost_total: replay.replay_fallback_cost_total,
        replay_roi: compute_replay_roi(
            replay.reasoning_avoided_tokens_total,
            replay.replay_fallback_cost_total,
        ),
        replay_task_classes: replay.replay_task_classes,
        replay_sources: replay.replay_sources,
    })
}

fn replay_roi_release_gate_contract(
    summary: &ReplayRoiWindowSummary,
    thresholds: ReplayRoiReleaseGateThresholds,
) -> ReplayRoiReleaseGateContract {
    let input = replay_roi_release_gate_input_contract(summary, thresholds);
    let output = evaluate_replay_roi_release_gate_contract_input(&input);
    ReplayRoiReleaseGateContract { input, output }
}

fn replay_roi_release_gate_input_contract(
    summary: &ReplayRoiWindowSummary,
    thresholds: ReplayRoiReleaseGateThresholds,
) -> ReplayRoiReleaseGateInputContract {
    let replay_safety_signal = replay_roi_release_gate_safety_signal(summary);
    let replay_safety = replay_safety_signal.fail_closed_default
        && replay_safety_signal.rollback_ready
        && replay_safety_signal.audit_trail_complete
        && replay_safety_signal.has_replay_activity;
    ReplayRoiReleaseGateInputContract {
        generated_at: summary.generated_at.clone(),
        window_seconds: summary.window_seconds,
        aggregation_dimensions: REPLAY_RELEASE_GATE_AGGREGATION_DIMENSIONS
            .iter()
            .map(|dimension| (*dimension).to_string())
            .collect(),
        replay_attempts_total: summary.replay_attempts_total,
        replay_success_total: summary.replay_success_total,
        replay_failure_total: summary.replay_failure_total,
        replay_hit_rate: safe_ratio(summary.replay_success_total, summary.replay_attempts_total),
        false_replay_rate: safe_ratio(summary.replay_failure_total, summary.replay_attempts_total),
        reasoning_avoided_tokens: summary.reasoning_avoided_tokens_total,
        replay_fallback_cost_total: summary.replay_fallback_cost_total,
        replay_roi: summary.replay_roi,
        replay_safety,
        replay_safety_signal,
        thresholds,
        fail_closed_policy: ReplayRoiReleaseGateFailClosedPolicy::default(),
    }
}

fn replay_roi_release_gate_safety_signal(
    summary: &ReplayRoiWindowSummary,
) -> ReplayRoiReleaseGateSafetySignal {
    ReplayRoiReleaseGateSafetySignal {
        fail_closed_default: true,
        rollback_ready: summary.replay_failure_total == 0 || summary.replay_fallback_cost_total > 0,
        audit_trail_complete: summary.replay_attempts_total
            == summary.replay_success_total + summary.replay_failure_total,
        has_replay_activity: summary.replay_attempts_total > 0,
    }
}

pub fn evaluate_replay_roi_release_gate_contract_input(
    input: &ReplayRoiReleaseGateInputContract,
) -> ReplayRoiReleaseGateOutputContract {
    let mut failed_checks = Vec::new();
    let mut evidence_refs = Vec::new();
    let mut indeterminate = false;

    replay_release_gate_push_unique(&mut evidence_refs, "replay_roi_release_gate_summary");
    replay_release_gate_push_unique(
        &mut evidence_refs,
        format!("window_seconds:{}", input.window_seconds),
    );
    if input.generated_at.trim().is_empty() {
        replay_release_gate_record_failed_check(
            &mut failed_checks,
            &mut evidence_refs,
            "missing_generated_at",
            &["field:generated_at"],
        );
        indeterminate = true;
    } else {
        replay_release_gate_push_unique(
            &mut evidence_refs,
            format!("generated_at:{}", input.generated_at),
        );
    }

    let expected_attempts_total = input.replay_success_total + input.replay_failure_total;
    if input.replay_attempts_total != expected_attempts_total {
        replay_release_gate_record_failed_check(
            &mut failed_checks,
            &mut evidence_refs,
            "invalid_attempt_accounting",
            &[
                "metric:replay_attempts_total",
                "metric:replay_success_total",
                "metric:replay_failure_total",
            ],
        );
        indeterminate = true;
    }

    if input.replay_attempts_total == 0 {
        replay_release_gate_record_failed_check(
            &mut failed_checks,
            &mut evidence_refs,
            "missing_replay_attempts",
            &["metric:replay_attempts_total"],
        );
        indeterminate = true;
    }

    if !replay_release_gate_rate_valid(input.replay_hit_rate) {
        replay_release_gate_record_failed_check(
            &mut failed_checks,
            &mut evidence_refs,
            "invalid_replay_hit_rate",
            &["metric:replay_hit_rate"],
        );
        indeterminate = true;
    }
    if !replay_release_gate_rate_valid(input.false_replay_rate) {
        replay_release_gate_record_failed_check(
            &mut failed_checks,
            &mut evidence_refs,
            "invalid_false_replay_rate",
            &["metric:false_replay_rate"],
        );
        indeterminate = true;
    }

    if !input.replay_roi.is_finite() {
        replay_release_gate_record_failed_check(
            &mut failed_checks,
            &mut evidence_refs,
            "invalid_replay_roi",
            &["metric:replay_roi"],
        );
        indeterminate = true;
    }

    let expected_hit_rate = safe_ratio(input.replay_success_total, input.replay_attempts_total);
    let expected_false_rate = safe_ratio(input.replay_failure_total, input.replay_attempts_total);
    if input.replay_attempts_total > 0
        && !replay_release_gate_float_eq(input.replay_hit_rate, expected_hit_rate)
    {
        replay_release_gate_record_failed_check(
            &mut failed_checks,
            &mut evidence_refs,
            "invalid_replay_hit_rate_consistency",
            &["metric:replay_hit_rate", "metric:replay_success_total"],
        );
        indeterminate = true;
    }
    if input.replay_attempts_total > 0
        && !replay_release_gate_float_eq(input.false_replay_rate, expected_false_rate)
    {
        replay_release_gate_record_failed_check(
            &mut failed_checks,
            &mut evidence_refs,
            "invalid_false_replay_rate_consistency",
            &["metric:false_replay_rate", "metric:replay_failure_total"],
        );
        indeterminate = true;
    }

    if !(0.0..=1.0).contains(&input.thresholds.min_replay_hit_rate) {
        replay_release_gate_record_failed_check(
            &mut failed_checks,
            &mut evidence_refs,
            "invalid_threshold_min_replay_hit_rate",
            &["threshold:min_replay_hit_rate"],
        );
        indeterminate = true;
    }
    if !(0.0..=1.0).contains(&input.thresholds.max_false_replay_rate) {
        replay_release_gate_record_failed_check(
            &mut failed_checks,
            &mut evidence_refs,
            "invalid_threshold_max_false_replay_rate",
            &["threshold:max_false_replay_rate"],
        );
        indeterminate = true;
    }
    if !input.thresholds.min_replay_roi.is_finite() {
        replay_release_gate_record_failed_check(
            &mut failed_checks,
            &mut evidence_refs,
            "invalid_threshold_min_replay_roi",
            &["threshold:min_replay_roi"],
        );
        indeterminate = true;
    }

    if input.replay_attempts_total < input.thresholds.min_replay_attempts {
        replay_release_gate_record_failed_check(
            &mut failed_checks,
            &mut evidence_refs,
            "min_replay_attempts_below_threshold",
            &[
                "threshold:min_replay_attempts",
                "metric:replay_attempts_total",
            ],
        );
    }
    if input.replay_attempts_total > 0
        && input.replay_hit_rate < input.thresholds.min_replay_hit_rate
    {
        replay_release_gate_record_failed_check(
            &mut failed_checks,
            &mut evidence_refs,
            "replay_hit_rate_below_threshold",
            &["threshold:min_replay_hit_rate", "metric:replay_hit_rate"],
        );
    }
    if input.replay_attempts_total > 0
        && input.false_replay_rate > input.thresholds.max_false_replay_rate
    {
        replay_release_gate_record_failed_check(
            &mut failed_checks,
            &mut evidence_refs,
            "false_replay_rate_above_threshold",
            &[
                "threshold:max_false_replay_rate",
                "metric:false_replay_rate",
            ],
        );
    }
    if input.reasoning_avoided_tokens < input.thresholds.min_reasoning_avoided_tokens {
        replay_release_gate_record_failed_check(
            &mut failed_checks,
            &mut evidence_refs,
            "reasoning_avoided_tokens_below_threshold",
            &[
                "threshold:min_reasoning_avoided_tokens",
                "metric:reasoning_avoided_tokens",
            ],
        );
    }
    if input.replay_roi < input.thresholds.min_replay_roi {
        replay_release_gate_record_failed_check(
            &mut failed_checks,
            &mut evidence_refs,
            "replay_roi_below_threshold",
            &["threshold:min_replay_roi", "metric:replay_roi"],
        );
    }
    if input.thresholds.require_replay_safety && !input.replay_safety {
        replay_release_gate_record_failed_check(
            &mut failed_checks,
            &mut evidence_refs,
            "replay_safety_required",
            &["metric:replay_safety", "threshold:require_replay_safety"],
        );
    }

    failed_checks.sort();
    evidence_refs.sort();

    let status = if failed_checks.is_empty() {
        ReplayRoiReleaseGateStatus::Pass
    } else if indeterminate {
        ReplayRoiReleaseGateStatus::Indeterminate
    } else {
        ReplayRoiReleaseGateStatus::FailClosed
    };
    let joined_checks = if failed_checks.is_empty() {
        "none".to_string()
    } else {
        failed_checks.join(",")
    };
    let summary = match status {
        ReplayRoiReleaseGateStatus::Pass => format!(
            "release gate pass: attempts={} hit_rate={:.3} false_replay_rate={:.3} reasoning_avoided_tokens={} replay_roi={:.3} replay_safety={}",
            input.replay_attempts_total,
            input.replay_hit_rate,
            input.false_replay_rate,
            input.reasoning_avoided_tokens,
            input.replay_roi,
            input.replay_safety
        ),
        ReplayRoiReleaseGateStatus::FailClosed => format!(
            "release gate fail_closed: failed_checks=[{}] attempts={} hit_rate={:.3} false_replay_rate={:.3} reasoning_avoided_tokens={} replay_roi={:.3} replay_safety={}",
            joined_checks,
            input.replay_attempts_total,
            input.replay_hit_rate,
            input.false_replay_rate,
            input.reasoning_avoided_tokens,
            input.replay_roi,
            input.replay_safety
        ),
        ReplayRoiReleaseGateStatus::Indeterminate => format!(
            "release gate indeterminate (fail-closed): failed_checks=[{}] attempts={} hit_rate={:.3} false_replay_rate={:.3} reasoning_avoided_tokens={} replay_roi={:.3} replay_safety={}",
            joined_checks,
            input.replay_attempts_total,
            input.replay_hit_rate,
            input.false_replay_rate,
            input.reasoning_avoided_tokens,
            input.replay_roi,
            input.replay_safety
        ),
    };

    ReplayRoiReleaseGateOutputContract {
        status,
        failed_checks,
        evidence_refs,
        summary,
    }
}

fn replay_release_gate_record_failed_check(
    failed_checks: &mut Vec<String>,
    evidence_refs: &mut Vec<String>,
    check: &str,
    refs: &[&str],
) {
    replay_release_gate_push_unique(failed_checks, check.to_string());
    for entry in refs {
        replay_release_gate_push_unique(evidence_refs, (*entry).to_string());
    }
}

fn replay_release_gate_push_unique(values: &mut Vec<String>, entry: impl Into<String>) {
    let entry = entry.into();
    if !values.iter().any(|current| current == &entry) {
        values.push(entry);
    }
}

fn replay_release_gate_rate_valid(value: f64) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

fn replay_release_gate_float_eq(left: f64, right: f64) -> bool {
    (left - right).abs() <= 1e-9
}

fn evolution_health_snapshot(snapshot: &EvolutionMetricsSnapshot) -> EvolutionHealthSnapshot {
    EvolutionHealthSnapshot {
        status: "ok".into(),
        last_event_seq: snapshot.last_event_seq,
        promoted_genes: snapshot.promoted_genes,
        promoted_capsules: snapshot.promoted_capsules,
    }
}

fn render_evolution_metrics_prometheus(
    snapshot: &EvolutionMetricsSnapshot,
    health: &EvolutionHealthSnapshot,
) -> String {
    let mut out = String::new();
    out.push_str(
        "# HELP oris_evolution_replay_attempts_total Total replay attempts that reached validation.\n",
    );
    out.push_str("# TYPE oris_evolution_replay_attempts_total counter\n");
    out.push_str(&format!(
        "oris_evolution_replay_attempts_total {}\n",
        snapshot.replay_attempts_total
    ));
    out.push_str("# HELP oris_evolution_replay_success_total Total replay attempts that reused a capsule successfully.\n");
    out.push_str("# TYPE oris_evolution_replay_success_total counter\n");
    out.push_str(&format!(
        "oris_evolution_replay_success_total {}\n",
        snapshot.replay_success_total
    ));
    out.push_str("# HELP oris_evolution_replay_reasoning_avoided_total Total planner steps avoided by successful replay.\n");
    out.push_str("# TYPE oris_evolution_replay_reasoning_avoided_total counter\n");
    out.push_str(&format!(
        "oris_evolution_replay_reasoning_avoided_total {}\n",
        snapshot.replay_reasoning_avoided_total
    ));
    out.push_str("# HELP oris_evolution_reasoning_avoided_tokens_total Estimated reasoning tokens avoided by replay hits.\n");
    out.push_str("# TYPE oris_evolution_reasoning_avoided_tokens_total counter\n");
    out.push_str(&format!(
        "oris_evolution_reasoning_avoided_tokens_total {}\n",
        snapshot.reasoning_avoided_tokens_total
    ));
    out.push_str("# HELP oris_evolution_replay_fallback_cost_total Estimated reasoning token cost spent on replay fallbacks.\n");
    out.push_str("# TYPE oris_evolution_replay_fallback_cost_total counter\n");
    out.push_str(&format!(
        "oris_evolution_replay_fallback_cost_total {}\n",
        snapshot.replay_fallback_cost_total
    ));
    out.push_str("# HELP oris_evolution_replay_roi Net replay ROI in token space ((avoided - fallback_cost) / total).\n");
    out.push_str("# TYPE oris_evolution_replay_roi gauge\n");
    out.push_str(&format!(
        "oris_evolution_replay_roi {:.6}\n",
        snapshot.replay_roi
    ));
    out.push_str("# HELP oris_evolution_replay_utilization_by_task_class_total Successful replay reuse counts grouped by deterministic task class.\n");
    out.push_str("# TYPE oris_evolution_replay_utilization_by_task_class_total counter\n");
    for task_class in &snapshot.replay_task_classes {
        out.push_str(&format!(
            "oris_evolution_replay_utilization_by_task_class_total{{task_class_id=\"{}\",task_label=\"{}\"}} {}\n",
            prometheus_label_value(&task_class.task_class_id),
            prometheus_label_value(&task_class.task_label),
            task_class.replay_success_total
        ));
    }
    out.push_str("# HELP oris_evolution_replay_reasoning_avoided_by_task_class_total Planner steps avoided by successful replay grouped by deterministic task class.\n");
    out.push_str("# TYPE oris_evolution_replay_reasoning_avoided_by_task_class_total counter\n");
    for task_class in &snapshot.replay_task_classes {
        out.push_str(&format!(
            "oris_evolution_replay_reasoning_avoided_by_task_class_total{{task_class_id=\"{}\",task_label=\"{}\"}} {}\n",
            prometheus_label_value(&task_class.task_class_id),
            prometheus_label_value(&task_class.task_label),
            task_class.reasoning_steps_avoided_total
        ));
    }
    out.push_str("# HELP oris_evolution_reasoning_avoided_tokens_by_task_class_total Estimated reasoning tokens avoided by replay hits grouped by deterministic task class.\n");
    out.push_str("# TYPE oris_evolution_reasoning_avoided_tokens_by_task_class_total counter\n");
    for task_class in &snapshot.replay_task_classes {
        out.push_str(&format!(
            "oris_evolution_reasoning_avoided_tokens_by_task_class_total{{task_class_id=\"{}\",task_label=\"{}\"}} {}\n",
            prometheus_label_value(&task_class.task_class_id),
            prometheus_label_value(&task_class.task_label),
            task_class.reasoning_avoided_tokens_total
        ));
    }
    out.push_str("# HELP oris_evolution_replay_fallback_cost_by_task_class_total Estimated fallback token cost grouped by deterministic task class.\n");
    out.push_str("# TYPE oris_evolution_replay_fallback_cost_by_task_class_total counter\n");
    for task_class in &snapshot.replay_task_classes {
        out.push_str(&format!(
            "oris_evolution_replay_fallback_cost_by_task_class_total{{task_class_id=\"{}\",task_label=\"{}\"}} {}\n",
            prometheus_label_value(&task_class.task_class_id),
            prometheus_label_value(&task_class.task_label),
            task_class.replay_fallback_cost_total
        ));
    }
    out.push_str("# HELP oris_evolution_replay_roi_by_task_class Replay ROI in token space grouped by deterministic task class.\n");
    out.push_str("# TYPE oris_evolution_replay_roi_by_task_class gauge\n");
    for task_class in &snapshot.replay_task_classes {
        out.push_str(&format!(
            "oris_evolution_replay_roi_by_task_class{{task_class_id=\"{}\",task_label=\"{}\"}} {:.6}\n",
            prometheus_label_value(&task_class.task_class_id),
            prometheus_label_value(&task_class.task_label),
            task_class.replay_roi
        ));
    }
    out.push_str("# HELP oris_evolution_replay_roi_by_source Replay ROI in token space grouped by remote sender id for cross-node reconciliation.\n");
    out.push_str("# TYPE oris_evolution_replay_roi_by_source gauge\n");
    for source in &snapshot.replay_sources {
        out.push_str(&format!(
            "oris_evolution_replay_roi_by_source{{source_sender_id=\"{}\"}} {:.6}\n",
            prometheus_label_value(&source.source_sender_id),
            source.replay_roi
        ));
    }
    out.push_str("# HELP oris_evolution_reasoning_avoided_tokens_by_source_total Estimated reasoning tokens avoided grouped by remote sender id.\n");
    out.push_str("# TYPE oris_evolution_reasoning_avoided_tokens_by_source_total counter\n");
    for source in &snapshot.replay_sources {
        out.push_str(&format!(
            "oris_evolution_reasoning_avoided_tokens_by_source_total{{source_sender_id=\"{}\"}} {}\n",
            prometheus_label_value(&source.source_sender_id),
            source.reasoning_avoided_tokens_total
        ));
    }
    out.push_str("# HELP oris_evolution_replay_fallback_cost_by_source_total Estimated replay fallback token cost grouped by remote sender id.\n");
    out.push_str("# TYPE oris_evolution_replay_fallback_cost_by_source_total counter\n");
    for source in &snapshot.replay_sources {
        out.push_str(&format!(
            "oris_evolution_replay_fallback_cost_by_source_total{{source_sender_id=\"{}\"}} {}\n",
            prometheus_label_value(&source.source_sender_id),
            source.replay_fallback_cost_total
        ));
    }
    out.push_str("# HELP oris_evolution_replay_success_rate Successful replay attempts divided by replay attempts that reached validation.\n");
    out.push_str("# TYPE oris_evolution_replay_success_rate gauge\n");
    out.push_str(&format!(
        "oris_evolution_replay_success_rate {:.6}\n",
        snapshot.replay_success_rate
    ));
    out.push_str("# HELP oris_evolution_confidence_revalidations_total Total confidence-driven demotions that require revalidation before replay.\n");
    out.push_str("# TYPE oris_evolution_confidence_revalidations_total counter\n");
    out.push_str(&format!(
        "oris_evolution_confidence_revalidations_total {}\n",
        snapshot.confidence_revalidations_total
    ));
    out.push_str(
        "# HELP oris_evolution_mutation_declared_total Total declared mutations recorded in the evolution log.\n",
    );
    out.push_str("# TYPE oris_evolution_mutation_declared_total counter\n");
    out.push_str(&format!(
        "oris_evolution_mutation_declared_total {}\n",
        snapshot.mutation_declared_total
    ));
    out.push_str("# HELP oris_evolution_promoted_mutations_total Total mutations promoted by the governor.\n");
    out.push_str("# TYPE oris_evolution_promoted_mutations_total counter\n");
    out.push_str(&format!(
        "oris_evolution_promoted_mutations_total {}\n",
        snapshot.promoted_mutations_total
    ));
    out.push_str(
        "# HELP oris_evolution_promotion_ratio Promoted mutations divided by declared mutations.\n",
    );
    out.push_str("# TYPE oris_evolution_promotion_ratio gauge\n");
    out.push_str(&format!(
        "oris_evolution_promotion_ratio {:.6}\n",
        snapshot.promotion_ratio
    ));
    out.push_str("# HELP oris_evolution_gene_revocations_total Total gene revocations recorded in the evolution log.\n");
    out.push_str("# TYPE oris_evolution_gene_revocations_total counter\n");
    out.push_str(&format!(
        "oris_evolution_gene_revocations_total {}\n",
        snapshot.gene_revocations_total
    ));
    out.push_str("# HELP oris_evolution_mutation_velocity_last_hour Declared mutations observed in the last hour.\n");
    out.push_str("# TYPE oris_evolution_mutation_velocity_last_hour gauge\n");
    out.push_str(&format!(
        "oris_evolution_mutation_velocity_last_hour {}\n",
        snapshot.mutation_velocity_last_hour
    ));
    out.push_str("# HELP oris_evolution_revoke_frequency_last_hour Gene revocations observed in the last hour.\n");
    out.push_str("# TYPE oris_evolution_revoke_frequency_last_hour gauge\n");
    out.push_str(&format!(
        "oris_evolution_revoke_frequency_last_hour {}\n",
        snapshot.revoke_frequency_last_hour
    ));
    out.push_str("# HELP oris_evolution_promoted_genes Current promoted genes in the evolution projection.\n");
    out.push_str("# TYPE oris_evolution_promoted_genes gauge\n");
    out.push_str(&format!(
        "oris_evolution_promoted_genes {}\n",
        snapshot.promoted_genes
    ));
    out.push_str("# HELP oris_evolution_promoted_capsules Current promoted capsules in the evolution projection.\n");
    out.push_str("# TYPE oris_evolution_promoted_capsules gauge\n");
    out.push_str(&format!(
        "oris_evolution_promoted_capsules {}\n",
        snapshot.promoted_capsules
    ));
    out.push_str("# HELP oris_evolution_store_last_event_seq Last visible append-only evolution event sequence.\n");
    out.push_str("# TYPE oris_evolution_store_last_event_seq gauge\n");
    out.push_str(&format!(
        "oris_evolution_store_last_event_seq {}\n",
        snapshot.last_event_seq
    ));
    out.push_str(
        "# HELP oris_evolution_health Evolution observability store health (1 = healthy).\n",
    );
    out.push_str("# TYPE oris_evolution_health gauge\n");
    out.push_str(&format!(
        "oris_evolution_health {}\n",
        u8::from(health.status == "ok")
    ));
    out
}

fn count_recent_events(
    events: &[StoredEvolutionEvent],
    cutoff: DateTime<Utc>,
    predicate: impl Fn(&EvolutionEvent) -> bool,
) -> u64 {
    events
        .iter()
        .filter(|stored| {
            predicate(&stored.event)
                && parse_event_timestamp(&stored.timestamp)
                    .map(|timestamp| timestamp >= cutoff)
                    .unwrap_or(false)
        })
        .count() as u64
}

fn prometheus_label_value(input: &str) -> String {
    input
        .replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('"', "\\\"")
}

fn parse_event_timestamp(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|parsed| parsed.with_timezone(&Utc))
}

fn is_replay_validation_failure(event: &EvolutionEvent) -> bool {
    matches!(
        event,
        EvolutionEvent::ValidationFailed {
            gene_id: Some(_),
            ..
        }
    )
}

fn is_confidence_revalidation_event(event: &EvolutionEvent) -> bool {
    matches!(
        event,
        EvolutionEvent::PromotionEvaluated {
            state,
            reason,
            reason_code,
            ..
        }
            if *state == AssetState::Quarantined
                && (reason_code == &TransitionReasonCode::RevalidationConfidenceDecay
                    || (reason_code == &TransitionReasonCode::Unspecified
                        && reason.contains("confidence decayed")))
    )
}

fn safe_ratio(numerator: u64, denominator: u64) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn store_err(err: EvolutionError) -> EvoKernelError {
    EvoKernelError::Store(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use oris_agent_contract::{
        AgentRole, CoordinationPlan, CoordinationPrimitive, CoordinationTask,
    };
    use oris_kernel::{
        AllowAllPolicy, InMemoryEventStore, KernelMode, KernelState, NoopActionExecutor,
        NoopStepFn, StateUpdatedOnlyReducer,
    };
    use serde::{Deserialize, Serialize};

    #[derive(Clone, Debug, Default, Serialize, Deserialize)]
    struct TestState;

    impl KernelState for TestState {
        fn version(&self) -> u32 {
            1
        }
    }

    #[test]
    fn repair_quality_gate_accepts_semantic_variants() {
        let plan = r#"
根本原因：脚本中拼写错误导致 unknown command 'process'。
修复建议：将 `proccess` 更正为 `process`，并统一命令入口。
验证方式：执行 `cargo check -p oris-runtime` 与回归测试。
恢复方案：若新入口异常，立即回滚到旧命令映射。
"#;
        let report = evaluate_repair_quality_gate(plan);
        assert!(report.passes());
        assert!(report.failed_checks().is_empty());
    }

    #[test]
    fn repair_quality_gate_rejects_missing_incident_anchor() {
        let plan = r#"
原因分析：逻辑分支覆盖不足。
修复方案：补充分支与日志。
验证命令：cargo check -p oris-runtime
回滚方案：git revert HEAD
"#;
        let report = evaluate_repair_quality_gate(plan);
        assert!(!report.passes());
        assert!(report
            .failed_checks()
            .iter()
            .any(|check| check.contains("unknown command")));
    }

    fn temp_workspace(name: &str) -> std::path::PathBuf {
        let root =
            std::env::temp_dir().join(format!("oris-evokernel-{name}-{}", std::process::id()));
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::write(root.join("Cargo.lock"), "# lock\n").unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn demo() -> usize { 1 }\n").unwrap();
        root
    }

    fn test_kernel() -> Arc<Kernel<TestState>> {
        Arc::new(Kernel::<TestState> {
            events: Box::new(InMemoryEventStore::new()),
            snaps: None,
            reducer: Box::new(StateUpdatedOnlyReducer),
            exec: Box::new(NoopActionExecutor),
            step: Box::new(NoopStepFn),
            policy: Box::new(AllowAllPolicy),
            effect_sink: None,
            mode: KernelMode::Normal,
        })
    }

    fn lightweight_plan() -> ValidationPlan {
        ValidationPlan {
            profile: "test".into(),
            stages: vec![ValidationStage::Command {
                program: "git".into(),
                args: vec!["--version".into()],
                timeout_ms: 5_000,
            }],
        }
    }

    fn sample_mutation() -> PreparedMutation {
        prepare_mutation(
            MutationIntent {
                id: "mutation-1".into(),
                intent: "add README".into(),
                target: MutationTarget::Paths {
                    allow: vec!["README.md".into()],
                },
                expected_effect: "repo still builds".into(),
                risk: RiskLevel::Low,
                signals: vec!["missing readme".into()],
                spec_id: None,
            },
            "\
diff --git a/README.md b/README.md
new file mode 100644
index 0000000..1111111
--- /dev/null
+++ b/README.md
@@ -0,0 +1 @@
+# sample
"
            .into(),
            Some("HEAD".into()),
        )
    }

    fn base_sandbox_policy() -> SandboxPolicy {
        SandboxPolicy {
            allowed_programs: vec!["git".into()],
            max_duration_ms: 60_000,
            max_output_bytes: 1024 * 1024,
            denied_env_prefixes: Vec::new(),
        }
    }

    fn command_validator() -> Arc<dyn Validator> {
        Arc::new(CommandValidator::new(base_sandbox_policy()))
    }

    fn replay_input(signal: &str) -> SelectorInput {
        let rustc_version = std::process::Command::new("rustc")
            .arg("--version")
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
            .unwrap_or_else(|| "rustc unknown".into());
        SelectorInput {
            signals: vec![signal.into()],
            env: EnvFingerprint {
                rustc_version,
                cargo_lock_hash: compute_artifact_hash("# lock\n"),
                target_triple: format!(
                    "{}-unknown-{}",
                    std::env::consts::ARCH,
                    std::env::consts::OS
                ),
                os: std::env::consts::OS.into(),
            },
            spec_id: None,
            limit: 1,
        }
    }

    fn build_test_evo_with_store(
        name: &str,
        run_id: &str,
        validator: Arc<dyn Validator>,
        store: Arc<dyn EvolutionStore>,
    ) -> EvoKernel<TestState> {
        let workspace = temp_workspace(name);
        let sandbox: Arc<dyn Sandbox> = Arc::new(oris_sandbox::LocalProcessSandbox::new(
            run_id,
            &workspace,
            std::env::temp_dir(),
        ));
        EvoKernel::new(test_kernel(), sandbox, validator, store)
            .with_governor(Arc::new(DefaultGovernor::new(
                oris_governor::GovernorConfig {
                    promote_after_successes: 1,
                    ..Default::default()
                },
            )))
            .with_validation_plan(lightweight_plan())
            .with_sandbox_policy(base_sandbox_policy())
    }

    fn build_test_evo(
        name: &str,
        run_id: &str,
        validator: Arc<dyn Validator>,
    ) -> (EvoKernel<TestState>, Arc<dyn EvolutionStore>) {
        let store_root = std::env::temp_dir().join(format!(
            "oris-evokernel-{name}-store-{}",
            std::process::id()
        ));
        if store_root.exists() {
            fs::remove_dir_all(&store_root).unwrap();
        }
        let store: Arc<dyn EvolutionStore> =
            Arc::new(oris_evolution::JsonlEvolutionStore::new(&store_root));
        let evo = build_test_evo_with_store(name, run_id, validator, store.clone());
        (evo, store)
    }

    fn remote_publish_envelope(
        sender_id: &str,
        run_id: &str,
        gene_id: &str,
        capsule_id: &str,
        mutation_id: &str,
        signal: &str,
        file_name: &str,
        line: &str,
    ) -> EvolutionEnvelope {
        remote_publish_envelope_with_env(
            sender_id,
            run_id,
            gene_id,
            capsule_id,
            mutation_id,
            signal,
            file_name,
            line,
            replay_input(signal).env,
        )
    }

    fn remote_publish_envelope_with_env(
        sender_id: &str,
        run_id: &str,
        gene_id: &str,
        capsule_id: &str,
        mutation_id: &str,
        signal: &str,
        file_name: &str,
        line: &str,
        env: EnvFingerprint,
    ) -> EvolutionEnvelope {
        let mutation = prepare_mutation(
            MutationIntent {
                id: mutation_id.into(),
                intent: format!("add {file_name}"),
                target: MutationTarget::Paths {
                    allow: vec![file_name.into()],
                },
                expected_effect: "replay should still validate".into(),
                risk: RiskLevel::Low,
                signals: vec![signal.into()],
                spec_id: None,
            },
            format!(
                "\
diff --git a/{file_name} b/{file_name}
new file mode 100644
index 0000000..1111111
--- /dev/null
+++ b/{file_name}
@@ -0,0 +1 @@
+{line}
"
            ),
            Some("HEAD".into()),
        );
        let gene = Gene {
            id: gene_id.into(),
            signals: vec![signal.into()],
            strategy: vec![file_name.into()],
            validation: vec!["test".into()],
            state: AssetState::Promoted,
            task_class_id: None,
        };
        let capsule = Capsule {
            id: capsule_id.into(),
            gene_id: gene_id.into(),
            mutation_id: mutation_id.into(),
            run_id: run_id.into(),
            diff_hash: mutation.artifact.content_hash.clone(),
            confidence: 0.9,
            env,
            outcome: Outcome {
                success: true,
                validation_profile: "test".into(),
                validation_duration_ms: 1,
                changed_files: vec![file_name.into()],
                validator_hash: "validator-hash".into(),
                lines_changed: 1,
                replay_verified: false,
            },
            state: AssetState::Promoted,
        };
        EvolutionEnvelope::publish(
            sender_id,
            vec![
                NetworkAsset::EvolutionEvent {
                    event: EvolutionEvent::MutationDeclared { mutation },
                },
                NetworkAsset::Gene { gene: gene.clone() },
                NetworkAsset::Capsule {
                    capsule: capsule.clone(),
                },
                NetworkAsset::EvolutionEvent {
                    event: EvolutionEvent::CapsuleReleased {
                        capsule_id: capsule.id.clone(),
                        state: AssetState::Promoted,
                    },
                },
            ],
        )
    }

    fn remote_publish_envelope_with_signals(
        sender_id: &str,
        run_id: &str,
        gene_id: &str,
        capsule_id: &str,
        mutation_id: &str,
        mutation_signals: Vec<String>,
        gene_signals: Vec<String>,
        file_name: &str,
        line: &str,
        env: EnvFingerprint,
    ) -> EvolutionEnvelope {
        let mutation = prepare_mutation(
            MutationIntent {
                id: mutation_id.into(),
                intent: format!("add {file_name}"),
                target: MutationTarget::Paths {
                    allow: vec![file_name.into()],
                },
                expected_effect: "replay should still validate".into(),
                risk: RiskLevel::Low,
                signals: mutation_signals,
                spec_id: None,
            },
            format!(
                "\
diff --git a/{file_name} b/{file_name}
new file mode 100644
index 0000000..1111111
--- /dev/null
+++ b/{file_name}
@@ -0,0 +1 @@
+{line}
"
            ),
            Some("HEAD".into()),
        );
        let gene = Gene {
            id: gene_id.into(),
            signals: gene_signals,
            strategy: vec![file_name.into()],
            validation: vec!["test".into()],
            state: AssetState::Promoted,
            task_class_id: None,
        };
        let capsule = Capsule {
            id: capsule_id.into(),
            gene_id: gene_id.into(),
            mutation_id: mutation_id.into(),
            run_id: run_id.into(),
            diff_hash: mutation.artifact.content_hash.clone(),
            confidence: 0.9,
            env,
            outcome: Outcome {
                success: true,
                validation_profile: "test".into(),
                validation_duration_ms: 1,
                changed_files: vec![file_name.into()],
                validator_hash: "validator-hash".into(),
                lines_changed: 1,
                replay_verified: false,
            },
            state: AssetState::Promoted,
        };
        EvolutionEnvelope::publish(
            sender_id,
            vec![
                NetworkAsset::EvolutionEvent {
                    event: EvolutionEvent::MutationDeclared { mutation },
                },
                NetworkAsset::Gene { gene: gene.clone() },
                NetworkAsset::Capsule {
                    capsule: capsule.clone(),
                },
                NetworkAsset::EvolutionEvent {
                    event: EvolutionEvent::CapsuleReleased {
                        capsule_id: capsule.id.clone(),
                        state: AssetState::Promoted,
                    },
                },
            ],
        )
    }

    struct FixedValidator {
        success: bool,
    }

    #[async_trait]
    impl Validator for FixedValidator {
        async fn run(
            &self,
            _receipt: &SandboxReceipt,
            plan: &ValidationPlan,
        ) -> Result<ValidationReport, ValidationError> {
            Ok(ValidationReport {
                success: self.success,
                duration_ms: 1,
                stages: Vec::new(),
                logs: if self.success {
                    format!("{} ok", plan.profile)
                } else {
                    format!("{} failed", plan.profile)
                },
            })
        }
    }

    struct FailOnAppendStore {
        inner: JsonlEvolutionStore,
        fail_on_call: usize,
        call_count: Mutex<usize>,
    }

    impl FailOnAppendStore {
        fn new(root_dir: std::path::PathBuf, fail_on_call: usize) -> Self {
            Self {
                inner: JsonlEvolutionStore::new(root_dir),
                fail_on_call,
                call_count: Mutex::new(0),
            }
        }
    }

    impl EvolutionStore for FailOnAppendStore {
        fn append_event(&self, event: EvolutionEvent) -> Result<u64, EvolutionError> {
            let mut call_count = self
                .call_count
                .lock()
                .map_err(|_| EvolutionError::Io("test store lock poisoned".into()))?;
            *call_count += 1;
            if *call_count == self.fail_on_call {
                return Err(EvolutionError::Io("injected append failure".into()));
            }
            self.inner.append_event(event)
        }

        fn scan(&self, from_seq: u64) -> Result<Vec<StoredEvolutionEvent>, EvolutionError> {
            self.inner.scan(from_seq)
        }

        fn rebuild_projection(&self) -> Result<EvolutionProjection, EvolutionError> {
            self.inner.rebuild_projection()
        }
    }

    #[test]
    fn coordination_planner_to_coder_handoff_is_deterministic() {
        let result = MultiAgentCoordinator::new().coordinate(CoordinationPlan {
            root_goal: "ship feature".into(),
            primitive: CoordinationPrimitive::Sequential,
            tasks: vec![
                CoordinationTask {
                    id: "planner".into(),
                    role: AgentRole::Planner,
                    description: "split the work".into(),
                    depends_on: Vec::new(),
                },
                CoordinationTask {
                    id: "coder".into(),
                    role: AgentRole::Coder,
                    description: "implement the patch".into(),
                    depends_on: vec!["planner".into()],
                },
            ],
            timeout_ms: 5_000,
            max_retries: 0,
        });

        assert_eq!(result.completed_tasks, vec!["planner", "coder"]);
        assert!(result.failed_tasks.is_empty());
        assert!(result.messages.iter().any(|message| {
            message.from_role == AgentRole::Planner
                && message.to_role == AgentRole::Coder
                && message.task_id == "coder"
        }));
    }

    #[test]
    fn coordination_repair_runs_only_after_coder_failure() {
        let result = MultiAgentCoordinator::new().coordinate(CoordinationPlan {
            root_goal: "fix broken implementation".into(),
            primitive: CoordinationPrimitive::Sequential,
            tasks: vec![
                CoordinationTask {
                    id: "coder".into(),
                    role: AgentRole::Coder,
                    description: "force-fail initial implementation".into(),
                    depends_on: Vec::new(),
                },
                CoordinationTask {
                    id: "repair".into(),
                    role: AgentRole::Repair,
                    description: "patch the failed implementation".into(),
                    depends_on: vec!["coder".into()],
                },
            ],
            timeout_ms: 5_000,
            max_retries: 0,
        });

        assert_eq!(result.completed_tasks, vec!["repair"]);
        assert_eq!(result.failed_tasks, vec!["coder"]);
        assert!(result.messages.iter().any(|message| {
            message.from_role == AgentRole::Coder
                && message.to_role == AgentRole::Repair
                && message.task_id == "repair"
        }));
    }

    #[test]
    fn coordination_optimizer_runs_after_successful_implementation_step() {
        let result = MultiAgentCoordinator::new().coordinate(CoordinationPlan {
            root_goal: "ship optimized patch".into(),
            primitive: CoordinationPrimitive::Sequential,
            tasks: vec![
                CoordinationTask {
                    id: "coder".into(),
                    role: AgentRole::Coder,
                    description: "implement a working patch".into(),
                    depends_on: Vec::new(),
                },
                CoordinationTask {
                    id: "optimizer".into(),
                    role: AgentRole::Optimizer,
                    description: "tighten the implementation".into(),
                    depends_on: vec!["coder".into()],
                },
            ],
            timeout_ms: 5_000,
            max_retries: 0,
        });

        assert_eq!(result.completed_tasks, vec!["coder", "optimizer"]);
        assert!(result.failed_tasks.is_empty());
    }

    #[test]
    fn coordination_parallel_waves_preserve_sorted_merge_order() {
        let result = MultiAgentCoordinator::new().coordinate(CoordinationPlan {
            root_goal: "parallelize safe tasks".into(),
            primitive: CoordinationPrimitive::Parallel,
            tasks: vec![
                CoordinationTask {
                    id: "z-task".into(),
                    role: AgentRole::Planner,
                    description: "analyze z".into(),
                    depends_on: Vec::new(),
                },
                CoordinationTask {
                    id: "a-task".into(),
                    role: AgentRole::Coder,
                    description: "implement a".into(),
                    depends_on: Vec::new(),
                },
                CoordinationTask {
                    id: "mid-task".into(),
                    role: AgentRole::Optimizer,
                    description: "polish after both".into(),
                    depends_on: vec!["z-task".into(), "a-task".into()],
                },
            ],
            timeout_ms: 5_000,
            max_retries: 0,
        });

        assert_eq!(result.completed_tasks, vec!["a-task", "z-task", "mid-task"]);
        assert!(result.failed_tasks.is_empty());
    }

    #[test]
    fn coordination_retries_stop_at_max_retries() {
        let result = MultiAgentCoordinator::new().coordinate(CoordinationPlan {
            root_goal: "retry then stop".into(),
            primitive: CoordinationPrimitive::Sequential,
            tasks: vec![CoordinationTask {
                id: "coder".into(),
                role: AgentRole::Coder,
                description: "force-fail this task".into(),
                depends_on: Vec::new(),
            }],
            timeout_ms: 5_000,
            max_retries: 1,
        });

        assert!(result.completed_tasks.is_empty());
        assert_eq!(result.failed_tasks, vec!["coder"]);
        assert_eq!(
            result
                .messages
                .iter()
                .filter(|message| message.task_id == "coder" && message.content.contains("failed"))
                .count(),
            2
        );
    }

    #[test]
    fn coordination_conditional_mode_skips_downstream_tasks_on_failure() {
        let result = MultiAgentCoordinator::new().coordinate(CoordinationPlan {
            root_goal: "skip blocked follow-up work".into(),
            primitive: CoordinationPrimitive::Conditional,
            tasks: vec![
                CoordinationTask {
                    id: "coder".into(),
                    role: AgentRole::Coder,
                    description: "force-fail the implementation".into(),
                    depends_on: Vec::new(),
                },
                CoordinationTask {
                    id: "optimizer".into(),
                    role: AgentRole::Optimizer,
                    description: "only optimize a successful implementation".into(),
                    depends_on: vec!["coder".into()],
                },
            ],
            timeout_ms: 5_000,
            max_retries: 0,
        });

        assert!(result.completed_tasks.is_empty());
        assert_eq!(result.failed_tasks, vec!["coder"]);
        assert!(result.messages.iter().any(|message| {
            message.task_id == "optimizer"
                && message
                    .content
                    .contains("skipped due to failed dependency chain")
        }));
        assert!(!result
            .failed_tasks
            .iter()
            .any(|task_id| task_id == "optimizer"));
    }

    #[tokio::test]
    async fn command_validator_aggregates_stage_reports() {
        let workspace = temp_workspace("validator");
        let receipt = SandboxReceipt {
            mutation_id: "m".into(),
            workdir: workspace,
            applied: true,
            changed_files: Vec::new(),
            patch_hash: "hash".into(),
            stdout_log: std::env::temp_dir().join("stdout.log"),
            stderr_log: std::env::temp_dir().join("stderr.log"),
        };
        let validator = CommandValidator::new(SandboxPolicy {
            allowed_programs: vec!["git".into()],
            max_duration_ms: 1_000,
            max_output_bytes: 1024,
            denied_env_prefixes: Vec::new(),
        });
        let report = validator
            .run(
                &receipt,
                &ValidationPlan {
                    profile: "test".into(),
                    stages: vec![ValidationStage::Command {
                        program: "git".into(),
                        args: vec!["--version".into()],
                        timeout_ms: 1_000,
                    }],
                },
            )
            .await
            .unwrap();
        assert_eq!(report.stages.len(), 1);
    }

    #[tokio::test]
    async fn capture_successful_mutation_appends_capsule() {
        let (evo, store) = build_test_evo("capture", "run-1", command_validator());
        let capsule = evo
            .capture_successful_mutation(&"run-1".into(), sample_mutation())
            .await
            .unwrap();
        let events = store.scan(1).unwrap();
        assert!(events
            .iter()
            .any(|stored| matches!(stored.event, EvolutionEvent::CapsuleCommitted { .. })));
        assert!(!capsule.id.is_empty());
    }

    #[tokio::test]
    async fn replay_hit_records_capsule_reused() {
        let (evo, store) = build_test_evo("replay", "run-2", command_validator());
        let capsule = evo
            .capture_successful_mutation(&"run-2".into(), sample_mutation())
            .await
            .unwrap();
        let replay_run_id = "run-replay".to_string();
        let decision = evo
            .replay_or_fallback_for_run(&replay_run_id, replay_input("missing readme"))
            .await
            .unwrap();
        assert!(decision.used_capsule);
        assert_eq!(decision.capsule_id, Some(capsule.id));
        assert!(!decision.detect_evidence.task_class_id.is_empty());
        assert!(!decision.detect_evidence.matched_signals.is_empty());
        assert!(decision.detect_evidence.mismatch_reasons.is_empty());
        assert!(!decision.select_evidence.candidates.is_empty());
        assert!(!decision.select_evidence.exact_match_lookup);
        assert_eq!(
            decision.select_evidence.selected_capsule_id.as_deref(),
            decision.capsule_id.as_deref()
        );
        assert!(store.scan(1).unwrap().iter().any(|stored| matches!(
            &stored.event,
            EvolutionEvent::CapsuleReused {
                run_id,
                replay_run_id: Some(current_replay_run_id),
                ..
            } if run_id == "run-2" && current_replay_run_id == &replay_run_id
        )));
    }

    #[tokio::test]
    async fn legacy_replay_executor_api_preserves_original_capsule_run_id() {
        let capture_run_id = "run-legacy-capture".to_string();
        let (evo, store) = build_test_evo("replay-legacy", &capture_run_id, command_validator());
        let capsule = evo
            .capture_successful_mutation(&capture_run_id, sample_mutation())
            .await
            .unwrap();
        let executor = StoreReplayExecutor {
            sandbox: evo.sandbox.clone(),
            validator: evo.validator.clone(),
            store: evo.store.clone(),
            selector: evo.selector.clone(),
            governor: evo.governor.clone(),
            economics: Some(evo.economics.clone()),
            remote_publishers: Some(evo.remote_publishers.clone()),
            stake_policy: evo.stake_policy.clone(),
        };

        let decision = executor
            .try_replay(
                &replay_input("missing readme"),
                &evo.sandbox_policy,
                &evo.validation_plan,
            )
            .await
            .unwrap();

        assert!(decision.used_capsule);
        assert_eq!(decision.capsule_id, Some(capsule.id));
        assert!(store.scan(1).unwrap().iter().any(|stored| matches!(
            &stored.event,
            EvolutionEvent::CapsuleReused {
                run_id,
                replay_run_id: None,
                ..
            } if run_id == &capture_run_id
        )));
    }

    #[tokio::test]
    async fn metrics_snapshot_tracks_replay_promotion_and_revocation_signals() {
        let (evo, _) = build_test_evo("metrics", "run-metrics", command_validator());
        let capsule = evo
            .capture_successful_mutation(&"run-metrics".into(), sample_mutation())
            .await
            .unwrap();
        let decision = evo
            .replay_or_fallback(replay_input("missing readme"))
            .await
            .unwrap();
        assert!(decision.used_capsule);

        evo.revoke_assets(&RevokeNotice {
            sender_id: "node-metrics".into(),
            asset_ids: vec![capsule.id.clone()],
            reason: "manual test revoke".into(),
        })
        .unwrap();

        let snapshot = evo.metrics_snapshot().unwrap();
        assert_eq!(snapshot.replay_attempts_total, 1);
        assert_eq!(snapshot.replay_success_total, 1);
        assert_eq!(snapshot.replay_success_rate, 1.0);
        assert_eq!(snapshot.confidence_revalidations_total, 0);
        assert_eq!(snapshot.replay_reasoning_avoided_total, 1);
        assert_eq!(
            snapshot.reasoning_avoided_tokens_total,
            decision.economics_evidence.reasoning_avoided_tokens
        );
        assert_eq!(snapshot.replay_fallback_cost_total, 0);
        assert_eq!(snapshot.replay_roi, 1.0);
        assert_eq!(snapshot.replay_task_classes.len(), 1);
        assert_eq!(snapshot.replay_task_classes[0].replay_success_total, 1);
        assert_eq!(snapshot.replay_task_classes[0].replay_failure_total, 0);
        assert_eq!(
            snapshot.replay_task_classes[0].reasoning_steps_avoided_total,
            1
        );
        assert_eq!(
            snapshot.replay_task_classes[0].replay_fallback_cost_total,
            0
        );
        assert_eq!(snapshot.replay_task_classes[0].replay_roi, 1.0);
        assert!(snapshot.replay_sources.is_empty());
        assert_eq!(snapshot.confidence_revalidations_total, 0);
        assert_eq!(snapshot.mutation_declared_total, 1);
        assert_eq!(snapshot.promoted_mutations_total, 1);
        assert_eq!(snapshot.promotion_ratio, 1.0);
        assert_eq!(snapshot.gene_revocations_total, 1);
        assert_eq!(snapshot.mutation_velocity_last_hour, 1);
        assert_eq!(snapshot.revoke_frequency_last_hour, 1);
        assert_eq!(snapshot.promoted_genes, 0);
        assert_eq!(snapshot.promoted_capsules, 0);

        let rendered = evo.render_metrics_prometheus().unwrap();
        assert!(rendered.contains("oris_evolution_replay_reasoning_avoided_total 1"));
        assert!(rendered.contains("oris_evolution_reasoning_avoided_tokens_total"));
        assert!(rendered.contains("oris_evolution_replay_fallback_cost_total"));
        assert!(rendered.contains("oris_evolution_replay_roi 1.000000"));
        assert!(rendered.contains("oris_evolution_replay_utilization_by_task_class_total"));
        assert!(rendered.contains("oris_evolution_replay_reasoning_avoided_by_task_class_total"));
        assert!(rendered.contains("oris_evolution_replay_success_rate 1.000000"));
        assert!(rendered.contains("oris_evolution_confidence_revalidations_total 0"));
        assert!(rendered.contains("oris_evolution_promotion_ratio 1.000000"));
        assert!(rendered.contains("oris_evolution_revoke_frequency_last_hour 1"));
        assert!(rendered.contains("oris_evolution_mutation_velocity_last_hour 1"));
        assert!(rendered.contains("oris_evolution_health 1"));
    }

    #[tokio::test]
    async fn replay_roi_release_gate_summary_matches_metrics_snapshot_for_legacy_replay_history() {
        let (evo, _) = build_test_evo("roi-legacy", "run-roi-legacy", command_validator());
        let capsule = evo
            .capture_successful_mutation(&"run-roi-legacy".into(), sample_mutation())
            .await
            .unwrap();

        evo.store
            .append_event(EvolutionEvent::CapsuleReused {
                capsule_id: capsule.id.clone(),
                gene_id: capsule.gene_id.clone(),
                run_id: capsule.run_id.clone(),
                replay_run_id: Some("run-roi-legacy-replay".into()),
            })
            .unwrap();
        evo.store
            .append_event(EvolutionEvent::ValidationFailed {
                mutation_id: "legacy-replay-failure".into(),
                report: ValidationSnapshot {
                    success: false,
                    profile: "test".into(),
                    duration_ms: 1,
                    summary: "legacy replay validation failed".into(),
                },
                gene_id: Some(capsule.gene_id.clone()),
            })
            .unwrap();

        let metrics = evo.metrics_snapshot().unwrap();
        let summary = evo.replay_roi_release_gate_summary(0).unwrap();
        let task_class = &metrics.replay_task_classes[0];

        assert_eq!(metrics.replay_attempts_total, 2);
        assert_eq!(metrics.replay_success_total, 1);
        assert_eq!(summary.replay_attempts_total, metrics.replay_attempts_total);
        assert_eq!(summary.replay_success_total, metrics.replay_success_total);
        assert_eq!(
            summary.replay_failure_total,
            metrics.replay_attempts_total - metrics.replay_success_total
        );
        assert_eq!(
            summary.reasoning_avoided_tokens_total,
            metrics.reasoning_avoided_tokens_total
        );
        assert_eq!(
            summary.replay_fallback_cost_total,
            metrics.replay_fallback_cost_total
        );
        assert_eq!(summary.replay_roi, metrics.replay_roi);
        assert_eq!(summary.replay_task_classes.len(), 1);
        assert_eq!(
            summary.replay_task_classes[0].task_class_id,
            task_class.task_class_id
        );
        assert_eq!(
            summary.replay_task_classes[0].replay_success_total,
            task_class.replay_success_total
        );
        assert_eq!(
            summary.replay_task_classes[0].replay_failure_total,
            task_class.replay_failure_total
        );
        assert_eq!(
            summary.replay_task_classes[0].reasoning_avoided_tokens_total,
            task_class.reasoning_avoided_tokens_total
        );
        assert_eq!(
            summary.replay_task_classes[0].replay_fallback_cost_total,
            task_class.replay_fallback_cost_total
        );
    }

    #[tokio::test]
    async fn replay_roi_release_gate_summary_aggregates_task_class_and_remote_source() {
        let (evo, _) = build_test_evo("roi-summary", "run-roi-summary", command_validator());
        let envelope = remote_publish_envelope(
            "node-roi",
            "run-remote-roi",
            "gene-roi",
            "capsule-roi",
            "mutation-roi",
            "roi-signal",
            "ROI.md",
            "# roi",
        );
        evo.import_remote_envelope(&envelope).unwrap();

        let miss = evo
            .replay_or_fallback(replay_input("entropy-hash-12345-no-overlap"))
            .await
            .unwrap();
        assert!(!miss.used_capsule);
        assert!(miss.fallback_to_planner);
        assert!(miss.select_evidence.candidates.is_empty());
        assert!(miss
            .detect_evidence
            .mismatch_reasons
            .iter()
            .any(|reason| reason == "no_candidate_after_select"));

        let hit = evo
            .replay_or_fallback(replay_input("roi-signal"))
            .await
            .unwrap();
        assert!(hit.used_capsule);
        assert!(!hit.select_evidence.candidates.is_empty());
        assert_eq!(
            hit.select_evidence.selected_capsule_id.as_deref(),
            hit.capsule_id.as_deref()
        );

        let summary = evo.replay_roi_release_gate_summary(60 * 60).unwrap();
        assert_eq!(summary.replay_attempts_total, 2);
        assert_eq!(summary.replay_success_total, 1);
        assert_eq!(summary.replay_failure_total, 1);
        assert!(summary.reasoning_avoided_tokens_total > 0);
        assert!(summary.replay_fallback_cost_total > 0);
        assert!(summary
            .replay_task_classes
            .iter()
            .any(|entry| { entry.replay_success_total == 1 && entry.replay_failure_total == 0 }));
        assert!(summary.replay_sources.iter().any(|source| {
            source.source_sender_id == "node-roi" && source.replay_success_total == 1
        }));

        let rendered = evo
            .render_replay_roi_release_gate_summary_json(60 * 60)
            .unwrap();
        assert!(rendered.contains("\"replay_attempts_total\": 2"));
        assert!(rendered.contains("\"source_sender_id\": \"node-roi\""));
    }

    #[tokio::test]
    async fn replay_roi_release_gate_summary_contract_exposes_core_metrics_and_fail_closed_defaults(
    ) {
        let (evo, _) = build_test_evo("roi-contract", "run-roi-contract", command_validator());
        let envelope = remote_publish_envelope(
            "node-contract",
            "run-remote-contract",
            "gene-contract",
            "capsule-contract",
            "mutation-contract",
            "contract-signal",
            "CONTRACT.md",
            "# contract",
        );
        evo.import_remote_envelope(&envelope).unwrap();

        let miss = evo
            .replay_or_fallback(replay_input("entropy-hash-contract-no-overlap"))
            .await
            .unwrap();
        assert!(!miss.used_capsule);
        assert!(miss.fallback_to_planner);

        let hit = evo
            .replay_or_fallback(replay_input("contract-signal"))
            .await
            .unwrap();
        assert!(hit.used_capsule);

        let summary = evo.replay_roi_release_gate_summary(60 * 60).unwrap();
        let contract = evo
            .replay_roi_release_gate_contract(60 * 60, ReplayRoiReleaseGateThresholds::default())
            .unwrap();

        assert_eq!(contract.input.replay_attempts_total, 2);
        assert_eq!(contract.input.replay_success_total, 1);
        assert_eq!(contract.input.replay_failure_total, 1);
        assert_eq!(
            contract.input.reasoning_avoided_tokens,
            summary.reasoning_avoided_tokens_total
        );
        assert_eq!(
            contract.input.replay_fallback_cost_total,
            summary.replay_fallback_cost_total
        );
        assert!((contract.input.replay_hit_rate - 0.5).abs() < f64::EPSILON);
        assert!((contract.input.false_replay_rate - 0.5).abs() < f64::EPSILON);
        assert!((contract.input.replay_roi - summary.replay_roi).abs() < f64::EPSILON);
        assert!(contract.input.replay_safety);
        assert_eq!(
            contract.input.aggregation_dimensions,
            REPLAY_RELEASE_GATE_AGGREGATION_DIMENSIONS
                .iter()
                .map(|dimension| (*dimension).to_string())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            contract.input.thresholds,
            ReplayRoiReleaseGateThresholds::default()
        );
        assert_eq!(
            contract.input.fail_closed_policy,
            ReplayRoiReleaseGateFailClosedPolicy::default()
        );
        assert_eq!(
            contract.output.status,
            ReplayRoiReleaseGateStatus::FailClosed
        );
        assert!(contract
            .output
            .failed_checks
            .iter()
            .any(|check| check == "min_replay_attempts_below_threshold"));
        assert!(contract
            .output
            .failed_checks
            .iter()
            .any(|check| check == "replay_hit_rate_below_threshold"));
        assert!(contract
            .output
            .failed_checks
            .iter()
            .any(|check| check == "false_replay_rate_above_threshold"));
        assert!(contract
            .output
            .evidence_refs
            .iter()
            .any(|evidence| evidence == "replay_roi_release_gate_summary"));
        assert!(contract.output.summary.contains("release gate fail_closed"));
    }

    #[tokio::test]
    async fn replay_roi_release_gate_summary_contract_accepts_custom_thresholds_and_json() {
        let (evo, _) = build_test_evo(
            "roi-contract-thresholds",
            "run-roi-contract-thresholds",
            command_validator(),
        );
        let thresholds = ReplayRoiReleaseGateThresholds {
            min_replay_attempts: 8,
            min_replay_hit_rate: 0.75,
            max_false_replay_rate: 0.10,
            min_reasoning_avoided_tokens: 600,
            min_replay_roi: 0.30,
            require_replay_safety: true,
        };
        let contract = evo
            .replay_roi_release_gate_contract(60 * 60, thresholds.clone())
            .unwrap();
        assert_eq!(contract.input.thresholds, thresholds.clone());
        assert_eq!(contract.input.replay_attempts_total, 0);
        assert_eq!(contract.input.replay_hit_rate, 0.0);
        assert_eq!(contract.input.false_replay_rate, 0.0);
        assert!(!contract.input.replay_safety_signal.has_replay_activity);
        assert!(!contract.input.replay_safety);
        assert_eq!(
            contract.output.status,
            ReplayRoiReleaseGateStatus::Indeterminate
        );
        assert!(contract
            .output
            .failed_checks
            .iter()
            .any(|check| check == "missing_replay_attempts"));
        assert!(contract
            .output
            .summary
            .contains("indeterminate (fail-closed)"));

        let rendered = evo
            .render_replay_roi_release_gate_contract_json(60 * 60, thresholds)
            .unwrap();
        assert!(rendered.contains("\"min_replay_attempts\": 8"));
        assert!(rendered.contains("\"min_replay_hit_rate\": 0.75"));
        assert!(rendered.contains("\"status\": \"indeterminate\""));
    }

    #[tokio::test]
    async fn replay_roi_release_gate_summary_window_boundary_filters_old_events() {
        let (evo, _) = build_test_evo("roi-window", "run-roi-window", command_validator());
        let envelope = remote_publish_envelope(
            "node-window",
            "run-remote-window",
            "gene-window",
            "capsule-window",
            "mutation-window",
            "window-signal",
            "WINDOW.md",
            "# window",
        );
        evo.import_remote_envelope(&envelope).unwrap();

        let miss = evo
            .replay_or_fallback(replay_input("window-no-match-signal"))
            .await
            .unwrap();
        assert!(!miss.used_capsule);
        assert!(miss.fallback_to_planner);

        let first_hit = evo
            .replay_or_fallback(replay_input("window-signal"))
            .await
            .unwrap();
        assert!(first_hit.used_capsule);

        std::thread::sleep(std::time::Duration::from_secs(2));

        let second_hit = evo
            .replay_or_fallback(replay_input("window-signal"))
            .await
            .unwrap();
        assert!(second_hit.used_capsule);

        let narrow = evo.replay_roi_release_gate_summary(1).unwrap();
        assert_eq!(narrow.replay_attempts_total, 1);
        assert_eq!(narrow.replay_success_total, 1);
        assert_eq!(narrow.replay_failure_total, 0);

        let all = evo.replay_roi_release_gate_summary(0).unwrap();
        assert_eq!(all.replay_attempts_total, 3);
        assert_eq!(all.replay_success_total, 2);
        assert_eq!(all.replay_failure_total, 1);
    }

    fn fixed_release_gate_pass_fixture() -> ReplayRoiReleaseGateInputContract {
        ReplayRoiReleaseGateInputContract {
            generated_at: "2026-03-13T00:00:00Z".to_string(),
            window_seconds: 86_400,
            aggregation_dimensions: REPLAY_RELEASE_GATE_AGGREGATION_DIMENSIONS
                .iter()
                .map(|dimension| (*dimension).to_string())
                .collect(),
            replay_attempts_total: 4,
            replay_success_total: 3,
            replay_failure_total: 1,
            replay_hit_rate: 0.75,
            false_replay_rate: 0.25,
            reasoning_avoided_tokens: 480,
            replay_fallback_cost_total: 64,
            replay_roi: compute_replay_roi(480, 64),
            replay_safety: true,
            replay_safety_signal: ReplayRoiReleaseGateSafetySignal {
                fail_closed_default: true,
                rollback_ready: true,
                audit_trail_complete: true,
                has_replay_activity: true,
            },
            thresholds: ReplayRoiReleaseGateThresholds::default(),
            fail_closed_policy: ReplayRoiReleaseGateFailClosedPolicy::default(),
        }
    }

    fn fixed_release_gate_fail_fixture() -> ReplayRoiReleaseGateInputContract {
        ReplayRoiReleaseGateInputContract {
            generated_at: "2026-03-13T00:00:00Z".to_string(),
            window_seconds: 86_400,
            aggregation_dimensions: REPLAY_RELEASE_GATE_AGGREGATION_DIMENSIONS
                .iter()
                .map(|dimension| (*dimension).to_string())
                .collect(),
            replay_attempts_total: 10,
            replay_success_total: 4,
            replay_failure_total: 6,
            replay_hit_rate: 0.4,
            false_replay_rate: 0.6,
            reasoning_avoided_tokens: 80,
            replay_fallback_cost_total: 400,
            replay_roi: compute_replay_roi(80, 400),
            replay_safety: false,
            replay_safety_signal: ReplayRoiReleaseGateSafetySignal {
                fail_closed_default: true,
                rollback_ready: true,
                audit_trail_complete: true,
                has_replay_activity: true,
            },
            thresholds: ReplayRoiReleaseGateThresholds::default(),
            fail_closed_policy: ReplayRoiReleaseGateFailClosedPolicy::default(),
        }
    }

    fn fixed_release_gate_borderline_fixture() -> ReplayRoiReleaseGateInputContract {
        ReplayRoiReleaseGateInputContract {
            generated_at: "2026-03-13T00:00:00Z".to_string(),
            window_seconds: 3_600,
            aggregation_dimensions: REPLAY_RELEASE_GATE_AGGREGATION_DIMENSIONS
                .iter()
                .map(|dimension| (*dimension).to_string())
                .collect(),
            replay_attempts_total: 4,
            replay_success_total: 3,
            replay_failure_total: 1,
            replay_hit_rate: 0.75,
            false_replay_rate: 0.25,
            reasoning_avoided_tokens: 192,
            replay_fallback_cost_total: 173,
            replay_roi: 0.05,
            replay_safety: true,
            replay_safety_signal: ReplayRoiReleaseGateSafetySignal {
                fail_closed_default: true,
                rollback_ready: true,
                audit_trail_complete: true,
                has_replay_activity: true,
            },
            thresholds: ReplayRoiReleaseGateThresholds {
                min_replay_attempts: 4,
                min_replay_hit_rate: 0.75,
                max_false_replay_rate: 0.25,
                min_reasoning_avoided_tokens: 192,
                min_replay_roi: 0.05,
                require_replay_safety: true,
            },
            fail_closed_policy: ReplayRoiReleaseGateFailClosedPolicy::default(),
        }
    }

    #[test]
    fn replay_roi_release_gate_summary_fixed_fixtures_cover_pass_fail_and_borderline() {
        let pass =
            evaluate_replay_roi_release_gate_contract_input(&fixed_release_gate_pass_fixture());
        let fail =
            evaluate_replay_roi_release_gate_contract_input(&fixed_release_gate_fail_fixture());
        let borderline = evaluate_replay_roi_release_gate_contract_input(
            &fixed_release_gate_borderline_fixture(),
        );

        assert_eq!(pass.status, ReplayRoiReleaseGateStatus::Pass);
        assert!(pass.failed_checks.is_empty());
        assert_eq!(fail.status, ReplayRoiReleaseGateStatus::FailClosed);
        assert!(!fail.failed_checks.is_empty());
        assert_eq!(borderline.status, ReplayRoiReleaseGateStatus::Pass);
        assert!(borderline.failed_checks.is_empty());
    }

    #[test]
    fn replay_roi_release_gate_summary_machine_readable_output_is_stable_and_sorted() {
        let output =
            evaluate_replay_roi_release_gate_contract_input(&fixed_release_gate_fail_fixture());

        assert_eq!(
            output.failed_checks,
            vec![
                "false_replay_rate_above_threshold".to_string(),
                "reasoning_avoided_tokens_below_threshold".to_string(),
                "replay_hit_rate_below_threshold".to_string(),
                "replay_roi_below_threshold".to_string(),
                "replay_safety_required".to_string(),
            ]
        );
        assert_eq!(
            output.evidence_refs,
            vec![
                "generated_at:2026-03-13T00:00:00Z".to_string(),
                "metric:false_replay_rate".to_string(),
                "metric:reasoning_avoided_tokens".to_string(),
                "metric:replay_hit_rate".to_string(),
                "metric:replay_roi".to_string(),
                "metric:replay_safety".to_string(),
                "replay_roi_release_gate_summary".to_string(),
                "threshold:max_false_replay_rate".to_string(),
                "threshold:min_reasoning_avoided_tokens".to_string(),
                "threshold:min_replay_hit_rate".to_string(),
                "threshold:min_replay_roi".to_string(),
                "threshold:require_replay_safety".to_string(),
                "window_seconds:86400".to_string(),
            ]
        );

        let rendered = serde_json::to_string(&output).unwrap();
        assert!(rendered.starts_with("{\"status\":\"fail_closed\",\"failed_checks\":"));
        assert_eq!(rendered, serde_json::to_string(&output).unwrap());
    }

    #[test]
    fn replay_roi_release_gate_summary_evaluator_passes_with_threshold_compliance() {
        let input = ReplayRoiReleaseGateInputContract {
            generated_at: Utc::now().to_rfc3339(),
            window_seconds: 86_400,
            aggregation_dimensions: REPLAY_RELEASE_GATE_AGGREGATION_DIMENSIONS
                .iter()
                .map(|dimension| (*dimension).to_string())
                .collect(),
            replay_attempts_total: 10,
            replay_success_total: 9,
            replay_failure_total: 1,
            replay_hit_rate: 0.9,
            false_replay_rate: 0.1,
            reasoning_avoided_tokens: 960,
            replay_fallback_cost_total: 64,
            replay_roi: compute_replay_roi(960, 64),
            replay_safety: true,
            replay_safety_signal: ReplayRoiReleaseGateSafetySignal {
                fail_closed_default: true,
                rollback_ready: true,
                audit_trail_complete: true,
                has_replay_activity: true,
            },
            thresholds: ReplayRoiReleaseGateThresholds::default(),
            fail_closed_policy: ReplayRoiReleaseGateFailClosedPolicy::default(),
        };

        let output = evaluate_replay_roi_release_gate_contract_input(&input);
        assert_eq!(output.status, ReplayRoiReleaseGateStatus::Pass);
        assert!(output.failed_checks.is_empty());
        assert!(output.summary.contains("release gate pass"));
    }

    #[test]
    fn replay_roi_release_gate_summary_evaluator_fail_closed_on_threshold_violations() {
        let input = ReplayRoiReleaseGateInputContract {
            generated_at: Utc::now().to_rfc3339(),
            window_seconds: 86_400,
            aggregation_dimensions: REPLAY_RELEASE_GATE_AGGREGATION_DIMENSIONS
                .iter()
                .map(|dimension| (*dimension).to_string())
                .collect(),
            replay_attempts_total: 10,
            replay_success_total: 4,
            replay_failure_total: 6,
            replay_hit_rate: 0.4,
            false_replay_rate: 0.6,
            reasoning_avoided_tokens: 80,
            replay_fallback_cost_total: 400,
            replay_roi: compute_replay_roi(80, 400),
            replay_safety: false,
            replay_safety_signal: ReplayRoiReleaseGateSafetySignal {
                fail_closed_default: true,
                rollback_ready: true,
                audit_trail_complete: true,
                has_replay_activity: true,
            },
            thresholds: ReplayRoiReleaseGateThresholds::default(),
            fail_closed_policy: ReplayRoiReleaseGateFailClosedPolicy::default(),
        };

        let output = evaluate_replay_roi_release_gate_contract_input(&input);
        assert_eq!(output.status, ReplayRoiReleaseGateStatus::FailClosed);
        assert!(output
            .failed_checks
            .iter()
            .any(|check| check == "replay_hit_rate_below_threshold"));
        assert!(output
            .failed_checks
            .iter()
            .any(|check| check == "false_replay_rate_above_threshold"));
        assert!(output
            .failed_checks
            .iter()
            .any(|check| check == "replay_roi_below_threshold"));
        assert!(output.summary.contains("release gate fail_closed"));
    }

    #[test]
    fn replay_roi_release_gate_summary_evaluator_marks_missing_data_indeterminate() {
        let input = ReplayRoiReleaseGateInputContract {
            generated_at: String::new(),
            window_seconds: 86_400,
            aggregation_dimensions: REPLAY_RELEASE_GATE_AGGREGATION_DIMENSIONS
                .iter()
                .map(|dimension| (*dimension).to_string())
                .collect(),
            replay_attempts_total: 0,
            replay_success_total: 0,
            replay_failure_total: 0,
            replay_hit_rate: 0.0,
            false_replay_rate: 0.0,
            reasoning_avoided_tokens: 0,
            replay_fallback_cost_total: 0,
            replay_roi: 0.0,
            replay_safety: false,
            replay_safety_signal: ReplayRoiReleaseGateSafetySignal {
                fail_closed_default: true,
                rollback_ready: true,
                audit_trail_complete: true,
                has_replay_activity: false,
            },
            thresholds: ReplayRoiReleaseGateThresholds::default(),
            fail_closed_policy: ReplayRoiReleaseGateFailClosedPolicy::default(),
        };

        let output = evaluate_replay_roi_release_gate_contract_input(&input);
        assert_eq!(output.status, ReplayRoiReleaseGateStatus::Indeterminate);
        assert!(output
            .failed_checks
            .iter()
            .any(|check| check == "missing_generated_at"));
        assert!(output
            .failed_checks
            .iter()
            .any(|check| check == "missing_replay_attempts"));
        assert!(output
            .summary
            .contains("release gate indeterminate (fail-closed)"));
    }

    #[test]
    fn stale_replay_targets_require_confidence_revalidation() {
        let now = Utc::now();
        let projection = EvolutionProjection {
            genes: vec![Gene {
                id: "gene-stale".into(),
                signals: vec!["missing readme".into()],
                strategy: vec!["README.md".into()],
                validation: vec!["test".into()],
                state: AssetState::Promoted,
                task_class_id: None,
            }],
            capsules: vec![Capsule {
                id: "capsule-stale".into(),
                gene_id: "gene-stale".into(),
                mutation_id: "mutation-stale".into(),
                run_id: "run-stale".into(),
                diff_hash: "hash".into(),
                confidence: 0.8,
                env: replay_input("missing readme").env,
                outcome: Outcome {
                    success: true,
                    validation_profile: "test".into(),
                    validation_duration_ms: 1,
                    changed_files: vec!["README.md".into()],
                    validator_hash: "validator".into(),
                    lines_changed: 1,
                    replay_verified: false,
                },
                state: AssetState::Promoted,
            }],
            reuse_counts: BTreeMap::from([("gene-stale".into(), 1)]),
            attempt_counts: BTreeMap::from([("gene-stale".into(), 1)]),
            last_updated_at: BTreeMap::from([(
                "gene-stale".into(),
                (now - Duration::hours(48)).to_rfc3339(),
            )]),
            spec_ids_by_gene: BTreeMap::new(),
        };

        let targets = stale_replay_revalidation_targets(&projection, now);

        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].gene_id, "gene-stale");
        assert_eq!(targets[0].capsule_ids, vec!["capsule-stale".to_string()]);
        assert!(targets[0].decayed_confidence < MIN_REPLAY_CONFIDENCE);
    }

    #[tokio::test]
    async fn remote_replay_prefers_closest_environment_match() {
        let (evo, _) = build_test_evo("remote-env", "run-remote-env", command_validator());
        let input = replay_input("env-signal");

        let envelope_a = remote_publish_envelope_with_env(
            "node-a",
            "run-remote-a",
            "gene-a",
            "capsule-a",
            "mutation-a",
            "env-signal",
            "A.md",
            "# from a",
            input.env.clone(),
        );
        let envelope_b = remote_publish_envelope_with_env(
            "node-b",
            "run-remote-b",
            "gene-b",
            "capsule-b",
            "mutation-b",
            "env-signal",
            "B.md",
            "# from b",
            EnvFingerprint {
                rustc_version: "old-rustc".into(),
                cargo_lock_hash: "other-lock".into(),
                target_triple: "aarch64-apple-darwin".into(),
                os: "linux".into(),
            },
        );

        evo.import_remote_envelope(&envelope_a).unwrap();
        evo.import_remote_envelope(&envelope_b).unwrap();

        let decision = evo.replay_or_fallback(input).await.unwrap();

        assert!(decision.used_capsule);
        assert_eq!(decision.capsule_id, Some("capsule-a".into()));
        assert!(!decision.fallback_to_planner);
    }

    #[test]
    fn remote_cold_start_scoring_caps_distinct_query_coverage() {
        let (evo, _) = build_test_evo("remote-score", "run-remote-score", command_validator());
        let input = replay_input("missing readme");

        let exact = remote_publish_envelope_with_signals(
            "node-exact",
            "run-remote-exact",
            "gene-exact",
            "capsule-exact",
            "mutation-exact",
            vec!["missing readme".into()],
            vec!["missing readme".into()],
            "EXACT.md",
            "# exact",
            input.env.clone(),
        );
        let overlapping = remote_publish_envelope_with_signals(
            "node-overlap",
            "run-remote-overlap",
            "gene-overlap",
            "capsule-overlap",
            "mutation-overlap",
            vec!["missing readme".into()],
            vec!["missing".into(), "readme".into()],
            "OVERLAP.md",
            "# overlap",
            input.env.clone(),
        );

        evo.import_remote_envelope(&exact).unwrap();
        evo.import_remote_envelope(&overlapping).unwrap();

        let candidates = quarantined_remote_exact_match_candidates(evo.store.as_ref(), &input);
        let exact_candidate = candidates
            .iter()
            .find(|candidate| candidate.gene.id == "gene-exact")
            .unwrap();
        let overlap_candidate = candidates
            .iter()
            .find(|candidate| candidate.gene.id == "gene-overlap")
            .unwrap();

        assert_eq!(exact_candidate.score, 1.0);
        assert_eq!(overlap_candidate.score, 1.0);
        assert!(candidates.iter().all(|candidate| candidate.score <= 1.0));
    }

    #[test]
    fn exact_match_candidates_respect_spec_linked_events() {
        let (evo, _) = build_test_evo(
            "spec-linked-filter",
            "run-spec-linked-filter",
            command_validator(),
        );
        let mut input = replay_input("missing readme");
        input.spec_id = Some("spec-readme".into());

        let mut mutation = sample_mutation();
        mutation.intent.id = "mutation-spec-linked".into();
        mutation.intent.spec_id = None;
        let gene = Gene {
            id: "gene-spec-linked".into(),
            signals: vec!["missing readme".into()],
            strategy: vec!["README.md".into()],
            validation: vec!["test".into()],
            state: AssetState::Promoted,
            task_class_id: None,
        };
        let capsule = Capsule {
            id: "capsule-spec-linked".into(),
            gene_id: gene.id.clone(),
            mutation_id: mutation.intent.id.clone(),
            run_id: "run-spec-linked".into(),
            diff_hash: mutation.artifact.content_hash.clone(),
            confidence: 0.9,
            env: input.env.clone(),
            outcome: Outcome {
                success: true,
                validation_profile: "test".into(),
                validation_duration_ms: 1,
                changed_files: vec!["README.md".into()],
                validator_hash: "validator-hash".into(),
                lines_changed: 1,
                replay_verified: false,
            },
            state: AssetState::Promoted,
        };

        evo.store
            .append_event(EvolutionEvent::MutationDeclared { mutation })
            .unwrap();
        evo.store
            .append_event(EvolutionEvent::GeneProjected { gene })
            .unwrap();
        evo.store
            .append_event(EvolutionEvent::CapsuleCommitted { capsule })
            .unwrap();
        evo.store
            .append_event(EvolutionEvent::SpecLinked {
                mutation_id: "mutation-spec-linked".into(),
                spec_id: "spec-readme".into(),
            })
            .unwrap();

        let candidates = exact_match_candidates(evo.store.as_ref(), &input);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].gene.id, "gene-spec-linked");
    }

    #[tokio::test]
    async fn remote_capsule_advances_from_quarantine_to_shadow_then_promoted() {
        let (evo, store) = build_test_evo(
            "remote-quarantine",
            "run-remote-quarantine",
            command_validator(),
        );
        let envelope = remote_publish_envelope(
            "node-remote",
            "run-remote-quarantine",
            "gene-remote",
            "capsule-remote",
            "mutation-remote",
            "remote-signal",
            "REMOTE.md",
            "# from remote",
        );

        evo.import_remote_envelope(&envelope).unwrap();

        let before_replay = store.rebuild_projection().unwrap();
        let imported_gene = before_replay
            .genes
            .iter()
            .find(|gene| gene.id == "gene-remote")
            .unwrap();
        let imported_capsule = before_replay
            .capsules
            .iter()
            .find(|capsule| capsule.id == "capsule-remote")
            .unwrap();
        assert_eq!(imported_gene.state, AssetState::Quarantined);
        assert_eq!(imported_capsule.state, AssetState::Quarantined);
        let exported_before_replay =
            export_promoted_assets_from_store(store.as_ref(), "node-local").unwrap();
        assert!(exported_before_replay.assets.is_empty());

        let first_decision = evo
            .replay_or_fallback(replay_input("remote-signal"))
            .await
            .unwrap();

        assert!(first_decision.used_capsule);
        assert_eq!(first_decision.capsule_id, Some("capsule-remote".into()));

        let after_first_replay = store.rebuild_projection().unwrap();
        let shadow_gene = after_first_replay
            .genes
            .iter()
            .find(|gene| gene.id == "gene-remote")
            .unwrap();
        let shadow_capsule = after_first_replay
            .capsules
            .iter()
            .find(|capsule| capsule.id == "capsule-remote")
            .unwrap();
        assert_eq!(shadow_gene.state, AssetState::ShadowValidated);
        assert_eq!(shadow_capsule.state, AssetState::ShadowValidated);
        let exported_after_first_replay =
            export_promoted_assets_from_store(store.as_ref(), "node-local").unwrap();
        assert!(exported_after_first_replay.assets.is_empty());

        let second_decision = evo
            .replay_or_fallback(replay_input("remote-signal"))
            .await
            .unwrap();
        assert!(second_decision.used_capsule);
        assert_eq!(second_decision.capsule_id, Some("capsule-remote".into()));

        let after_second_replay = store.rebuild_projection().unwrap();
        let promoted_gene = after_second_replay
            .genes
            .iter()
            .find(|gene| gene.id == "gene-remote")
            .unwrap();
        let promoted_capsule = after_second_replay
            .capsules
            .iter()
            .find(|capsule| capsule.id == "capsule-remote")
            .unwrap();
        assert_eq!(promoted_gene.state, AssetState::Promoted);
        assert_eq!(promoted_capsule.state, AssetState::Promoted);
        let exported_after_second_replay =
            export_promoted_assets_from_store(store.as_ref(), "node-local").unwrap();
        assert_eq!(exported_after_second_replay.assets.len(), 3);
        assert!(exported_after_second_replay
            .assets
            .iter()
            .any(|asset| matches!(
                asset,
                NetworkAsset::EvolutionEvent {
                    event: EvolutionEvent::MutationDeclared { .. }
                }
            )));
    }

    #[tokio::test]
    async fn publish_local_assets_include_mutation_payload_for_remote_replay() {
        let (source, source_store) = build_test_evo(
            "remote-publish-export",
            "run-remote-publish-export",
            command_validator(),
        );
        source
            .capture_successful_mutation(&"run-remote-publish-export".into(), sample_mutation())
            .await
            .unwrap();
        let envelope = EvolutionNetworkNode::new(source_store.clone())
            .publish_local_assets("node-source")
            .unwrap();
        assert!(envelope.assets.iter().any(|asset| matches!(
            asset,
            NetworkAsset::EvolutionEvent {
                event: EvolutionEvent::MutationDeclared { mutation }
            } if mutation.intent.id == "mutation-1"
        )));

        let (remote, _) = build_test_evo(
            "remote-publish-import",
            "run-remote-publish-import",
            command_validator(),
        );
        remote.import_remote_envelope(&envelope).unwrap();

        let decision = remote
            .replay_or_fallback(replay_input("missing readme"))
            .await
            .unwrap();

        assert!(decision.used_capsule);
        assert!(!decision.fallback_to_planner);
    }

    #[tokio::test]
    async fn import_remote_envelope_records_manifest_validation_event() {
        let (source, source_store) = build_test_evo(
            "remote-manifest-success-source",
            "run-remote-manifest-success-source",
            command_validator(),
        );
        source
            .capture_successful_mutation(
                &"run-remote-manifest-success-source".into(),
                sample_mutation(),
            )
            .await
            .unwrap();
        let envelope = EvolutionNetworkNode::new(source_store.clone())
            .publish_local_assets("node-source")
            .unwrap();

        let (remote, remote_store) = build_test_evo(
            "remote-manifest-success-remote",
            "run-remote-manifest-success-remote",
            command_validator(),
        );
        remote.import_remote_envelope(&envelope).unwrap();

        let events = remote_store.scan(1).unwrap();
        assert!(events.iter().any(|stored| matches!(
            &stored.event,
            EvolutionEvent::ManifestValidated {
                accepted: true,
                reason,
                sender_id: Some(sender_id),
                publisher: Some(publisher),
                asset_ids,
            } if reason == "manifest validated"
                && sender_id == "node-source"
                && publisher == "node-source"
                && !asset_ids.is_empty()
        )));
    }

    #[test]
    fn import_remote_envelope_rejects_invalid_manifest_and_records_audit_event() {
        let (remote, remote_store) = build_test_evo(
            "remote-manifest-invalid",
            "run-remote-manifest-invalid",
            command_validator(),
        );
        let mut envelope = remote_publish_envelope(
            "node-remote",
            "run-remote-manifest-invalid",
            "gene-remote",
            "capsule-remote",
            "mutation-remote",
            "manifest-signal",
            "MANIFEST.md",
            "# drift",
        );
        if let Some(manifest) = envelope.manifest.as_mut() {
            manifest.asset_hash = "tampered-hash".to_string();
        }
        envelope.content_hash = envelope.compute_content_hash();

        let error = remote.import_remote_envelope(&envelope).unwrap_err();
        assert!(error.to_string().contains("manifest"));

        let events = remote_store.scan(1).unwrap();
        assert!(events.iter().any(|stored| matches!(
            &stored.event,
            EvolutionEvent::ManifestValidated {
                accepted: false,
                reason,
                sender_id: Some(sender_id),
                publisher: Some(publisher),
                asset_ids,
            } if reason.contains("manifest asset_hash mismatch")
                && sender_id == "node-remote"
                && publisher == "node-remote"
                && !asset_ids.is_empty()
        )));
    }

    #[tokio::test]
    async fn fetch_assets_include_mutation_payload_for_remote_replay() {
        let (evo, store) = build_test_evo(
            "remote-fetch-export",
            "run-remote-fetch",
            command_validator(),
        );
        evo.capture_successful_mutation(&"run-remote-fetch".into(), sample_mutation())
            .await
            .unwrap();

        let response = EvolutionNetworkNode::new(store.clone())
            .fetch_assets(
                "node-source",
                &FetchQuery {
                    sender_id: "node-client".into(),
                    signals: vec!["missing readme".into()],
                    since_cursor: None,
                    resume_token: None,
                },
            )
            .unwrap();

        assert!(response.assets.iter().any(|asset| matches!(
            asset,
            NetworkAsset::EvolutionEvent {
                event: EvolutionEvent::MutationDeclared { mutation }
            } if mutation.intent.id == "mutation-1"
        )));
        assert!(response
            .assets
            .iter()
            .any(|asset| matches!(asset, NetworkAsset::Gene { .. })));
        assert!(response
            .assets
            .iter()
            .any(|asset| matches!(asset, NetworkAsset::Capsule { .. })));
    }

    #[test]
    fn fetch_assets_delta_sync_supports_since_cursor_and_resume_token() {
        let store_root =
            std::env::temp_dir().join(format!("oris-evokernel-fetch-delta-store-{}", next_id("t")));
        if store_root.exists() {
            fs::remove_dir_all(&store_root).unwrap();
        }
        let store: Arc<dyn EvolutionStore> =
            Arc::new(oris_evolution::JsonlEvolutionStore::new(&store_root));
        let node = EvolutionNetworkNode::new(store.clone());
        node.record_reported_experience(
            "delta-agent",
            "gene-delta-a",
            vec!["delta.signal".into()],
            vec![
                "task_class=delta.signal".into(),
                "task_label=delta replay".into(),
            ],
            vec!["a2a.tasks.report".into()],
        )
        .unwrap();

        let first = node
            .fetch_assets(
                "execution-api",
                &FetchQuery {
                    sender_id: "delta-agent".into(),
                    signals: vec!["delta.signal".into()],
                    since_cursor: None,
                    resume_token: None,
                },
            )
            .unwrap();
        let first_cursor = first.next_cursor.clone().expect("first next_cursor");
        let first_token = first.resume_token.clone().expect("first resume_token");
        assert!(first.assets.iter().any(
            |asset| matches!(asset, NetworkAsset::Gene { gene } if gene.id == "gene-delta-a")
        ));

        let restarted = EvolutionNetworkNode::new(store.clone());
        restarted
            .record_reported_experience(
                "delta-agent",
                "gene-delta-b",
                vec!["delta.signal".into()],
                vec![
                    "task_class=delta.signal".into(),
                    "task_label=delta replay".into(),
                ],
                vec!["a2a.tasks.report".into()],
            )
            .unwrap();

        let from_token = restarted
            .fetch_assets(
                "execution-api",
                &FetchQuery {
                    sender_id: "delta-agent".into(),
                    signals: vec!["delta.signal".into()],
                    since_cursor: None,
                    resume_token: Some(first_token),
                },
            )
            .unwrap();
        assert!(from_token.assets.iter().any(
            |asset| matches!(asset, NetworkAsset::Gene { gene } if gene.id == "gene-delta-b")
        ));
        assert!(!from_token.assets.iter().any(
            |asset| matches!(asset, NetworkAsset::Gene { gene } if gene.id == "gene-delta-a")
        ));
        assert_eq!(
            from_token.sync_audit.requested_cursor,
            Some(first_cursor.clone())
        );
        assert!(from_token.sync_audit.applied_count >= 1);

        let from_cursor = restarted
            .fetch_assets(
                "execution-api",
                &FetchQuery {
                    sender_id: "delta-agent".into(),
                    signals: vec!["delta.signal".into()],
                    since_cursor: Some(first_cursor),
                    resume_token: None,
                },
            )
            .unwrap();
        assert!(from_cursor.assets.iter().any(
            |asset| matches!(asset, NetworkAsset::Gene { gene } if gene.id == "gene-delta-b")
        ));
    }

    #[test]
    fn partial_remote_import_keeps_publisher_for_already_imported_assets() {
        let store_root = std::env::temp_dir().join(format!(
            "oris-evokernel-remote-partial-store-{}",
            std::process::id()
        ));
        if store_root.exists() {
            fs::remove_dir_all(&store_root).unwrap();
        }
        let store: Arc<dyn EvolutionStore> = Arc::new(FailOnAppendStore::new(store_root, 5));
        let evo = build_test_evo_with_store(
            "remote-partial",
            "run-remote-partial",
            command_validator(),
            store.clone(),
        );
        let envelope = remote_publish_envelope(
            "node-partial",
            "run-remote-partial",
            "gene-partial",
            "capsule-partial",
            "mutation-partial",
            "partial-signal",
            "PARTIAL.md",
            "# partial",
        );

        let result = evo.import_remote_envelope(&envelope);

        assert!(matches!(result, Err(EvoKernelError::Store(_))));
        let projection = store.rebuild_projection().unwrap();
        assert!(projection
            .genes
            .iter()
            .any(|gene| gene.id == "gene-partial"));
        assert!(projection.capsules.is_empty());
        let publishers = evo.remote_publishers.lock().unwrap();
        assert_eq!(
            publishers.get("gene-partial").map(String::as_str),
            Some("node-partial")
        );
    }

    #[test]
    fn retry_remote_import_after_partial_failure_only_imports_missing_assets() {
        let store_root = std::env::temp_dir().join(format!(
            "oris-evokernel-remote-partial-retry-store-{}",
            next_id("t")
        ));
        if store_root.exists() {
            fs::remove_dir_all(&store_root).unwrap();
        }
        let store: Arc<dyn EvolutionStore> = Arc::new(FailOnAppendStore::new(store_root, 5));
        let evo = build_test_evo_with_store(
            "remote-partial-retry",
            "run-remote-partial-retry",
            command_validator(),
            store.clone(),
        );
        let envelope = remote_publish_envelope(
            "node-partial",
            "run-remote-partial-retry",
            "gene-partial-retry",
            "capsule-partial-retry",
            "mutation-partial-retry",
            "partial-retry-signal",
            "PARTIAL_RETRY.md",
            "# partial retry",
        );

        let first = evo.import_remote_envelope(&envelope);
        assert!(matches!(first, Err(EvoKernelError::Store(_))));

        let retry = evo.import_remote_envelope(&envelope).unwrap();

        assert_eq!(retry.imported_asset_ids, vec!["capsule-partial-retry"]);
        let projection = store.rebuild_projection().unwrap();
        let gene = projection
            .genes
            .iter()
            .find(|gene| gene.id == "gene-partial-retry")
            .unwrap();
        assert_eq!(gene.state, AssetState::Quarantined);
        let capsule = projection
            .capsules
            .iter()
            .find(|capsule| capsule.id == "capsule-partial-retry")
            .unwrap();
        assert_eq!(capsule.state, AssetState::Quarantined);
        assert_eq!(projection.attempt_counts["gene-partial-retry"], 1);

        let events = store.scan(1).unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|stored| {
                    matches!(
                        &stored.event,
                        EvolutionEvent::MutationDeclared { mutation }
                            if mutation.intent.id == "mutation-partial-retry"
                    )
                })
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|stored| {
                    matches!(
                        &stored.event,
                        EvolutionEvent::GeneProjected { gene } if gene.id == "gene-partial-retry"
                    )
                })
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|stored| {
                    matches!(
                        &stored.event,
                        EvolutionEvent::CapsuleCommitted { capsule }
                            if capsule.id == "capsule-partial-retry"
                    )
                })
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn duplicate_remote_import_does_not_requarantine_locally_validated_assets() {
        let (evo, store) = build_test_evo(
            "remote-idempotent",
            "run-remote-idempotent",
            command_validator(),
        );
        let envelope = remote_publish_envelope(
            "node-idempotent",
            "run-remote-idempotent",
            "gene-idempotent",
            "capsule-idempotent",
            "mutation-idempotent",
            "idempotent-signal",
            "IDEMPOTENT.md",
            "# idempotent",
        );

        let first = evo.import_remote_envelope(&envelope).unwrap();
        assert_eq!(
            first.imported_asset_ids,
            vec!["gene-idempotent", "capsule-idempotent"]
        );

        let decision = evo
            .replay_or_fallback(replay_input("idempotent-signal"))
            .await
            .unwrap();
        assert!(decision.used_capsule);
        assert_eq!(decision.capsule_id, Some("capsule-idempotent".into()));

        let projection_before = store.rebuild_projection().unwrap();
        let attempts_before = projection_before.attempt_counts["gene-idempotent"];
        let gene_before = projection_before
            .genes
            .iter()
            .find(|gene| gene.id == "gene-idempotent")
            .unwrap();
        assert_eq!(gene_before.state, AssetState::ShadowValidated);
        let capsule_before = projection_before
            .capsules
            .iter()
            .find(|capsule| capsule.id == "capsule-idempotent")
            .unwrap();
        assert_eq!(capsule_before.state, AssetState::ShadowValidated);

        let second = evo.import_remote_envelope(&envelope).unwrap();
        assert!(second.imported_asset_ids.is_empty());

        let projection_after = store.rebuild_projection().unwrap();
        assert_eq!(
            projection_after.attempt_counts["gene-idempotent"],
            attempts_before
        );
        let gene_after = projection_after
            .genes
            .iter()
            .find(|gene| gene.id == "gene-idempotent")
            .unwrap();
        assert_eq!(gene_after.state, AssetState::ShadowValidated);
        let capsule_after = projection_after
            .capsules
            .iter()
            .find(|capsule| capsule.id == "capsule-idempotent")
            .unwrap();
        assert_eq!(capsule_after.state, AssetState::ShadowValidated);

        let third_decision = evo
            .replay_or_fallback(replay_input("idempotent-signal"))
            .await
            .unwrap();
        assert!(third_decision.used_capsule);
        assert_eq!(third_decision.capsule_id, Some("capsule-idempotent".into()));

        let projection_promoted = store.rebuild_projection().unwrap();
        let promoted_gene = projection_promoted
            .genes
            .iter()
            .find(|gene| gene.id == "gene-idempotent")
            .unwrap();
        let promoted_capsule = projection_promoted
            .capsules
            .iter()
            .find(|capsule| capsule.id == "capsule-idempotent")
            .unwrap();
        assert_eq!(promoted_gene.state, AssetState::Promoted);
        assert_eq!(promoted_capsule.state, AssetState::Promoted);

        let events = store.scan(1).unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|stored| {
                    matches!(
                        &stored.event,
                        EvolutionEvent::MutationDeclared { mutation }
                            if mutation.intent.id == "mutation-idempotent"
                    )
                })
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|stored| {
                    matches!(
                        &stored.event,
                        EvolutionEvent::GeneProjected { gene } if gene.id == "gene-idempotent"
                    )
                })
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|stored| {
                    matches!(
                        &stored.event,
                        EvolutionEvent::CapsuleCommitted { capsule }
                            if capsule.id == "capsule-idempotent"
                    )
                })
                .count(),
            1
        );

        assert_eq!(first.sync_audit.scanned_count, envelope.assets.len());
        assert_eq!(first.sync_audit.failed_count, 0);
        assert_eq!(second.sync_audit.applied_count, 0);
        assert_eq!(second.sync_audit.skipped_count, envelope.assets.len());
        assert!(second.resume_token.is_some());
    }

    #[tokio::test]
    async fn insufficient_evu_blocks_publish_but_not_local_replay() {
        let (evo, _) = build_test_evo("stake-gate", "run-stake", command_validator());
        let capsule = evo
            .capture_successful_mutation(&"run-stake".into(), sample_mutation())
            .await
            .unwrap();
        let publish = evo.export_promoted_assets("node-local");
        assert!(matches!(publish, Err(EvoKernelError::Validation(_))));

        let decision = evo
            .replay_or_fallback(replay_input("missing readme"))
            .await
            .unwrap();
        assert!(decision.used_capsule);
        assert_eq!(decision.capsule_id, Some(capsule.id));
    }

    #[tokio::test]
    async fn second_replay_validation_failure_revokes_gene_immediately() {
        let (capturer, store) = build_test_evo("revoke-replay", "run-capture", command_validator());
        let capsule = capturer
            .capture_successful_mutation(&"run-capture".into(), sample_mutation())
            .await
            .unwrap();

        let failing_validator: Arc<dyn Validator> = Arc::new(FixedValidator { success: false });
        let failing_replay = build_test_evo_with_store(
            "revoke-replay",
            "run-replay-fail",
            failing_validator,
            store.clone(),
        );

        let first = failing_replay
            .replay_or_fallback(replay_input("missing readme"))
            .await
            .unwrap();
        let second = failing_replay
            .replay_or_fallback(replay_input("missing readme"))
            .await
            .unwrap();

        assert!(!first.used_capsule);
        assert!(first.fallback_to_planner);
        assert!(!second.used_capsule);
        assert!(second.fallback_to_planner);

        let projection = store.rebuild_projection().unwrap();
        let gene = projection
            .genes
            .iter()
            .find(|gene| gene.id == capsule.gene_id)
            .unwrap();
        assert_eq!(gene.state, AssetState::Promoted);
        let committed_capsule = projection
            .capsules
            .iter()
            .find(|current| current.id == capsule.id)
            .unwrap();
        assert_eq!(committed_capsule.state, AssetState::Promoted);

        let events = store.scan(1).unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|stored| {
                    matches!(
                        &stored.event,
                        EvolutionEvent::ValidationFailed {
                            gene_id: Some(gene_id),
                            ..
                        } if gene_id == &capsule.gene_id
                    )
                })
                .count(),
            1
        );
        assert!(!events.iter().any(|stored| {
            matches!(
                &stored.event,
                EvolutionEvent::GeneRevoked { gene_id, .. } if gene_id == &capsule.gene_id
            )
        }));

        let recovered = build_test_evo_with_store(
            "revoke-replay",
            "run-replay-check",
            command_validator(),
            store.clone(),
        );
        let after_revoke = recovered
            .replay_or_fallback(replay_input("missing readme"))
            .await
            .unwrap();
        assert!(!after_revoke.used_capsule);
        assert!(after_revoke.fallback_to_planner);
        assert!(after_revoke.reason.contains("below replay threshold"));
    }

    #[tokio::test]
    async fn remote_reuse_success_rewards_publisher_and_biases_selection() {
        let ledger = Arc::new(Mutex::new(EvuLedger {
            accounts: vec![],
            reputations: vec![
                oris_economics::ReputationRecord {
                    node_id: "node-a".into(),
                    publish_success_rate: 0.4,
                    validator_accuracy: 0.4,
                    reuse_impact: 0,
                },
                oris_economics::ReputationRecord {
                    node_id: "node-b".into(),
                    publish_success_rate: 0.95,
                    validator_accuracy: 0.95,
                    reuse_impact: 8,
                },
            ],
        }));
        let (evo, _) = build_test_evo("remote-success", "run-remote", command_validator());
        let evo = evo.with_economics(ledger.clone());

        let envelope_a = remote_publish_envelope(
            "node-a",
            "run-remote-a",
            "gene-a",
            "capsule-a",
            "mutation-a",
            "shared-signal",
            "A.md",
            "# from a",
        );
        let envelope_b = remote_publish_envelope(
            "node-b",
            "run-remote-b",
            "gene-b",
            "capsule-b",
            "mutation-b",
            "shared-signal",
            "B.md",
            "# from b",
        );

        evo.import_remote_envelope(&envelope_a).unwrap();
        evo.import_remote_envelope(&envelope_b).unwrap();

        let decision = evo
            .replay_or_fallback(replay_input("shared-signal"))
            .await
            .unwrap();

        assert!(decision.used_capsule);
        assert_eq!(decision.capsule_id, Some("capsule-b".into()));
        let locked = ledger.lock().unwrap();
        let rewarded = locked
            .accounts
            .iter()
            .find(|item| item.node_id == "node-b")
            .unwrap();
        assert_eq!(rewarded.balance, evo.stake_policy.reuse_reward);
        assert!(
            locked.selector_reputation_bias()["node-b"]
                > locked.selector_reputation_bias()["node-a"]
        );
    }

    #[tokio::test]
    async fn remote_reuse_settlement_tracks_selected_capsule_publisher_for_shared_gene() {
        let ledger = Arc::new(Mutex::new(EvuLedger::default()));
        let (evo, _) = build_test_evo(
            "remote-shared-publisher",
            "run-remote-shared-publisher",
            command_validator(),
        );
        let evo = evo.with_economics(ledger.clone());
        let input = replay_input("shared-signal");
        let preferred = remote_publish_envelope_with_env(
            "node-a",
            "run-remote-a",
            "gene-shared",
            "capsule-preferred",
            "mutation-preferred",
            "shared-signal",
            "A.md",
            "# from a",
            input.env.clone(),
        );
        let fallback = remote_publish_envelope_with_env(
            "node-b",
            "run-remote-b",
            "gene-shared",
            "capsule-fallback",
            "mutation-fallback",
            "shared-signal",
            "B.md",
            "# from b",
            EnvFingerprint {
                rustc_version: "old-rustc".into(),
                cargo_lock_hash: "other-lock".into(),
                target_triple: "aarch64-apple-darwin".into(),
                os: "linux".into(),
            },
        );

        evo.import_remote_envelope(&preferred).unwrap();
        evo.import_remote_envelope(&fallback).unwrap();

        let decision = evo.replay_or_fallback(input).await.unwrap();

        assert!(decision.used_capsule);
        assert_eq!(decision.capsule_id, Some("capsule-preferred".into()));
        let locked = ledger.lock().unwrap();
        let rewarded = locked
            .accounts
            .iter()
            .find(|item| item.node_id == "node-a")
            .unwrap();
        assert_eq!(rewarded.balance, evo.stake_policy.reuse_reward);
        assert!(locked.accounts.iter().all(|item| item.node_id != "node-b"));
    }

    #[test]
    fn select_candidates_surfaces_ranked_remote_cold_start_candidates() {
        let ledger = Arc::new(Mutex::new(EvuLedger {
            accounts: vec![],
            reputations: vec![
                oris_economics::ReputationRecord {
                    node_id: "node-a".into(),
                    publish_success_rate: 0.4,
                    validator_accuracy: 0.4,
                    reuse_impact: 0,
                },
                oris_economics::ReputationRecord {
                    node_id: "node-b".into(),
                    publish_success_rate: 0.95,
                    validator_accuracy: 0.95,
                    reuse_impact: 8,
                },
            ],
        }));
        let (evo, _) = build_test_evo("remote-select", "run-remote-select", command_validator());
        let evo = evo.with_economics(ledger);

        let envelope_a = remote_publish_envelope(
            "node-a",
            "run-remote-a",
            "gene-a",
            "capsule-a",
            "mutation-a",
            "shared-signal",
            "A.md",
            "# from a",
        );
        let envelope_b = remote_publish_envelope(
            "node-b",
            "run-remote-b",
            "gene-b",
            "capsule-b",
            "mutation-b",
            "shared-signal",
            "B.md",
            "# from b",
        );

        evo.import_remote_envelope(&envelope_a).unwrap();
        evo.import_remote_envelope(&envelope_b).unwrap();

        let candidates = evo.select_candidates(&replay_input("shared-signal"));

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].gene.id, "gene-b");
        assert_eq!(candidates[0].capsules[0].id, "capsule-b");
    }

    #[tokio::test]
    async fn remote_reuse_publisher_bias_survives_restart() {
        let ledger = Arc::new(Mutex::new(EvuLedger {
            accounts: vec![],
            reputations: vec![
                oris_economics::ReputationRecord {
                    node_id: "node-a".into(),
                    publish_success_rate: 0.4,
                    validator_accuracy: 0.4,
                    reuse_impact: 0,
                },
                oris_economics::ReputationRecord {
                    node_id: "node-b".into(),
                    publish_success_rate: 0.95,
                    validator_accuracy: 0.95,
                    reuse_impact: 8,
                },
            ],
        }));
        let store_root = std::env::temp_dir().join(format!(
            "oris-evokernel-remote-restart-store-{}",
            next_id("t")
        ));
        if store_root.exists() {
            fs::remove_dir_all(&store_root).unwrap();
        }
        let store: Arc<dyn EvolutionStore> =
            Arc::new(oris_evolution::JsonlEvolutionStore::new(&store_root));
        let evo = build_test_evo_with_store(
            "remote-success-restart-source",
            "run-remote-restart-source",
            command_validator(),
            store.clone(),
        )
        .with_economics(ledger.clone());

        let envelope_a = remote_publish_envelope(
            "node-a",
            "run-remote-a",
            "gene-a",
            "capsule-a",
            "mutation-a",
            "shared-signal",
            "A.md",
            "# from a",
        );
        let envelope_b = remote_publish_envelope(
            "node-b",
            "run-remote-b",
            "gene-b",
            "capsule-b",
            "mutation-b",
            "shared-signal",
            "B.md",
            "# from b",
        );

        evo.import_remote_envelope(&envelope_a).unwrap();
        evo.import_remote_envelope(&envelope_b).unwrap();

        let recovered = build_test_evo_with_store(
            "remote-success-restart-recovered",
            "run-remote-restart-recovered",
            command_validator(),
            store.clone(),
        )
        .with_economics(ledger.clone());

        let decision = recovered
            .replay_or_fallback(replay_input("shared-signal"))
            .await
            .unwrap();

        assert!(decision.used_capsule);
        assert_eq!(decision.capsule_id, Some("capsule-b".into()));
        let locked = ledger.lock().unwrap();
        let rewarded = locked
            .accounts
            .iter()
            .find(|item| item.node_id == "node-b")
            .unwrap();
        assert_eq!(rewarded.balance, recovered.stake_policy.reuse_reward);
    }

    #[tokio::test]
    async fn remote_reuse_failure_penalizes_remote_reputation() {
        let ledger = Arc::new(Mutex::new(EvuLedger::default()));
        let failing_validator: Arc<dyn Validator> = Arc::new(FixedValidator { success: false });
        let (evo, _) = build_test_evo("remote-failure", "run-failure", failing_validator);
        let evo = evo.with_economics(ledger.clone());

        let envelope = remote_publish_envelope(
            "node-remote",
            "run-remote-failed",
            "gene-remote",
            "capsule-remote",
            "mutation-remote",
            "failure-signal",
            "FAILED.md",
            "# from remote",
        );
        evo.import_remote_envelope(&envelope).unwrap();

        let decision = evo
            .replay_or_fallback(replay_input("failure-signal"))
            .await
            .unwrap();

        assert!(!decision.used_capsule);
        assert!(decision.fallback_to_planner);

        let signal = evo.economics_signal("node-remote").unwrap();
        assert_eq!(signal.available_evu, 0);
        assert!(signal.publish_success_rate < 0.5);
        assert!(signal.validator_accuracy < 0.5);
    }

    #[test]
    fn ensure_builtin_experience_assets_is_idempotent_and_fetchable() {
        let store_root = std::env::temp_dir().join(format!(
            "oris-evokernel-builtin-experience-store-{}",
            next_id("t")
        ));
        if store_root.exists() {
            fs::remove_dir_all(&store_root).unwrap();
        }
        let store: Arc<dyn EvolutionStore> =
            Arc::new(oris_evolution::JsonlEvolutionStore::new(&store_root));
        let node = EvolutionNetworkNode::new(store.clone());

        let first = node
            .ensure_builtin_experience_assets("runtime-bootstrap")
            .unwrap();
        assert!(!first.imported_asset_ids.is_empty());

        let second = node
            .ensure_builtin_experience_assets("runtime-bootstrap")
            .unwrap();
        assert!(second.imported_asset_ids.is_empty());

        let fetch = node
            .fetch_assets(
                "execution-api",
                &FetchQuery {
                    sender_id: "compat-agent".into(),
                    signals: vec!["error".into()],
                    since_cursor: None,
                    resume_token: None,
                },
            )
            .unwrap();

        let mut has_builtin_evomap = false;
        for asset in fetch.assets {
            if let NetworkAsset::Gene { gene } = asset {
                if strategy_metadata_value(&gene.strategy, "asset_origin").as_deref()
                    == Some("builtin_evomap")
                    && gene.state == AssetState::Promoted
                {
                    has_builtin_evomap = true;
                    break;
                }
            }
        }
        assert!(has_builtin_evomap);
    }

    #[test]
    fn reported_experience_retention_keeps_latest_three_and_preserves_builtin_assets() {
        let store_root = std::env::temp_dir().join(format!(
            "oris-evokernel-reported-retention-store-{}",
            next_id("t")
        ));
        if store_root.exists() {
            fs::remove_dir_all(&store_root).unwrap();
        }
        let store: Arc<dyn EvolutionStore> =
            Arc::new(oris_evolution::JsonlEvolutionStore::new(&store_root));
        let node = EvolutionNetworkNode::new(store.clone());

        node.ensure_builtin_experience_assets("runtime-bootstrap")
            .unwrap();

        for idx in 0..4 {
            node.record_reported_experience(
                "reporter-a",
                format!("reported-docs-rewrite-v{}", idx + 1),
                vec!["docs.rewrite".into(), format!("task-{}", idx + 1)],
                vec![
                    "task_class=docs.rewrite".into(),
                    format!("task_label=Docs rewrite v{}", idx + 1),
                    format!("summary=reported replay {}", idx + 1),
                ],
                vec!["a2a.tasks.report".into()],
            )
            .unwrap();
        }

        let (_, projection) = store.scan_projection().unwrap();
        let reported_promoted = projection
            .genes
            .iter()
            .filter(|gene| {
                gene.state == AssetState::Promoted
                    && strategy_metadata_value(&gene.strategy, "asset_origin").as_deref()
                        == Some("reported_experience")
                    && strategy_metadata_value(&gene.strategy, "task_class").as_deref()
                        == Some("docs.rewrite")
            })
            .count();
        let reported_revoked = projection
            .genes
            .iter()
            .filter(|gene| {
                gene.state == AssetState::Revoked
                    && strategy_metadata_value(&gene.strategy, "asset_origin").as_deref()
                        == Some("reported_experience")
                    && strategy_metadata_value(&gene.strategy, "task_class").as_deref()
                        == Some("docs.rewrite")
            })
            .count();
        let builtin_promoted = projection
            .genes
            .iter()
            .filter(|gene| {
                gene.state == AssetState::Promoted
                    && matches!(
                        strategy_metadata_value(&gene.strategy, "asset_origin").as_deref(),
                        Some("builtin") | Some("builtin_evomap")
                    )
            })
            .count();

        assert_eq!(reported_promoted, 3);
        assert_eq!(reported_revoked, 1);
        assert!(builtin_promoted >= 1);

        let fetch = node
            .fetch_assets(
                "execution-api",
                &FetchQuery {
                    sender_id: "consumer-b".into(),
                    signals: vec!["docs.rewrite".into()],
                    since_cursor: None,
                    resume_token: None,
                },
            )
            .unwrap();
        let docs_genes = fetch
            .assets
            .into_iter()
            .filter_map(|asset| match asset {
                NetworkAsset::Gene { gene } => Some(gene),
                _ => None,
            })
            .filter(|gene| {
                strategy_metadata_value(&gene.strategy, "task_class").as_deref()
                    == Some("docs.rewrite")
            })
            .collect::<Vec<_>>();
        assert!(docs_genes.len() >= 3);
    }

    // ── #252 Supervised DEVLOOP expansion: new task-class boundary tests ──

    #[test]
    fn cargo_dep_upgrade_single_manifest_accepted() {
        let files = vec!["Cargo.toml".to_string()];
        let result = validate_bounded_cargo_dep_files(&files);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), vec!["Cargo.toml"]);
    }

    #[test]
    fn cargo_dep_upgrade_nested_manifest_accepted() {
        let files = vec!["crates/oris-runtime/Cargo.toml".to_string()];
        let result = validate_bounded_cargo_dep_files(&files);
        assert!(result.is_ok());
    }

    #[test]
    fn cargo_dep_upgrade_lock_file_accepted() {
        let files = vec!["Cargo.lock".to_string()];
        let result = validate_bounded_cargo_dep_files(&files);
        assert!(result.is_ok());
    }

    #[test]
    fn cargo_dep_upgrade_too_many_files_rejected_fail_closed() {
        let files: Vec<String> = (0..6)
            .map(|i| format!("crates/crate{i}/Cargo.toml"))
            .collect();
        let result = validate_bounded_cargo_dep_files(&files);
        assert!(
            result.is_err(),
            "more than 5 manifests should be rejected fail-closed"
        );
        assert_eq!(
            result.unwrap_err(),
            MutationProposalContractReasonCode::UnsupportedTaskClass
        );
    }

    #[test]
    fn cargo_dep_upgrade_rs_source_file_rejected_fail_closed() {
        let files = vec!["crates/oris-runtime/src/lib.rs".to_string()];
        let result = validate_bounded_cargo_dep_files(&files);
        assert!(
            result.is_err(),
            ".rs files must be rejected from dep-upgrade scope"
        );
        assert_eq!(
            result.unwrap_err(),
            MutationProposalContractReasonCode::OutOfBoundsPath
        );
    }

    #[test]
    fn cargo_dep_upgrade_path_traversal_rejected_fail_closed() {
        let files = vec!["../outside/Cargo.toml".to_string()];
        let result = validate_bounded_cargo_dep_files(&files);
        assert!(
            result.is_err(),
            "path traversal must be rejected fail-closed"
        );
        assert_eq!(
            result.unwrap_err(),
            MutationProposalContractReasonCode::OutOfBoundsPath
        );
    }

    #[test]
    fn lint_fix_src_rs_file_accepted() {
        let files = vec!["src/lib.rs".to_string()];
        let result = validate_bounded_lint_files(&files);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), vec!["src/lib.rs"]);
    }

    #[test]
    fn lint_fix_crates_rs_file_accepted() {
        let files = vec!["crates/oris-runtime/src/agent.rs".to_string()];
        let result = validate_bounded_lint_files(&files);
        assert!(result.is_ok());
    }

    #[test]
    fn lint_fix_examples_rs_file_accepted() {
        let files = vec!["examples/evo_oris_repo/src/main.rs".to_string()];
        let result = validate_bounded_lint_files(&files);
        assert!(result.is_ok());
    }

    #[test]
    fn lint_fix_too_many_files_rejected_fail_closed() {
        let files: Vec<String> = (0..6).map(|i| format!("src/module{i}.rs")).collect();
        let result = validate_bounded_lint_files(&files);
        assert!(
            result.is_err(),
            "more than 5 source files should be rejected fail-closed"
        );
        assert_eq!(
            result.unwrap_err(),
            MutationProposalContractReasonCode::UnsupportedTaskClass
        );
    }

    #[test]
    fn lint_fix_non_rs_extension_rejected_fail_closed() {
        let files = vec!["src/config.toml".to_string()];
        let result = validate_bounded_lint_files(&files);
        assert!(
            result.is_err(),
            "non-.rs files must be rejected from lint-fix scope"
        );
        assert_eq!(
            result.unwrap_err(),
            MutationProposalContractReasonCode::OutOfBoundsPath
        );
    }

    #[test]
    fn lint_fix_out_of_allowed_prefix_rejected_fail_closed() {
        let files = vec!["scripts/helper.rs".to_string()];
        let result = validate_bounded_lint_files(&files);
        assert!(
            result.is_err(),
            "rs files outside allowed prefixes must be rejected fail-closed"
        );
        assert_eq!(
            result.unwrap_err(),
            MutationProposalContractReasonCode::OutOfBoundsPath
        );
    }

    #[test]
    fn lint_fix_path_traversal_rejected_fail_closed() {
        let files = vec!["../../outside/src/lib.rs".to_string()];
        let result = validate_bounded_lint_files(&files);
        assert!(
            result.is_err(),
            "path traversal must be rejected fail-closed"
        );
        assert_eq!(
            result.unwrap_err(),
            MutationProposalContractReasonCode::OutOfBoundsPath
        );
    }

    #[test]
    fn proposal_scope_classifies_cargo_dep_upgrade() {
        use oris_agent_contract::{
            AgentTask, BoundedTaskClass, HumanApproval, MutationProposal, SupervisedDevloopRequest,
        };
        let request = SupervisedDevloopRequest {
            task: AgentTask {
                id: "t-dep".into(),
                description: "bump serde".into(),
            },
            proposal: MutationProposal {
                intent: "bump serde to 1.0.200".into(),
                expected_effect: "version field updated".into(),
                files: vec!["Cargo.toml".to_string()],
            },
            approval: HumanApproval {
                approved: true,
                approver: Some("alice".into()),
                note: None,
            },
        };
        let scope_result = supervised_devloop_mutation_proposal_scope(&request);
        assert!(scope_result.is_ok());
        assert_eq!(
            scope_result.unwrap().task_class,
            BoundedTaskClass::CargoDepUpgrade
        );
    }

    #[test]
    fn proposal_scope_classifies_lint_fix() {
        use oris_agent_contract::{
            AgentTask, BoundedTaskClass, HumanApproval, MutationProposal, SupervisedDevloopRequest,
        };
        let request = SupervisedDevloopRequest {
            task: AgentTask {
                id: "t-lint".into(),
                description: "cargo fmt fix".into(),
            },
            proposal: MutationProposal {
                intent: "apply cargo fmt to src/lib.rs".into(),
                expected_effect: "formatting normalized".into(),
                files: vec!["src/lib.rs".to_string()],
            },
            approval: HumanApproval {
                approved: true,
                approver: Some("alice".into()),
                note: None,
            },
        };
        let scope_result = supervised_devloop_mutation_proposal_scope(&request);
        assert!(scope_result.is_ok());
        assert_eq!(scope_result.unwrap().task_class, BoundedTaskClass::LintFix);
    }
}
