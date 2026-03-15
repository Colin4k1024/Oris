//! Evolution Pipeline - Complete detect/select/mutate runtime pipeline.
//!
//! This module implements the full evolution loop as separate runtime stages:
//! - Detect: Extract signals from task context
//! - Select: Choose gene candidates based on signals
//! - Mutate: Prepare mutation proposals
//! - Execute: Run the mutation in sandbox
//! - Validate: Verify mutation correctness
//! - Evaluate: Assess mutation quality
//! - Solidify: Create gene/capsule events
//! - Reuse: Mark capsule as reusable

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;

use crate::core::{GeneCandidate, PreparedMutation, Selector, SelectorInput};
use crate::evolver::{EvolutionSignal, MutationProposal, MutationRiskLevel, ValidationResult};
use crate::port::{
    EvaluateInput, EvaluatePort, GeneStorePersistPort, SandboxPort, SignalExtractorInput,
    SignalExtractorPort, ValidateInput, ValidatePort,
};

/// Pipeline configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EvolutionPipelineConfig {
    /// Enable/disable specific stages
    pub enable_detect: bool,
    pub enable_select: bool,
    pub enable_mutate: bool,
    pub enable_execute: bool,
    pub enable_validate: bool,
    pub enable_evaluate: bool,
    pub enable_solidify: bool,
    pub enable_reuse: bool,

    /// Stage timeout in seconds
    pub detect_timeout_secs: u64,
    pub select_timeout_secs: u64,
    pub mutate_timeout_secs: u64,
    pub execute_timeout_secs: u64,
    pub validate_timeout_secs: u64,
    pub evaluate_timeout_secs: u64,
    pub solidify_timeout_secs: u64,
    pub reuse_timeout_secs: u64,

    /// Max candidates to select
    pub max_candidates: usize,

    /// Min signal confidence threshold
    pub min_signal_confidence: f32,
}

impl Default for EvolutionPipelineConfig {
    fn default() -> Self {
        Self {
            enable_detect: true,
            enable_select: true,
            enable_mutate: true,
            enable_execute: true,
            enable_validate: true,
            enable_evaluate: true,
            enable_solidify: true,
            enable_reuse: true,
            detect_timeout_secs: 30,
            select_timeout_secs: 30,
            mutate_timeout_secs: 60,
            execute_timeout_secs: 300,
            validate_timeout_secs: 60,
            evaluate_timeout_secs: 30,
            solidify_timeout_secs: 30,
            reuse_timeout_secs: 30,
            max_candidates: 10,
            min_signal_confidence: 0.5,
        }
    }
}

/// Stage state
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum PipelineStageState {
    /// Stage not started
    Pending,
    /// Stage currently running
    Running,
    /// Stage completed successfully
    Completed,
    /// Stage failed with error
    Failed(String),
    /// Stage was skipped
    Skipped(String),
}

/// Pipeline execution context (internal use, not serialized)
pub struct PipelineContext {
    /// Input task context
    pub task_input: serde_json::Value,
    /// Optional extractor input for the Detect stage.
    /// When set and a `SignalExtractorPort` is injected into the pipeline,
    /// the Detect stage will call the extractor to populate `signals`.
    pub extractor_input: Option<SignalExtractorInput>,
    /// Signals extracted in Detect phase
    pub signals: Vec<EvolutionSignal>,
    /// Gene candidates selected in Select phase
    pub candidates: Vec<GeneCandidate>,
    /// Mutation proposals prepared in Mutate phase
    pub proposals: Vec<MutationProposal>,
    /// Execution result
    pub execution_result: Option<serde_json::Value>,
    /// Validation result
    pub validation_result: Option<ValidationResult>,
    /// Evaluation result
    pub evaluation_result: Option<EvaluationResult>,
    /// Solidified genes
    pub solidified_genes: Vec<String>,
    /// Reused capsules
    pub reused_capsules: Vec<String>,
    /// Wall-clock duration recorded for each stage that ran.
    pub stage_timings: HashMap<String, Duration>,
}

impl Default for PipelineContext {
    fn default() -> Self {
        Self {
            task_input: serde_json::json!({}),
            extractor_input: None,
            signals: Vec::new(),
            candidates: Vec::new(),
            proposals: Vec::new(),
            execution_result: None,
            validation_result: None,
            evaluation_result: None,
            solidified_genes: Vec::new(),
            reused_capsules: Vec::new(),
            stage_timings: HashMap::new(),
        }
    }
}

/// Evaluation result
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EvaluationResult {
    pub score: f32,
    pub improvements: Vec<String>,
    pub regressions: Vec<String>,
    pub recommendation: EvaluationRecommendation,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum EvaluationRecommendation {
    Accept,
    Reject,
    NeedsRevision,
    RequiresHumanReview,
}

/// Pipeline execution result
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PipelineResult {
    /// Whether the pipeline completed successfully
    pub success: bool,
    /// Stage states
    pub stage_states: Vec<StageState>,
    /// Error message if failed
    pub error: Option<String>,
}

/// Individual stage state
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StageState {
    pub stage_name: String,
    pub state: PipelineStageState,
    pub duration_ms: Option<u64>,
}

impl StageState {
    pub fn new(name: &str) -> Self {
        Self {
            stage_name: name.to_string(),
            state: PipelineStageState::Pending,
            duration_ms: None,
        }
    }
}

/// Pipeline errors
#[derive(Error, Debug)]
pub enum PipelineError {
    #[error("Detect stage error: {0}")]
    DetectError(String),

    #[error("Select stage error: {0}")]
    SelectError(String),

    #[error("Mutate stage error: {0}")]
    MutateError(String),

    #[error("Execute stage error: {0}")]
    ExecuteError(String),

    #[error("Validate stage error: {0}")]
    ValidateError(String),

    #[error("Evaluate stage error: {0}")]
    EvaluateError(String),

    #[error("Solidify stage error: {0}")]
    SolidifyError(String),

    #[error("Reuse stage error: {0}")]
    ReuseError(String),

    #[error("Pipeline timeout: {0}")]
    Timeout(String),
}

/// Evolution Pipeline trait
pub trait EvolutionPipeline: Send + Sync {
    /// Get pipeline name
    fn name(&self) -> &str;

    /// Get pipeline configuration
    fn config(&self) -> &EvolutionPipelineConfig;

    /// Execute the full pipeline
    fn execute(&self, context: PipelineContext) -> Result<PipelineResult, PipelineError>;

    /// Execute a specific stage
    fn execute_stage(
        &self,
        stage: PipelineStage,
        context: &mut PipelineContext,
    ) -> Result<PipelineStageState, PipelineError>;
}

/// Pipeline stages
#[derive(Clone, Debug, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PipelineStage {
    Detect,
    Select,
    Mutate,
    Execute,
    Validate,
    Evaluate,
    Solidify,
    Reuse,
}

impl PipelineStage {
    pub fn as_str(&self) -> &'static str {
        match self {
            PipelineStage::Detect => "detect",
            PipelineStage::Select => "select",
            PipelineStage::Mutate => "mutate",
            PipelineStage::Execute => "execute",
            PipelineStage::Validate => "validate",
            PipelineStage::Evaluate => "evaluate",
            PipelineStage::Solidify => "solidify",
            PipelineStage::Reuse => "reuse",
        }
    }

    pub fn all() -> Vec<PipelineStage> {
        vec![
            PipelineStage::Detect,
            PipelineStage::Select,
            PipelineStage::Mutate,
            PipelineStage::Execute,
            PipelineStage::Validate,
            PipelineStage::Evaluate,
            PipelineStage::Solidify,
            PipelineStage::Reuse,
        ]
    }
}

/// Standard evolution pipeline implementation
pub struct StandardEvolutionPipeline {
    name: String,
    config: EvolutionPipelineConfig,
    selector: Arc<dyn Selector>,
    /// Optional signal extractor for the Detect stage.
    signal_extractor: Option<Arc<dyn SignalExtractorPort>>,
    /// Optional sandbox for the Execute stage.
    sandbox: Option<Arc<dyn SandboxPort>>,
    /// Optional gene store for the Solidify/Reuse stages.
    gene_store: Option<Arc<dyn GeneStorePersistPort>>,
    /// Optional validator for the Validate stage.
    validate_port: Option<Arc<dyn ValidatePort>>,
    /// Optional evaluator for the Evaluate stage.
    evaluate_port: Option<Arc<dyn EvaluatePort>>,
}

impl StandardEvolutionPipeline {
    /// Create a pipeline with a mandatory selector and no injected ports.
    /// Detect will pass through pre-populated signals; Execute will use a
    /// no-op stub that records a synthetic success result.
    pub fn new(config: EvolutionPipelineConfig, selector: Arc<dyn Selector>) -> Self {
        Self {
            name: "standard".to_string(),
            config,
            selector,
            signal_extractor: None,
            sandbox: None,
            gene_store: None,
            validate_port: None,
            evaluate_port: None,
        }
    }

    /// Attach a `SignalExtractorPort` for the Detect stage.
    pub fn with_signal_extractor(mut self, extractor: Arc<dyn SignalExtractorPort>) -> Self {
        self.signal_extractor = Some(extractor);
        self
    }

    /// Attach a `SandboxPort` for the Execute stage.
    pub fn with_sandbox(mut self, sandbox: Arc<dyn SandboxPort>) -> Self {
        self.sandbox = Some(sandbox);
        self
    }

    /// Attach a `GeneStorePersistPort` for the Solidify/Reuse stages.
    pub fn with_gene_store(mut self, gene_store: Arc<dyn GeneStorePersistPort>) -> Self {
        self.gene_store = Some(gene_store);
        self
    }

    /// Attach a `ValidatePort` for the Validate stage.
    pub fn with_validate_port(mut self, validator: Arc<dyn ValidatePort>) -> Self {
        self.validate_port = Some(validator);
        self
    }

    /// Attach an `EvaluatePort` for the Evaluate stage.
    pub fn with_evaluate_port(mut self, evaluator: Arc<dyn EvaluatePort>) -> Self {
        self.evaluate_port = Some(evaluator);
        self
    }
}

impl EvolutionPipeline for StandardEvolutionPipeline {
    fn name(&self) -> &str {
        &self.name
    }

    fn config(&self) -> &EvolutionPipelineConfig {
        &self.config
    }

    fn execute(&self, mut context: PipelineContext) -> Result<PipelineResult, PipelineError> {
        let mut stage_states = Vec::new();

        // Detect phase
        if self.config.enable_detect {
            let mut stage = StageState::new(PipelineStage::Detect.as_str());
            stage.state = PipelineStageState::Running;
            stage_states.push(stage);

            let t0 = Instant::now();
            if let Some(ref extractor) = self.signal_extractor {
                // Use the injected extractor to populate signals from raw input.
                let input = context.extractor_input.clone().unwrap_or_default();
                let extracted = extractor.extract(&input);
                // Merge: keep any pre-populated signals and append new ones.
                context.signals.extend(extracted);
            }
            // When no extractor is injected, signals already in context are
            // used as-is (pass-through, backward-compatible behaviour).
            let elapsed = t0.elapsed();
            context
                .stage_timings
                .insert(PipelineStage::Detect.as_str().to_string(), elapsed);
            let d_ms = elapsed.as_millis() as u64;
            let last = stage_states.last_mut().unwrap();
            last.state = PipelineStageState::Completed;
            last.duration_ms = Some(d_ms);
        } else {
            stage_states.push(StageState {
                stage_name: PipelineStage::Detect.as_str().to_string(),
                state: PipelineStageState::Skipped("disabled".to_string()),
                duration_ms: None,
            });
        }

        // Select phase
        if self.config.enable_select {
            let mut stage = StageState::new(PipelineStage::Select.as_str());
            stage.state = PipelineStageState::Running;
            stage_states.push(stage);

            let t0 = Instant::now();
            let input = SelectorInput {
                signals: context
                    .signals
                    .iter()
                    .map(|s| s.description.clone())
                    .collect(),
                env: crate::core::EnvFingerprint {
                    rustc_version: String::new(),
                    cargo_lock_hash: String::new(),
                    target_triple: String::new(),
                    os: String::new(),
                },
                spec_id: None,
                limit: self.config.max_candidates,
            };

            context.candidates = self.selector.select(&input);
            let elapsed = t0.elapsed();
            context
                .stage_timings
                .insert(PipelineStage::Select.as_str().to_string(), elapsed);
            let last = stage_states.last_mut().unwrap();
            last.state = PipelineStageState::Completed;
            last.duration_ms = Some(elapsed.as_millis() as u64);
        } else {
            stage_states.push(StageState {
                stage_name: PipelineStage::Select.as_str().to_string(),
                state: PipelineStageState::Skipped("disabled".to_string()),
                duration_ms: None,
            });
        }

        // Mutate phase - prepare proposals from candidates
        if self.config.enable_mutate {
            let mut stage = StageState::new(PipelineStage::Mutate.as_str());
            stage.state = PipelineStageState::Running;
            stage_states.push(stage);

            let t0 = Instant::now();
            context.proposals = context
                .candidates
                .iter()
                .enumerate()
                .map(|(i, candidate)| MutationProposal {
                    proposal_id: format!("proposal_{}", i),
                    signal_ids: vec![],
                    gene_id: candidate.gene.id.clone(),
                    description: format!("Mutation for gene {}", candidate.gene.id),
                    estimated_impact: candidate.score,
                    risk_level: MutationRiskLevel::Medium,
                    proposed_changes: serde_json::json!({}),
                })
                .collect();
            let elapsed = t0.elapsed();
            context
                .stage_timings
                .insert(PipelineStage::Mutate.as_str().to_string(), elapsed);
            let last = stage_states.last_mut().unwrap();
            last.state = PipelineStageState::Completed;
            last.duration_ms = Some(elapsed.as_millis() as u64);
        } else {
            stage_states.push(StageState {
                stage_name: PipelineStage::Mutate.as_str().to_string(),
                state: PipelineStageState::Skipped("disabled".to_string()),
                duration_ms: None,
            });
        }

        // Execute phase
        if self.config.enable_execute {
            let mut stage = StageState::new(PipelineStage::Execute.as_str());
            stage.state = PipelineStageState::Running;
            stage_states.push(stage);

            let t0 = Instant::now();
            if let (Some(ref sb), Some(proposal)) = (&self.sandbox, context.proposals.first()) {
                // Build a minimal PreparedMutation from the first proposal so
                // the sandbox can apply the change in an isolated workspace.
                let mutation = build_prepared_mutation(proposal);
                let result = sb.execute(&mutation);
                context.execution_result = Some(result.to_json());
                let last = stage_states.last_mut().unwrap();
                last.state = if result.success {
                    PipelineStageState::Completed
                } else {
                    PipelineStageState::Failed(result.message.clone())
                };
            } else {
                // No sandbox injected — fall back to stub (backward-compatible).
                context.execution_result = Some(serde_json::json!({
                    "success": true,
                    "stdout": "",
                    "stderr": "",
                    "duration_ms": 0,
                    "message": "Mutation executed successfully (stub)"
                }));
                stage_states.last_mut().unwrap().state = PipelineStageState::Completed;
            }
            let elapsed = t0.elapsed();
            context
                .stage_timings
                .insert(PipelineStage::Execute.as_str().to_string(), elapsed);
            stage_states.last_mut().unwrap().duration_ms = Some(elapsed.as_millis() as u64);
        } else {
            stage_states.push(StageState {
                stage_name: PipelineStage::Execute.as_str().to_string(),
                state: PipelineStageState::Skipped("disabled".to_string()),
                duration_ms: None,
            });
        }

        // Validate phase
        if self.config.enable_validate {
            let mut stage = StageState::new(PipelineStage::Validate.as_str());
            stage.state = PipelineStageState::Running;
            stage_states.push(stage);

            let t0 = Instant::now();
            if let Some(proposal) = context.proposals.first() {
                let vresult = if let Some(ref vp) = self.validate_port {
                    // Build input from execution result stored in context.
                    let exec = context.execution_result.as_ref();
                    let exec_success = exec
                        .and_then(|v| v.get("success"))
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let stdout = exec
                        .and_then(|v| v.get("stdout"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let stderr = exec
                        .and_then(|v| v.get("stderr"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let input = ValidateInput {
                        proposal_id: proposal.proposal_id.clone(),
                        execution_success: exec_success,
                        stdout,
                        stderr,
                    };
                    vp.validate(&input)
                } else {
                    // Backward-compatible stub when no validator is injected.
                    ValidationResult {
                        proposal_id: proposal.proposal_id.clone(),
                        passed: true,
                        score: 0.8,
                        issues: vec![],
                        simulation_results: None,
                    }
                };
                context.validation_result = Some(vresult);
            }
            let elapsed = t0.elapsed();
            context
                .stage_timings
                .insert(PipelineStage::Validate.as_str().to_string(), elapsed);
            let last = stage_states.last_mut().unwrap();
            // Mark stage Failed when validation did not pass so callers can detect it.
            last.state = match &context.validation_result {
                Some(r) if !r.passed => PipelineStageState::Failed("validation failed".to_string()),
                _ => PipelineStageState::Completed,
            };
            last.duration_ms = Some(elapsed.as_millis() as u64);
        } else {
            stage_states.push(StageState {
                stage_name: PipelineStage::Validate.as_str().to_string(),
                state: PipelineStageState::Skipped("disabled".to_string()),
                duration_ms: None,
            });
        }

        // Evaluate phase
        if self.config.enable_evaluate {
            let mut stage = StageState::new(PipelineStage::Evaluate.as_str());
            stage.state = PipelineStageState::Running;
            stage_states.push(stage);

            let t0 = Instant::now();
            context.evaluation_result = if let Some(ref ep) = self.evaluate_port {
                if let Some(proposal) = context.proposals.first() {
                    let input = EvaluateInput {
                        proposal_id: proposal.proposal_id.clone(),
                        intent: proposal.description.clone(),
                        original: String::new(),
                        proposed: String::new(),
                        signals: context
                            .signals
                            .iter()
                            .map(|s| s.description.clone())
                            .collect(),
                    };
                    Some(ep.evaluate(&input))
                } else {
                    None
                }
            } else {
                // Backward-compatible stub when no evaluator is injected.
                Some(EvaluationResult {
                    score: 0.8,
                    improvements: vec!["Mutation applied successfully".to_string()],
                    regressions: vec![],
                    recommendation: EvaluationRecommendation::Accept,
                })
            };
            let elapsed = t0.elapsed();
            context
                .stage_timings
                .insert(PipelineStage::Evaluate.as_str().to_string(), elapsed);
            let last = stage_states.last_mut().unwrap();
            last.state = PipelineStageState::Completed;
            last.duration_ms = Some(elapsed.as_millis() as u64);
        } else {
            stage_states.push(StageState {
                stage_name: PipelineStage::Evaluate.as_str().to_string(),
                state: PipelineStageState::Skipped("disabled".to_string()),
                duration_ms: None,
            });
        }

        // Solidify phase - persist promoted genes via the GeneStorePersistPort
        if self.config.enable_solidify {
            let mut stage = StageState::new(PipelineStage::Solidify.as_str());
            stage.state = PipelineStageState::Running;
            stage_states.push(stage);

            let t0 = Instant::now();
            let mut solidified: Vec<String> = Vec::new();
            for candidate in &context.candidates {
                let gene = &candidate.gene;
                // Persist via injected port when available.
                if let Some(ref store) = self.gene_store {
                    store.persist_gene(&gene.id, &gene.signals, &gene.strategy, &gene.validation);
                }
                solidified.push(gene.id.clone());
            }
            context.solidified_genes = solidified;
            let elapsed = t0.elapsed();
            context
                .stage_timings
                .insert(PipelineStage::Solidify.as_str().to_string(), elapsed);
            let last = stage_states.last_mut().unwrap();
            last.state = PipelineStageState::Completed;
            last.duration_ms = Some(elapsed.as_millis() as u64);
        } else {
            stage_states.push(StageState {
                stage_name: PipelineStage::Solidify.as_str().to_string(),
                state: PipelineStageState::Skipped("disabled".to_string()),
                duration_ms: None,
            });
        }

        // Reuse phase - record capsule reuse via the GeneStorePersistPort
        if self.config.enable_reuse {
            let mut stage = StageState::new(PipelineStage::Reuse.as_str());
            stage.state = PipelineStageState::Running;
            stage_states.push(stage);

            let t0 = Instant::now();
            let mut reused: Vec<String> = Vec::new();
            for candidate in &context.candidates {
                let cap_ids: Vec<String> =
                    candidate.capsules.iter().map(|c| c.id.clone()).collect();
                if let Some(ref store) = self.gene_store {
                    store.mark_reused(&candidate.gene.id, &cap_ids);
                }
                reused.extend(cap_ids);
            }
            context.reused_capsules = reused;
            let elapsed = t0.elapsed();
            context
                .stage_timings
                .insert(PipelineStage::Reuse.as_str().to_string(), elapsed);
            let last = stage_states.last_mut().unwrap();
            last.state = PipelineStageState::Completed;
            last.duration_ms = Some(elapsed.as_millis() as u64);
        } else {
            stage_states.push(StageState {
                stage_name: PipelineStage::Reuse.as_str().to_string(),
                state: PipelineStageState::Skipped("disabled".to_string()),
                duration_ms: None,
            });
        }

        // Propagate validation failure to the overall pipeline result.
        let validation_passed = context
            .validation_result
            .as_ref()
            .map(|r| r.passed)
            .unwrap_or(true);

        Ok(PipelineResult {
            success: validation_passed,
            stage_states,
            error: if validation_passed {
                None
            } else {
                Some("Validation stage did not pass".to_string())
            },
        })
    }

    fn execute_stage(
        &self,
        stage: PipelineStage,
        context: &mut PipelineContext,
    ) -> Result<PipelineStageState, PipelineError> {
        match stage {
            PipelineStage::Detect => {
                // Signals already in context
                Ok(PipelineStageState::Completed)
            }
            PipelineStage::Select => {
                let input = SelectorInput {
                    signals: context
                        .signals
                        .iter()
                        .map(|s| s.description.clone())
                        .collect(),
                    env: crate::core::EnvFingerprint {
                        rustc_version: String::new(),
                        cargo_lock_hash: String::new(),
                        target_triple: String::new(),
                        os: String::new(),
                    },
                    spec_id: None,
                    limit: self.config.max_candidates,
                };
                context.candidates = self.selector.select(&input);
                Ok(PipelineStageState::Completed)
            }
            PipelineStage::Mutate => {
                context.proposals = context
                    .candidates
                    .iter()
                    .enumerate()
                    .map(|(i, candidate)| MutationProposal {
                        proposal_id: format!("proposal_{}", i),
                        signal_ids: vec![],
                        gene_id: candidate.gene.id.clone(),
                        description: format!("Mutation for gene {}", candidate.gene.id),
                        estimated_impact: candidate.score,
                        risk_level: MutationRiskLevel::Medium,
                        proposed_changes: serde_json::json!({}),
                    })
                    .collect();
                Ok(PipelineStageState::Completed)
            }
            PipelineStage::Execute => {
                context.execution_result = Some(serde_json::json!({
                    "status": "success"
                }));
                Ok(PipelineStageState::Completed)
            }
            PipelineStage::Validate => {
                if let Some(proposal) = context.proposals.first() {
                    let validation_result = if let Some(ref validator) = self.validate_port {
                        let exec = context.execution_result.as_ref();
                        let input = ValidateInput {
                            proposal_id: proposal.proposal_id.clone(),
                            execution_success: exec
                                .and_then(|value| value.get("success"))
                                .and_then(|value| value.as_bool())
                                .unwrap_or(false),
                            stdout: exec
                                .and_then(|value| value.get("stdout"))
                                .and_then(|value| value.as_str())
                                .unwrap_or("")
                                .to_string(),
                            stderr: exec
                                .and_then(|value| value.get("stderr"))
                                .and_then(|value| value.as_str())
                                .unwrap_or("")
                                .to_string(),
                        };
                        validator.validate(&input)
                    } else {
                        ValidationResult {
                            proposal_id: proposal.proposal_id.clone(),
                            passed: true,
                            score: 0.8,
                            issues: vec![],
                            simulation_results: None,
                        }
                    };
                    context.validation_result = Some(validation_result);
                }
                Ok(match context.validation_result.as_ref() {
                    Some(result) if !result.passed => {
                        PipelineStageState::Failed("validation failed".to_string())
                    }
                    _ => PipelineStageState::Completed,
                })
            }
            PipelineStage::Evaluate => {
                context.evaluation_result = if let Some(ref evaluator) = self.evaluate_port {
                    context.proposals.first().map(|proposal| {
                        evaluator.evaluate(&EvaluateInput {
                            proposal_id: proposal.proposal_id.clone(),
                            intent: proposal.description.clone(),
                            original: String::new(),
                            proposed: String::new(),
                            signals: context
                                .signals
                                .iter()
                                .map(|signal| signal.description.clone())
                                .collect(),
                        })
                    })
                } else {
                    Some(EvaluationResult {
                        score: 0.8,
                        improvements: vec!["Mutation applied successfully".to_string()],
                        regressions: vec![],
                        recommendation: EvaluationRecommendation::Accept,
                    })
                };
                Ok(PipelineStageState::Completed)
            }
            PipelineStage::Solidify => {
                for candidate in &context.candidates {
                    let gene = &candidate.gene;
                    if let Some(ref store) = self.gene_store {
                        store.persist_gene(
                            &gene.id,
                            &gene.signals,
                            &gene.strategy,
                            &gene.validation,
                        );
                    }
                }
                context.solidified_genes = context
                    .candidates
                    .iter()
                    .map(|c| c.gene.id.clone())
                    .collect();
                Ok(PipelineStageState::Completed)
            }
            PipelineStage::Reuse => {
                for candidate in &context.candidates {
                    let cap_ids: Vec<String> =
                        candidate.capsules.iter().map(|c| c.id.clone()).collect();
                    if let Some(ref store) = self.gene_store {
                        store.mark_reused(&candidate.gene.id, &cap_ids);
                    }
                    context.reused_capsules.extend(cap_ids);
                }
                Ok(PipelineStageState::Completed)
            }
        }
    }
}

/// Build a minimal `PreparedMutation` from a `MutationProposal`.
///
/// Used by the Execute stage when a `SandboxPort` is injected. The resulting
/// mutation carries the proposal identifier and an empty unified-diff payload;
/// real mutation generation (LLM or rule-based) will replace this in a later
/// phase of the evolution pipeline.
fn build_prepared_mutation(proposal: &MutationProposal) -> PreparedMutation {
    use crate::core::{
        ArtifactEncoding, MutationArtifact, MutationIntent, MutationTarget, RiskLevel,
    };

    PreparedMutation {
        intent: MutationIntent {
            id: proposal.proposal_id.clone(),
            intent: proposal.description.clone(),
            target: MutationTarget::WorkspaceRoot,
            expected_effect: format!("Apply mutation for gene {}", proposal.gene_id),
            risk: match proposal.risk_level {
                MutationRiskLevel::Low => RiskLevel::Low,
                MutationRiskLevel::Medium => RiskLevel::Medium,
                MutationRiskLevel::High | MutationRiskLevel::Critical => RiskLevel::High,
            },
            signals: proposal.signal_ids.clone(),
            spec_id: None,
        },
        artifact: MutationArtifact {
            encoding: ArtifactEncoding::UnifiedDiff,
            payload: String::new(),
            base_revision: None,
            content_hash: String::new(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn test_pipeline_config_default() {
        let config = EvolutionPipelineConfig::default();
        assert!(config.enable_detect);
        assert!(config.enable_select);
        assert!(config.enable_mutate);
    }

    #[test]
    fn test_pipeline_stage_states() {
        let config = EvolutionPipelineConfig {
            enable_detect: false,
            enable_select: true,
            enable_mutate: false,
            ..Default::default()
        };

        // Just test that config works
        assert!(!config.enable_detect);
        assert!(config.enable_select);
    }

    // ─── GeneStorePersistPort integration test ─────────────────────────────

    /// A minimal in-memory mock that records which genes/capsules were persisted.
    struct MockGeneStore {
        genes: std::sync::Mutex<Vec<String>>,
        reused: std::sync::Mutex<Vec<String>>,
    }

    impl MockGeneStore {
        fn new() -> Self {
            Self {
                genes: std::sync::Mutex::new(Vec::new()),
                reused: std::sync::Mutex::new(Vec::new()),
            }
        }
    }

    impl GeneStorePersistPort for MockGeneStore {
        fn persist_gene(
            &self,
            gene_id: &str,
            _signals: &[String],
            _strategy: &[String],
            _validation: &[String],
        ) -> bool {
            self.genes.lock().unwrap().push(gene_id.to_string());
            true
        }

        fn mark_reused(&self, gene_id: &str, _capsule_ids: &[String]) -> bool {
            self.reused.lock().unwrap().push(gene_id.to_string());
            true
        }
    }

    /// A minimal `Selector` that always returns one hard-coded candidate.
    struct SingleCandidateSelector;

    impl Selector for SingleCandidateSelector {
        fn select(&self, _input: &SelectorInput) -> Vec<GeneCandidate> {
            use crate::core;
            vec![GeneCandidate {
                gene: core::Gene {
                    id: "gene-abc-001".to_string(),
                    signals: vec!["test-signal".to_string()],
                    strategy: vec!["apply-fix".to_string()],
                    validation: vec!["cargo test".to_string()],
                    state: core::AssetState::default(),
                    task_class_id: None,
                },
                capsules: vec![],
                score: 0.9,
            }]
        }
    }

    struct RecordingValidatePort {
        passed: bool,
        calls: std::sync::Mutex<Vec<ValidateInput>>,
    }

    impl RecordingValidatePort {
        fn new(passed: bool) -> Self {
            Self {
                passed,
                calls: std::sync::Mutex::new(Vec::new()),
            }
        }
    }

    impl ValidatePort for RecordingValidatePort {
        fn validate(&self, input: &ValidateInput) -> ValidationResult {
            self.calls.lock().unwrap().push(input.clone());
            ValidationResult {
                proposal_id: input.proposal_id.clone(),
                passed: self.passed,
                score: if self.passed { 1.0 } else { 0.2 },
                issues: vec![],
                simulation_results: None,
            }
        }
    }

    struct RecordingEvaluatePort {
        calls: std::sync::Mutex<Vec<EvaluateInput>>,
    }

    impl RecordingEvaluatePort {
        fn new() -> Self {
            Self {
                calls: std::sync::Mutex::new(Vec::new()),
            }
        }
    }

    struct CountingValidatePort {
        call_count: Arc<AtomicUsize>,
        passed: bool,
    }

    impl CountingValidatePort {
        fn new(call_count: Arc<AtomicUsize>, passed: bool) -> Self {
            Self { call_count, passed }
        }
    }

    impl ValidatePort for CountingValidatePort {
        fn validate(&self, input: &ValidateInput) -> ValidationResult {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            ValidationResult {
                proposal_id: input.proposal_id.clone(),
                passed: self.passed,
                score: if self.passed { 0.95 } else { 0.15 },
                issues: vec![],
                simulation_results: None,
            }
        }
    }

    struct CountingEvaluatePort {
        call_count: Arc<AtomicUsize>,
    }

    impl CountingEvaluatePort {
        fn new(call_count: Arc<AtomicUsize>) -> Self {
            Self { call_count }
        }
    }

    impl EvaluatePort for CountingEvaluatePort {
        fn evaluate(&self, _input: &EvaluateInput) -> EvaluationResult {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            EvaluationResult {
                score: 0.91,
                improvements: vec!["evaluate port called".to_string()],
                regressions: vec![],
                recommendation: EvaluationRecommendation::Accept,
            }
        }
    }

    impl EvaluatePort for RecordingEvaluatePort {
        fn evaluate(&self, input: &EvaluateInput) -> EvaluationResult {
            self.calls.lock().unwrap().push(input.clone());
            EvaluationResult {
                score: 0.33,
                improvements: vec!["used injected evaluator".to_string()],
                regressions: vec!["none".to_string()],
                recommendation: EvaluationRecommendation::NeedsRevision,
            }
        }
    }

    #[test]
    fn test_solidify_reuse_calls_gene_store() {
        let store = Arc::new(MockGeneStore::new());
        let config = EvolutionPipelineConfig {
            enable_detect: false,
            enable_select: true,
            enable_mutate: false,
            enable_execute: false,
            enable_validate: false,
            enable_evaluate: false,
            enable_solidify: true,
            enable_reuse: true,
            ..Default::default()
        };

        let pipeline = StandardEvolutionPipeline::new(config, Arc::new(SingleCandidateSelector))
            .with_gene_store(store.clone());

        let ctx = PipelineContext::default();
        let result = pipeline.execute(ctx).expect("pipeline should succeed");
        assert!(result.success);

        // Verify Solidify stage persisted the gene
        let persisted_genes = store.genes.lock().unwrap();
        assert!(
            persisted_genes.contains(&"gene-abc-001".to_string()),
            "Solidify stage should have persisted gene-abc-001, got: {:?}",
            persisted_genes
        );
    }

    #[test]
    fn test_execute_stage_validate_uses_injected_port_and_fallback() {
        let validator = Arc::new(RecordingValidatePort::new(false));
        let pipeline = StandardEvolutionPipeline::new(
            EvolutionPipelineConfig::default(),
            Arc::new(SingleCandidateSelector),
        )
        .with_validate_port(validator.clone());

        let mut context = PipelineContext::default();
        context.proposals.push(MutationProposal {
            proposal_id: "proposal-1".to_string(),
            signal_ids: vec![],
            gene_id: "gene-1".to_string(),
            description: "validate proposal".to_string(),
            estimated_impact: 0.5,
            risk_level: MutationRiskLevel::Medium,
            proposed_changes: serde_json::json!({}),
        });
        context.execution_result = Some(serde_json::json!({
            "success": true,
            "stdout": "validator stdout",
            "stderr": ""
        }));

        let state = pipeline
            .execute_stage(PipelineStage::Validate, &mut context)
            .expect("validate stage should succeed");

        assert_eq!(
            state,
            PipelineStageState::Failed("validation failed".to_string())
        );
        let calls = validator.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].proposal_id, "proposal-1");
        assert!(calls[0].execution_success);
        assert_eq!(calls[0].stdout, "validator stdout");
        drop(calls);
        assert!(!context.validation_result.as_ref().unwrap().passed);

        let fallback_pipeline = StandardEvolutionPipeline::new(
            EvolutionPipelineConfig::default(),
            Arc::new(SingleCandidateSelector),
        );
        let mut fallback_context = PipelineContext::default();
        fallback_context.proposals.push(MutationProposal {
            proposal_id: "proposal-2".to_string(),
            signal_ids: vec![],
            gene_id: "gene-2".to_string(),
            description: "fallback validate".to_string(),
            estimated_impact: 0.4,
            risk_level: MutationRiskLevel::Low,
            proposed_changes: serde_json::json!({}),
        });

        let fallback_state = fallback_pipeline
            .execute_stage(PipelineStage::Validate, &mut fallback_context)
            .expect("fallback validate stage should succeed");

        assert_eq!(fallback_state, PipelineStageState::Completed);
        assert!(fallback_context.validation_result.as_ref().unwrap().passed);
        assert_eq!(
            fallback_context.validation_result.as_ref().unwrap().score,
            0.8
        );
    }

    #[test]
    fn test_execute_stage_evaluate_uses_injected_port_and_fallback() {
        let evaluator = Arc::new(RecordingEvaluatePort::new());
        let pipeline = StandardEvolutionPipeline::new(
            EvolutionPipelineConfig::default(),
            Arc::new(SingleCandidateSelector),
        )
        .with_evaluate_port(evaluator.clone());

        let mut context = PipelineContext::default();
        context.signals.push(EvolutionSignal {
            signal_id: "signal-1".to_string(),
            signal_type: crate::evolver::SignalType::ErrorPattern {
                error_type: "panic".to_string(),
                frequency: 1,
            },
            source_task_id: "task-1".to_string(),
            confidence: 0.9,
            description: "improve evaluator path".to_string(),
            metadata: serde_json::json!({}),
        });
        context.proposals.push(MutationProposal {
            proposal_id: "proposal-3".to_string(),
            signal_ids: vec!["signal-1".to_string()],
            gene_id: "gene-3".to_string(),
            description: "evaluate proposal".to_string(),
            estimated_impact: 0.7,
            risk_level: MutationRiskLevel::Medium,
            proposed_changes: serde_json::json!({}),
        });

        let state = pipeline
            .execute_stage(PipelineStage::Evaluate, &mut context)
            .expect("evaluate stage should succeed");

        assert_eq!(state, PipelineStageState::Completed);
        let calls = evaluator.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].proposal_id, "proposal-3");
        assert_eq!(calls[0].intent, "evaluate proposal");
        assert_eq!(calls[0].signals, vec!["improve evaluator path".to_string()]);
        drop(calls);
        assert_eq!(
            context.evaluation_result.as_ref().unwrap().recommendation,
            EvaluationRecommendation::NeedsRevision
        );
        assert_eq!(context.evaluation_result.as_ref().unwrap().score, 0.33);

        let fallback_pipeline = StandardEvolutionPipeline::new(
            EvolutionPipelineConfig::default(),
            Arc::new(SingleCandidateSelector),
        );
        let mut fallback_context = PipelineContext::default();

        let fallback_state = fallback_pipeline
            .execute_stage(PipelineStage::Evaluate, &mut fallback_context)
            .expect("fallback evaluate stage should succeed");

        assert_eq!(fallback_state, PipelineStageState::Completed);
        assert_eq!(
            fallback_context
                .evaluation_result
                .as_ref()
                .unwrap()
                .recommendation,
            EvaluationRecommendation::Accept
        );
        assert_eq!(
            fallback_context
                .evaluation_result
                .as_ref()
                .unwrap()
                .improvements,
            vec!["Mutation applied successfully".to_string()]
        );
    }

    #[test]
    fn test_execute_invokes_injected_validate_and_evaluate_ports() {
        let validate_calls = Arc::new(AtomicUsize::new(0));
        let evaluate_calls = Arc::new(AtomicUsize::new(0));
        let config = EvolutionPipelineConfig {
            enable_detect: false,
            enable_select: true,
            enable_mutate: true,
            enable_execute: true,
            enable_validate: true,
            enable_evaluate: true,
            enable_solidify: false,
            enable_reuse: false,
            ..Default::default()
        };

        let pipeline = StandardEvolutionPipeline::new(config, Arc::new(SingleCandidateSelector))
            .with_validate_port(Arc::new(CountingValidatePort::new(
                validate_calls.clone(),
                true,
            )))
            .with_evaluate_port(Arc::new(CountingEvaluatePort::new(
                evaluate_calls.clone(),
            )));

        let result = pipeline
            .execute(PipelineContext::default())
            .expect("pipeline should execute");

        assert!(result.success);
        assert_eq!(validate_calls.load(Ordering::SeqCst), 1);
        assert_eq!(evaluate_calls.load(Ordering::SeqCst), 1);
        assert!(result.error.is_none());
        assert!(result.stage_states.iter().any(|stage| {
            stage.stage_name == PipelineStage::Validate.as_str()
                && stage.state == PipelineStageState::Completed
        }));
        assert!(result.stage_states.iter().any(|stage| {
            stage.stage_name == PipelineStage::Evaluate.as_str()
                && stage.state == PipelineStageState::Completed
        }));
    }

    #[test]
    fn test_execute_propagates_validate_port_failure_to_pipeline_result() {
        let validate_calls = Arc::new(AtomicUsize::new(0));
        let evaluate_calls = Arc::new(AtomicUsize::new(0));
        let config = EvolutionPipelineConfig {
            enable_detect: false,
            enable_select: true,
            enable_mutate: true,
            enable_execute: true,
            enable_validate: true,
            enable_evaluate: true,
            enable_solidify: false,
            enable_reuse: false,
            ..Default::default()
        };

        let pipeline = StandardEvolutionPipeline::new(config, Arc::new(SingleCandidateSelector))
            .with_validate_port(Arc::new(CountingValidatePort::new(
                validate_calls.clone(),
                false,
            )))
            .with_evaluate_port(Arc::new(CountingEvaluatePort::new(
                evaluate_calls.clone(),
            )));

        let result = pipeline
            .execute(PipelineContext::default())
            .expect("pipeline should execute");

        assert!(!result.success);
        assert_eq!(validate_calls.load(Ordering::SeqCst), 1);
        assert_eq!(evaluate_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            result.error.as_deref(),
            Some("Validation stage did not pass")
        );
        assert!(result.stage_states.iter().any(|stage| {
            stage.stage_name == PipelineStage::Validate.as_str()
                && stage.state == PipelineStageState::Failed("validation failed".to_string())
        }));
    }
}
