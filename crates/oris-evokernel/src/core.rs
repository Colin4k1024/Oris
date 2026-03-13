//! EvoKernel orchestration: mutation capture, validation, capsule construction, and replay-first reuse.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use oris_agent_contract::{
    infer_replay_fallback_reason_code, normalize_replay_fallback_contract, AgentRole,
    BoundedTaskClass, CoordinationMessage, CoordinationPlan, CoordinationPrimitive,
    CoordinationResult, CoordinationTask, ExecutionFeedback,
    MutationProposal as AgentMutationProposal, ReplayFeedback, ReplayPlannerDirective,
    SupervisedDevloopOutcome, SupervisedDevloopRequest, SupervisedDevloopStatus,
};
use oris_economics::{EconomicsSignal, EvuLedger, StakePolicy};
use oris_evolution::{
    compute_artifact_hash, decayed_replay_confidence, next_id, stable_hash_json, AssetState,
    BlastRadius, CandidateSource, Capsule, CapsuleId, EnvFingerprint, EvolutionError,
    EvolutionEvent, EvolutionProjection, EvolutionStore, Gene, GeneCandidate, MutationId,
    PreparedMutation, ReplayRoiEvidence, ReplayRoiReasonCode, Selector, SelectorInput,
    StoreBackedSelector, StoredEvolutionEvent, TransitionEvidence, TransitionReasonCode,
    ValidationSnapshot, MIN_REPLAY_CONFIDENCE,
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
    default_store_root, ArtifactEncoding, AssetState as EvoAssetState,
    BlastRadius as EvoBlastRadius, CandidateSource as EvoCandidateSource,
    EnvFingerprint as EvoEnvFingerprint, EvolutionStore as EvoEvolutionStore, JsonlEvolutionStore,
    MutationArtifact, MutationIntent, MutationTarget, Outcome, RiskLevel,
    SelectorInput as EvoSelectorInput, TransitionReasonCode as EvoTransitionReasonCode,
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
        let (current_confidence, historical_peak_confidence, confidence_last_updated_secs) =
            Self::confidence_context(&projection, &best.gene.id);

        self.store
            .append_event(EvolutionEvent::ValidationFailed {
                mutation_id: capsule.mutation_id.clone(),
                report: report.to_snapshot(&validation.profile),
                gene_id: Some(best.gene.id.clone()),
            })
            .map_err(|err| ReplayError::Store(err.to_string()))?;

        let replay_failures = self.replay_failure_count(&best.gene.id)?;
        let governor_decision = self.governor.evaluate(GovernorInput {
            candidate_source: if self.publisher_for_capsule(&capsule.id).is_some() {
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
            current_confidence,
            historical_peak_confidence,
            confidence_last_updated_secs,
        });

        if matches!(governor_decision.target_state, AssetState::Revoked) {
            self.store
                .append_event(EvolutionEvent::PromotionEvaluated {
                    gene_id: best.gene.id.clone(),
                    state: AssetState::Revoked,
                    reason: governor_decision.reason.clone(),
                    reason_code: governor_decision.reason_code.clone(),
                    evidence: Some(TransitionEvidence {
                        replay_attempts: Some(replay_failures),
                        replay_successes: None,
                        replay_success_rate: None,
                        environment_match_factor: None,
                        decayed_confidence: Some(current_confidence),
                        confidence_decay_ratio: if historical_peak_confidence > 0.0 {
                            Some((current_confidence / historical_peak_confidence).clamp(0.0, 1.0))
                        } else {
                            None
                        },
                        summary: Some(format!(
                            "phase=replay_failure_revocation; replay_failures={replay_failures}; current_confidence={:.3}; historical_peak_confidence={:.3}",
                            current_confidence, historical_peak_confidence
                        )),
                    }),
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
    ) -> (f32, f32, Option<u64>) {
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
        (peak_confidence, peak_confidence, age_secs)
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
                self.store
                    .append_event(EvolutionEvent::MutationRejected {
                        mutation_id: mutation.intent.id.clone(),
                        reason: err.to_string(),
                    })
                    .map_err(store_err)?;
                return Err(EvoKernelError::Sandbox(err.to_string()));
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

        let report = self
            .validator
            .run(&receipt, &self.validation_plan)
            .await
            .map_err(|err| EvoKernelError::Validation(err.to_string()))?;
        if !report.success {
            self.store
                .append_event(EvolutionEvent::ValidationFailed {
                    mutation_id: mutation.intent.id.clone(),
                    report: report.to_snapshot(&self.validation_plan.profile),
                    gene_id: None,
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
        let (current_confidence, historical_peak_confidence, confidence_last_updated_secs) =
            StoreReplayExecutor::confidence_context(&projection, &gene.id);
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
            current_confidence,
            historical_peak_confidence,
            confidence_last_updated_secs,
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
                evidence: None,
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
        let (task_class_id, task_label) = replay_task_descriptor(signals);
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
    pub async fn run_supervised_devloop(
        &self,
        run_id: &RunId,
        request: &SupervisedDevloopRequest,
        diff_payload: String,
        base_revision: Option<String>,
    ) -> Result<SupervisedDevloopOutcome, EvoKernelError> {
        let task_class = classify_supervised_devloop_request(request);
        let Some(task_class) = task_class else {
            return Ok(SupervisedDevloopOutcome {
                task_id: request.task.id.clone(),
                task_class: None,
                status: SupervisedDevloopStatus::RejectedByPolicy,
                execution_feedback: None,
                summary: format!(
                    "supervised devloop rejected task '{}' because it is an unsupported task outside the bounded scope",
                    request.task.id
                ),
            });
        };

        if !request.approval.approved {
            return Ok(SupervisedDevloopOutcome {
                task_id: request.task.id.clone(),
                task_class: Some(task_class),
                status: SupervisedDevloopStatus::AwaitingApproval,
                execution_feedback: None,
                summary: format!(
                    "supervised devloop paused task '{}' until explicit human approval is granted",
                    request.task.id
                ),
            });
        }

        let capture = self
            .capture_from_proposal(run_id, &request.proposal, diff_payload, base_revision)
            .await?;
        let approver = request
            .approval
            .approver
            .as_deref()
            .unwrap_or("unknown approver");

        Ok(SupervisedDevloopOutcome {
            task_id: request.task.id.clone(),
            task_class: Some(task_class),
            status: SupervisedDevloopStatus::Executed,
            execution_feedback: Some(Self::feedback_for_agent(&capture)),
            summary: format!(
                "supervised devloop executed task '{}' with explicit approval from {approver}",
                request.task.id
            ),
        })
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
    let strategy = vec![template.id.clone(), "bootstrap".into()];
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
    let strategy = strategy.into_iter().collect::<Vec<_>>();
    let id = stable_hash_json(&(extracted_signals, &strategy, validation_profile))
        .unwrap_or_else(|_| next_id("gene"));
    Gene {
        id,
        signals: extracted_signals.to_vec(),
        strategy,
        validation: vec![validation_profile.to_string()],
        state: AssetState::Promoted,
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
    let normalized = input
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter_map(|raw| {
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
                None
            } else {
                Some(normalized)
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
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
        .find(|value| {
            value.as_str() != "validation passed" && value.as_str() != "validation failed"
        })
        .cloned()
        .unwrap_or_else(|| normalized[0].clone());
    let task_class_id = stable_hash_json(&normalized)
        .unwrap_or_else(|_| compute_artifact_hash(&normalized.join("\n")));
    (task_class_id, task_label)
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

fn classify_supervised_devloop_request(
    request: &SupervisedDevloopRequest,
) -> Option<BoundedTaskClass> {
    let path = request.proposal.files.first()?.trim();
    if request.proposal.files.len() != 1 || path.is_empty() {
        return None;
    }
    let normalized = path.replace('\\', "/");
    if normalized.starts_with("docs/") && normalized.ends_with(".md") {
        Some(BoundedTaskClass::DocsSingleFile)
    } else {
        None
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
    let requested: BTreeSet<String> = notice
        .asset_ids
        .iter()
        .map(|asset_id| asset_id.trim().to_string())
        .filter(|asset_id| !asset_id.is_empty())
        .collect();
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
        sender_id: notice.sender_id.clone(),
        asset_ids: affected_ids,
        reason: notice.reason.clone(),
    })
}

fn evolution_metrics_snapshot(
    store: &dyn EvolutionStore,
) -> Result<EvolutionMetricsSnapshot, EvoKernelError> {
    let (events, projection) = scan_projection(store)?;
    let mut replay_task_class_totals = BTreeMap::<(String, String), (u64, u64, u64, u64)>::new();
    let mut replay_source_totals = BTreeMap::<String, (u64, u64, u64, u64)>::new();
    let replay_evidences = events
        .iter()
        .filter_map(|stored| match &stored.event {
            EvolutionEvent::ReplayEconomicsRecorded { evidence, .. } => Some(evidence.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let (
        replay_success_total,
        replay_failures_total,
        replay_reasoning_avoided_total,
        reasoning_avoided_tokens_total,
        replay_fallback_cost_total,
    ) = if replay_evidences.is_empty() {
        let gene_task_classes = projection
            .genes
            .iter()
            .map(|gene| (gene.id.clone(), replay_task_descriptor(&gene.signals)))
            .collect::<BTreeMap<_, _>>();
        let replay_success_total = events
            .iter()
            .filter(|stored| matches!(stored.event, EvolutionEvent::CapsuleReused { .. }))
            .count() as u64;
        for stored in &events {
            if let EvolutionEvent::CapsuleReused { gene_id, .. } = &stored.event {
                if let Some((task_class_id, task_label)) = gene_task_classes.get(gene_id) {
                    let entry = replay_task_class_totals
                        .entry((task_class_id.clone(), task_label.clone()))
                        .or_insert((0, 0, 0, 0));
                    entry.0 += 1;
                    entry.2 += REPLAY_REASONING_TOKEN_FLOOR;
                }
            }
        }
        let replay_failures_total = events
            .iter()
            .filter(|stored| is_replay_validation_failure(&stored.event))
            .count() as u64;
        (
            replay_success_total,
            replay_failures_total,
            replay_success_total,
            replay_success_total * REPLAY_REASONING_TOKEN_FLOOR,
            replay_failures_total * REPLAY_REASONING_TOKEN_FLOOR,
        )
    } else {
        let mut replay_success_total = 0_u64;
        let mut replay_failures_total = 0_u64;
        let mut reasoning_avoided_tokens_total = 0_u64;
        let mut replay_fallback_cost_total = 0_u64;
        for evidence in &replay_evidences {
            if evidence.success {
                replay_success_total += 1;
            } else {
                replay_failures_total += 1;
            }
            reasoning_avoided_tokens_total += evidence.reasoning_avoided_tokens;
            replay_fallback_cost_total += evidence.replay_fallback_cost;
            let entry = replay_task_class_totals
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
                let source_entry = replay_source_totals
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
            replay_failures_total,
            replay_success_total,
            reasoning_avoided_tokens_total,
            replay_fallback_cost_total,
        )
    };
    let replay_task_classes = replay_task_class_totals
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
    let replay_sources = replay_source_totals
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
    let replay_attempts_total = replay_success_total + replay_failures_total;
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
        replay_attempts_total,
        replay_success_total,
        replay_success_rate: safe_ratio(replay_success_total, replay_attempts_total),
        confidence_revalidations_total,
        replay_reasoning_avoided_total,
        reasoning_avoided_tokens_total,
        replay_fallback_cost_total,
        replay_roi: compute_replay_roi(reasoning_avoided_tokens_total, replay_fallback_cost_total),
        replay_task_classes,
        replay_sources,
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

fn replay_roi_release_gate_summary(
    store: &dyn EvolutionStore,
    window_seconds: u64,
) -> Result<ReplayRoiWindowSummary, EvoKernelError> {
    let events = store.scan(1).map_err(store_err)?;
    let now = Utc::now();
    let cutoff = if window_seconds == 0 {
        None
    } else {
        let seconds = i64::try_from(window_seconds).unwrap_or(i64::MAX);
        Some(now - Duration::seconds(seconds))
    };

    let mut replay_attempts_total = 0_u64;
    let mut replay_success_total = 0_u64;
    let mut replay_failure_total = 0_u64;
    let mut reasoning_avoided_tokens_total = 0_u64;
    let mut replay_fallback_cost_total = 0_u64;
    let mut task_totals = BTreeMap::<(String, String), (u64, u64, u64, u64)>::new();
    let mut source_totals = BTreeMap::<String, (u64, u64, u64, u64)>::new();

    for stored in events {
        let EvolutionEvent::ReplayEconomicsRecorded { evidence, .. } = stored.event else {
            continue;
        };
        if let Some(cutoff) = cutoff {
            let Some(timestamp) = parse_event_timestamp(&stored.timestamp) else {
                continue;
            };
            if timestamp < cutoff {
                continue;
            }
        }
        replay_attempts_total += 1;
        if evidence.success {
            replay_success_total += 1;
        } else {
            replay_failure_total += 1;
        }
        reasoning_avoided_tokens_total += evidence.reasoning_avoided_tokens;
        replay_fallback_cost_total += evidence.replay_fallback_cost;
        let task_entry = task_totals
            .entry((evidence.task_class_id.clone(), evidence.task_label.clone()))
            .or_insert((0, 0, 0, 0));
        if evidence.success {
            task_entry.0 += 1;
        } else {
            task_entry.1 += 1;
        }
        task_entry.2 += evidence.reasoning_avoided_tokens;
        task_entry.3 += evidence.replay_fallback_cost;
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

    Ok(ReplayRoiWindowSummary {
        generated_at: now.to_rfc3339(),
        window_seconds,
        replay_attempts_total,
        replay_success_total,
        replay_failure_total,
        reasoning_avoided_tokens_total,
        replay_fallback_cost_total,
        replay_roi: compute_replay_roi(reasoning_avoided_tokens_total, replay_fallback_cost_total),
        replay_task_classes,
        replay_sources,
    })
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
}
