//! EvoKernel orchestration: mutation capture, validation, capsule construction, and replay-first reuse.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use oris_agent_contract::{ExecutionFeedback, MutationProposal as AgentMutationProposal};
use oris_economics::{EconomicsSignal, EvuLedger, StakePolicy};
use oris_evolution::{
    compute_artifact_hash, next_id, stable_hash_json, AssetState, BlastRadius, CandidateSource,
    Capsule, CapsuleId, EnvFingerprint, EvolutionError, EvolutionEvent, EvolutionProjection,
    EvolutionStore, Gene, GeneCandidate, MutationId, PreparedMutation, Selector, SelectorInput,
    StoreBackedSelector, StoredEvolutionEvent, ValidationSnapshot,
};
use oris_evolution_network::{EvolutionEnvelope, NetworkAsset};
use oris_governor::{DefaultGovernor, Governor, GovernorDecision, GovernorInput};
use oris_kernel::{Kernel, KernelState, RunId};
use oris_sandbox::{
    compute_blast_radius, execute_allowed_command, Sandbox, SandboxPolicy, SandboxReceipt,
};
use oris_spec::CompiledMutationPlan;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use oris_evolution::{
    default_store_root, ArtifactEncoding, AssetState as EvoAssetState,
    BlastRadius as EvoBlastRadius, CandidateSource as EvoCandidateSource,
    EnvFingerprint as EvoEnvFingerprint, EvolutionStore as EvoEvolutionStore, JsonlEvolutionStore,
    MutationArtifact, MutationIntent, MutationTarget, Outcome, RiskLevel,
    SelectorInput as EvoSelectorInput,
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

#[derive(Clone, Debug)]
pub struct ReplayDecision {
    pub used_capsule: bool,
    pub capsule_id: Option<CapsuleId>,
    pub fallback_to_planner: bool,
    pub reason: String,
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

#[async_trait]
impl ReplayExecutor for StoreReplayExecutor {
    async fn try_replay(
        &self,
        input: &SelectorInput,
        policy: &SandboxPolicy,
        validation: &ValidationPlan,
    ) -> Result<ReplayDecision, ReplayError> {
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
        let Some(best) = candidates.into_iter().next() else {
            return Ok(ReplayDecision {
                used_capsule: false,
                capsule_id: None,
                fallback_to_planner: true,
                reason: "no matching gene".into(),
            });
        };
        let remote_publisher = self.publisher_for_gene(&best.gene.id);

        if !exact_match && best.score < 0.82 {
            return Ok(ReplayDecision {
                used_capsule: false,
                capsule_id: None,
                fallback_to_planner: true,
                reason: format!("best gene score {:.3} below replay threshold", best.score),
            });
        }

        let Some(capsule) = best.capsules.first().cloned() else {
            return Ok(ReplayDecision {
                used_capsule: false,
                capsule_id: None,
                fallback_to_planner: true,
                reason: "candidate gene has no capsule".into(),
            });
        };

        let Some(mutation) = find_declared_mutation(self.store.as_ref(), &capsule.mutation_id)
            .map_err(|err| ReplayError::Store(err.to_string()))?
        else {
            return Ok(ReplayDecision {
                used_capsule: false,
                capsule_id: None,
                fallback_to_planner: true,
                reason: "mutation payload missing from store".into(),
            });
        };

        let receipt = match self.sandbox.apply(&mutation, policy).await {
            Ok(receipt) => receipt,
            Err(err) => {
                self.record_reuse_settlement(remote_publisher.as_deref(), false);
                return Ok(ReplayDecision {
                    used_capsule: false,
                    capsule_id: Some(capsule.id.clone()),
                    fallback_to_planner: true,
                    reason: format!("replay patch apply failed: {err}"),
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
            return Ok(ReplayDecision {
                used_capsule: false,
                capsule_id: Some(capsule.id.clone()),
                fallback_to_planner: true,
                reason: "replay validation failed".into(),
            });
        }

        if matches!(capsule.state, AssetState::Quarantined) {
            self.store
                .append_event(EvolutionEvent::ValidationPassed {
                    mutation_id: capsule.mutation_id.clone(),
                    report: report.to_snapshot(&validation.profile),
                    gene_id: Some(best.gene.id.clone()),
                })
                .map_err(|err| ReplayError::Store(err.to_string()))?;
            self.store
                .append_event(EvolutionEvent::CapsuleReleased {
                    capsule_id: capsule.id.clone(),
                    state: AssetState::Promoted,
                })
                .map_err(|err| ReplayError::Store(err.to_string()))?;
        }

        self.store
            .append_event(EvolutionEvent::CapsuleReused {
                capsule_id: capsule.id.clone(),
                gene_id: capsule.gene_id.clone(),
                run_id: capsule.run_id.clone(),
            })
            .map_err(|err| ReplayError::Store(err.to_string()))?;
        self.record_reuse_settlement(remote_publisher.as_deref(), true);

        Ok(ReplayDecision {
            used_capsule: true,
            capsule_id: Some(capsule.id),
            fallback_to_planner: false,
            reason: if exact_match {
                "replayed via exact-match cold-start lookup".into()
            } else {
                "replayed via selector".into()
            },
        })
    }
}

impl StoreReplayExecutor {
    fn rerank_with_reputation_bias(&self, candidates: &mut [GeneCandidate]) {
        let Some(ledger) = self.economics.as_ref() else {
            return;
        };
        let Some(remote_publishers) = self.remote_publishers.as_ref() else {
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
        let publisher_map = remote_publishers
            .lock()
            .ok()
            .map(|locked| locked.clone())
            .unwrap_or_default();
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

    fn publisher_for_gene(&self, gene_id: &str) -> Option<String> {
        self.remote_publishers
            .as_ref()?
            .lock()
            .ok()?
            .get(gene_id)
            .cloned()
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
        let projection = self
            .store
            .rebuild_projection()
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
            candidate_source: if self.publisher_for_gene(&best.gene.id).is_some() {
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
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct EvolutionMetricsSnapshot {
    pub replay_attempts_total: u64,
    pub replay_success_total: u64,
    pub replay_success_rate: f64,
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
        import_remote_envelope_into_store(
            self.store.as_ref(),
            &EvolutionEnvelope::publish(request.sender_id.clone(), request.assets.clone()),
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

        let projection = self.store.rebuild_projection().map_err(store_err)?;
        let blast_radius = compute_blast_radius(&mutation.artifact.payload);
        let recent_mutation_ages_secs = self
            .recent_prior_mutation_ages_secs(Some(mutation.intent.id.as_str()))
            .map_err(store_err)?;
        let mut gene = derive_gene(&mutation, &receipt, &self.validation_plan.profile);
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
            current_confidence: 0.7,
            historical_peak_confidence: 0.7,
            confidence_last_updated_secs: Some(0),
        });

        gene.state = governor_decision.target_state.clone();
        self.store
            .append_event(EvolutionEvent::ValidationPassed {
                mutation_id: mutation.intent.id.clone(),
                report: report.to_snapshot(&self.validation_plan.profile),
                gene_id: Some(gene.id.clone()),
            })
            .map_err(store_err)?;
        self.store
            .append_event(EvolutionEvent::GeneProjected { gene: gene.clone() })
            .map_err(store_err)?;
        self.store
            .append_event(EvolutionEvent::PromotionEvaluated {
                gene_id: gene.id.clone(),
                state: governor_decision.target_state.clone(),
                reason: governor_decision.reason.clone(),
            })
            .map_err(store_err)?;
        if matches!(governor_decision.target_state, AssetState::Promoted) {
            self.store
                .append_event(EvolutionEvent::GenePromoted {
                    gene_id: gene.id.clone(),
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
        let outcome = import_remote_envelope_into_store(self.store.as_ref(), envelope)?;
        self.record_remote_publishers(envelope);
        Ok(outcome)
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
            .try_replay(&input, &self.sandbox_policy, &self.validation_plan)
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

    fn record_remote_publishers(&self, envelope: &EvolutionEnvelope) {
        let sender_id = envelope.sender_id.trim();
        if sender_id.is_empty() {
            return;
        }
        let Ok(mut publishers) = self.remote_publishers.lock() else {
            return;
        };
        for asset in &envelope.assets {
            match asset {
                NetworkAsset::Gene { gene } => {
                    publishers.insert(gene.id.clone(), sender_id.to_string());
                }
                NetworkAsset::Capsule { capsule } => {
                    publishers.insert(capsule.gene_id.clone(), sender_id.to_string());
                }
                NetworkAsset::EvolutionEvent { .. } => {}
            }
        }
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

fn derive_gene(
    mutation: &PreparedMutation,
    receipt: &SandboxReceipt,
    validation_profile: &str,
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
    let id = stable_hash_json(&(&mutation.intent.signals, &strategy, validation_profile))
        .unwrap_or_else(|_| next_id("gene"));
    Gene {
        id,
        signals: mutation.intent.signals.clone(),
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
    let Ok(projection) = store.rebuild_projection() else {
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

    let Ok(projection) = store.rebuild_projection() else {
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
                        capsule.gene_id == gene.id
                            && capsule.state == AssetState::Quarantined
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
    publishers_by_gene: &BTreeMap<String, String>,
    reputation_bias: &BTreeMap<String, f32>,
) -> f32 {
    let bias = publishers_by_gene
        .get(&candidate.gene.id)
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
    let projection = store.rebuild_projection().map_err(store_err)?;
    let mut assets = Vec::new();
    for gene in projection
        .genes
        .into_iter()
        .filter(|gene| gene.state == AssetState::Promoted)
    {
        assets.push(NetworkAsset::Gene { gene });
    }
    for capsule in projection
        .capsules
        .into_iter()
        .filter(|capsule| capsule.state == AssetState::Promoted)
    {
        assets.push(NetworkAsset::Capsule { capsule });
    }
    Ok(EvolutionEnvelope::publish(sender_id, assets))
}

fn import_remote_envelope_into_store(
    store: &dyn EvolutionStore,
    envelope: &EvolutionEnvelope,
) -> Result<ImportOutcome, EvoKernelError> {
    if !envelope.verify_content_hash() {
        return Err(EvoKernelError::Validation(
            "invalid evolution envelope hash".into(),
        ));
    }

    let mut imported_asset_ids = Vec::new();
    for asset in &envelope.assets {
        match asset {
            NetworkAsset::Gene { gene } => {
                imported_asset_ids.push(gene.id.clone());
                store
                    .append_event(EvolutionEvent::RemoteAssetImported {
                        source: CandidateSource::Remote,
                        asset_ids: vec![gene.id.clone()],
                    })
                    .map_err(store_err)?;
                store
                    .append_event(EvolutionEvent::GeneProjected { gene: gene.clone() })
                    .map_err(store_err)?;
            }
            NetworkAsset::Capsule { capsule } => {
                imported_asset_ids.push(capsule.id.clone());
                store
                    .append_event(EvolutionEvent::RemoteAssetImported {
                        source: CandidateSource::Remote,
                        asset_ids: vec![capsule.id.clone()],
                    })
                    .map_err(store_err)?;
                let mut quarantined = capsule.clone();
                quarantined.state = AssetState::Quarantined;
                store
                    .append_event(EvolutionEvent::CapsuleCommitted {
                        capsule: quarantined.clone(),
                    })
                    .map_err(store_err)?;
                store
                    .append_event(EvolutionEvent::CapsuleQuarantined {
                        capsule_id: quarantined.id,
                    })
                    .map_err(store_err)?;
            }
            NetworkAsset::EvolutionEvent { event } => {
                if should_import_remote_event(event) {
                    store.append_event(event.clone()).map_err(store_err)?;
                }
            }
        }
    }

    Ok(ImportOutcome {
        imported_asset_ids,
        accepted: true,
    })
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
    let projection = store.rebuild_projection().map_err(store_err)?;
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

    let mut assets = Vec::new();
    for gene in matched_genes {
        assets.push(NetworkAsset::Gene { gene });
    }
    for capsule in matched_capsules {
        assets.push(NetworkAsset::Capsule { capsule });
    }

    Ok(FetchResponse {
        sender_id: responder_id.into(),
        assets,
    })
}

fn revoke_assets_in_store(
    store: &dyn EvolutionStore,
    notice: &RevokeNotice,
) -> Result<RevokeNotice, EvoKernelError> {
    let projection = store.rebuild_projection().map_err(store_err)?;
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
    let events = store.scan(1).map_err(store_err)?;
    let projection = store.rebuild_projection().map_err(store_err)?;
    let replay_success_total = events
        .iter()
        .filter(|stored| matches!(stored.event, EvolutionEvent::CapsuleReused { .. }))
        .count() as u64;
    let replay_failures_total = events
        .iter()
        .filter(|stored| is_replay_validation_failure(&stored.event))
        .count() as u64;
    let replay_attempts_total = replay_success_total + replay_failures_total;
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
    out.push_str("# HELP oris_evolution_replay_success_rate Successful replay attempts divided by replay attempts that reached validation.\n");
    out.push_str("# TYPE oris_evolution_replay_success_rate gauge\n");
    out.push_str(&format!(
        "oris_evolution_replay_success_rate {:.6}\n",
        snapshot.replay_success_rate
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
        SelectorInput {
            signals: vec![signal.into()],
            env: EnvFingerprint {
                rustc_version: "rustc".into(),
                cargo_lock_hash: "lock".into(),
                target_triple: "x86_64-unknown-linux-gnu".into(),
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
        let decision = evo
            .replay_or_fallback(replay_input("missing readme"))
            .await
            .unwrap();
        assert!(decision.used_capsule);
        assert_eq!(decision.capsule_id, Some(capsule.id));
        assert!(store
            .scan(1)
            .unwrap()
            .iter()
            .any(|stored| matches!(stored.event, EvolutionEvent::CapsuleReused { .. })));
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
        assert_eq!(snapshot.mutation_declared_total, 1);
        assert_eq!(snapshot.promoted_mutations_total, 1);
        assert_eq!(snapshot.promotion_ratio, 1.0);
        assert_eq!(snapshot.gene_revocations_total, 1);
        assert_eq!(snapshot.mutation_velocity_last_hour, 1);
        assert_eq!(snapshot.revoke_frequency_last_hour, 1);
        assert_eq!(snapshot.promoted_genes, 0);
        assert_eq!(snapshot.promoted_capsules, 0);

        let rendered = evo.render_metrics_prometheus().unwrap();
        assert!(rendered.contains("oris_evolution_replay_success_rate 1.000000"));
        assert!(rendered.contains("oris_evolution_promotion_ratio 1.000000"));
        assert!(rendered.contains("oris_evolution_revoke_frequency_last_hour 1"));
        assert!(rendered.contains("oris_evolution_mutation_velocity_last_hour 1"));
        assert!(rendered.contains("oris_evolution_health 1"));
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

    #[tokio::test]
    async fn remote_capsule_stays_quarantined_until_first_successful_replay() {
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
        let imported_capsule = before_replay
            .capsules
            .iter()
            .find(|capsule| capsule.id == "capsule-remote")
            .unwrap();
        assert_eq!(imported_capsule.state, AssetState::Quarantined);

        let decision = evo
            .replay_or_fallback(replay_input("remote-signal"))
            .await
            .unwrap();

        assert!(decision.used_capsule);
        assert_eq!(decision.capsule_id, Some("capsule-remote".into()));

        let after_replay = store.rebuild_projection().unwrap();
        let released_capsule = after_replay
            .capsules
            .iter()
            .find(|capsule| capsule.id == "capsule-remote")
            .unwrap();
        assert_eq!(released_capsule.state, AssetState::Promoted);
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
        assert_eq!(gene.state, AssetState::Revoked);
        let committed_capsule = projection
            .capsules
            .iter()
            .find(|current| current.id == capsule.id)
            .unwrap();
        assert_eq!(committed_capsule.state, AssetState::Quarantined);

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
            2
        );
        assert!(events.iter().any(|stored| {
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
        assert_eq!(after_revoke.reason, "no matching gene");
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
}
