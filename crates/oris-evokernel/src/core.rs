//! EvoKernel orchestration: mutation capture, validation, capsule construction, and replay-first reuse.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use async_trait::async_trait;
use oris_agent_contract::{ExecutionFeedback, MutationProposal as AgentMutationProposal};
use oris_evolution::{
    compute_artifact_hash, next_id, stable_hash_json, AssetState, BlastRadius, CandidateSource,
    Capsule, CapsuleId, EnvFingerprint, EvolutionError, EvolutionEvent, EvolutionStore, Gene,
    GeneCandidate, MutationId, PreparedMutation, Selector, SelectorInput, StoreBackedSelector,
    ValidationSnapshot,
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
}

#[async_trait]
impl ReplayExecutor for StoreReplayExecutor {
    async fn try_replay(
        &self,
        input: &SelectorInput,
        policy: &SandboxPolicy,
        validation: &ValidationPlan,
    ) -> Result<ReplayDecision, ReplayError> {
        let mut candidates = self.selector.select(input);
        let mut exact_match = false;
        if candidates.is_empty() {
            if let Some(candidate) = exact_match_candidate(self.store.as_ref(), input) {
                candidates.push(candidate);
                exact_match = true;
            }
        }
        let Some(best) = candidates.into_iter().next() else {
            return Ok(ReplayDecision {
                used_capsule: false,
                capsule_id: None,
                fallback_to_planner: true,
                reason: "no matching gene".into(),
            });
        };

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
                return Ok(ReplayDecision {
                    used_capsule: false,
                    capsule_id: Some(capsule.id.clone()),
                    fallback_to_planner: true,
                    reason: format!("replay patch apply failed: {err}"),
                })
            }
        };

        let report = self
            .validator
            .run(&receipt, validation)
            .await
            .map_err(|err| ReplayError::Validation(err.to_string()))?;
        if !report.success {
            return Ok(ReplayDecision {
                used_capsule: false,
                capsule_id: Some(capsule.id.clone()),
                fallback_to_planner: true,
                reason: "replay validation failed".into(),
            });
        }

        self.store
            .append_event(EvolutionEvent::CapsuleReused {
                capsule_id: capsule.id.clone(),
                gene_id: capsule.gene_id.clone(),
                run_id: capsule.run_id.clone(),
            })
            .map_err(|err| ReplayError::Store(err.to_string()))?;

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
}

pub struct EvoKernel<S: KernelState> {
    pub kernel: Arc<Kernel<S>>,
    pub sandbox: Arc<dyn Sandbox>,
    pub validator: Arc<dyn Validator>,
    pub store: Arc<dyn EvolutionStore>,
    pub selector: Arc<dyn Selector>,
    pub governor: Arc<dyn Governor>,
    pub sandbox_policy: SandboxPolicy,
    pub validation_plan: ValidationPlan,
}

impl<S: KernelState> EvoKernel<S> {
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
        let success_count = projection
            .genes
            .iter()
            .find(|gene| {
                gene.id == derive_gene(&mutation, &receipt, &self.validation_plan.profile).id
            })
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
        });

        let mut gene = derive_gene(&mutation, &receipt, &self.validation_plan.profile);
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
        export_promoted_assets_from_store(self.store.as_ref(), sender_id)
    }

    pub fn import_remote_envelope(
        &self,
        envelope: &EvolutionEnvelope,
    ) -> Result<ImportOutcome, EvoKernelError> {
        import_remote_envelope_into_store(self.store.as_ref(), envelope)
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
        };
        executor
            .try_replay(&input, &self.sandbox_policy, &self.validation_plan)
            .await
            .map_err(|err| EvoKernelError::Validation(err.to_string()))
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

fn exact_match_candidate(
    store: &dyn EvolutionStore,
    input: &SelectorInput,
) -> Option<GeneCandidate> {
    let projection = store.rebuild_projection().ok()?;
    let capsules = projection.capsules.clone();
    let signal_set = input
        .signals
        .iter()
        .map(|signal| signal.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    projection.genes.into_iter().find_map(|gene| {
        if gene.state != AssetState::Promoted {
            return None;
        }
        let gene_signals = gene
            .signals
            .iter()
            .map(|signal| signal.to_ascii_lowercase())
            .collect::<BTreeSet<_>>();
        if gene_signals == signal_set {
            let matched_capsules = capsules
                .iter()
                .filter(|capsule| {
                    capsule.gene_id == gene.id && capsule.state == AssetState::Promoted
                })
                .cloned()
                .collect::<Vec<_>>();
            if matched_capsules.is_empty() {
                None
            } else {
                Some(GeneCandidate {
                    gene,
                    score: 1.0,
                    capsules: matched_capsules,
                })
            }
        } else {
            None
        }
    })
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
                store.append_event(event.clone()).map_err(store_err)?;
            }
        }
    }

    Ok(ImportOutcome {
        imported_asset_ids,
        accepted: true,
    })
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
        let workspace = temp_workspace("capture");
        let store_root =
            std::env::temp_dir().join(format!("oris-evokernel-store-{}", std::process::id()));
        if store_root.exists() {
            fs::remove_dir_all(&store_root).unwrap();
        }
        let store: Arc<dyn EvolutionStore> =
            Arc::new(oris_evolution::JsonlEvolutionStore::new(&store_root));
        let sandbox: Arc<dyn Sandbox> = Arc::new(oris_sandbox::LocalProcessSandbox::new(
            "run-1",
            &workspace,
            std::env::temp_dir(),
        ));
        let validator: Arc<dyn Validator> = Arc::new(CommandValidator::new(SandboxPolicy {
            allowed_programs: vec!["git".into()],
            max_duration_ms: 60_000,
            max_output_bytes: 1024 * 1024,
            denied_env_prefixes: Vec::new(),
        }));
        let evo = EvoKernel::new(test_kernel(), sandbox, validator, store.clone())
            .with_governor(Arc::new(DefaultGovernor::new(
                oris_governor::GovernorConfig {
                    promote_after_successes: 1,
                    ..Default::default()
                },
            )))
            .with_validation_plan(lightweight_plan())
            .with_sandbox_policy(SandboxPolicy {
                allowed_programs: vec!["git".into()],
                max_duration_ms: 60_000,
                max_output_bytes: 1024 * 1024,
                denied_env_prefixes: Vec::new(),
            });
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
        let workspace = temp_workspace("replay");
        let store_root =
            std::env::temp_dir().join(format!("oris-evokernel-replay-{}", std::process::id()));
        if store_root.exists() {
            fs::remove_dir_all(&store_root).unwrap();
        }
        let store: Arc<dyn EvolutionStore> =
            Arc::new(oris_evolution::JsonlEvolutionStore::new(&store_root));
        let sandbox: Arc<dyn Sandbox> = Arc::new(oris_sandbox::LocalProcessSandbox::new(
            "run-2",
            &workspace,
            std::env::temp_dir(),
        ));
        let validator: Arc<dyn Validator> = Arc::new(CommandValidator::new(SandboxPolicy {
            allowed_programs: vec!["git".into()],
            max_duration_ms: 60_000,
            max_output_bytes: 1024 * 1024,
            denied_env_prefixes: Vec::new(),
        }));
        let evo = EvoKernel::new(test_kernel(), sandbox, validator, store.clone())
            .with_governor(Arc::new(DefaultGovernor::new(
                oris_governor::GovernorConfig {
                    promote_after_successes: 1,
                    ..Default::default()
                },
            )))
            .with_validation_plan(lightweight_plan())
            .with_sandbox_policy(SandboxPolicy {
                allowed_programs: vec!["git".into()],
                max_duration_ms: 60_000,
                max_output_bytes: 1024 * 1024,
                denied_env_prefixes: Vec::new(),
            });
        let capsule = evo
            .capture_successful_mutation(&"run-2".into(), sample_mutation())
            .await
            .unwrap();
        let decision = evo
            .replay_or_fallback(SelectorInput {
                signals: vec!["missing readme".into()],
                env: EnvFingerprint {
                    rustc_version: "rustc".into(),
                    cargo_lock_hash: "lock".into(),
                    target_triple: "x86_64-unknown-linux-gnu".into(),
                    os: std::env::consts::OS.into(),
                },
                limit: 1,
            })
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
}
