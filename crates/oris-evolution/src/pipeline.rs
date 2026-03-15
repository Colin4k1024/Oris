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
use crate::port::{SandboxPort, SignalExtractorInput, SignalExtractorPort};

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

        // Validate phase - placeholder
        if self.config.enable_validate {
            let mut stage = StageState::new(PipelineStage::Validate.as_str());
            stage.state = PipelineStageState::Running;
            stage_states.push(stage);

            let t0 = Instant::now();
            // Create a validation result for each proposal
            if let Some(proposal) = context.proposals.first() {
                context.validation_result = Some(ValidationResult {
                    proposal_id: proposal.proposal_id.clone(),
                    passed: true,
                    score: 0.8,
                    issues: vec![],
                    simulation_results: None,
                });
            }
            let elapsed = t0.elapsed();
            context
                .stage_timings
                .insert(PipelineStage::Validate.as_str().to_string(), elapsed);
            let last = stage_states.last_mut().unwrap();
            last.state = PipelineStageState::Completed;
            last.duration_ms = Some(elapsed.as_millis() as u64);
        } else {
            stage_states.push(StageState {
                stage_name: PipelineStage::Validate.as_str().to_string(),
                state: PipelineStageState::Skipped("disabled".to_string()),
                duration_ms: None,
            });
        }

        // Evaluate phase - placeholder
        if self.config.enable_evaluate {
            let mut stage = StageState::new(PipelineStage::Evaluate.as_str());
            stage.state = PipelineStageState::Running;
            stage_states.push(stage);

            let t0 = Instant::now();
            context.evaluation_result = Some(EvaluationResult {
                score: 0.8,
                improvements: vec!["Mutation applied successfully".to_string()],
                regressions: vec![],
                recommendation: EvaluationRecommendation::Accept,
            });
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

        // Solidify phase - placeholder
        if self.config.enable_solidify {
            let mut stage = StageState::new(PipelineStage::Solidify.as_str());
            stage.state = PipelineStageState::Running;
            stage_states.push(stage);

            let t0 = Instant::now();
            context.solidified_genes = context
                .candidates
                .iter()
                .map(|c| c.gene.id.clone())
                .collect();
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

        // Reuse phase - placeholder
        if self.config.enable_reuse {
            let mut stage = StageState::new(PipelineStage::Reuse.as_str());
            stage.state = PipelineStageState::Running;
            stage_states.push(stage);

            let t0 = Instant::now();
            // Mark capsules as reusable
            context.reused_capsules = context
                .candidates
                .iter()
                .flat_map(|c| c.capsules.iter().map(|cap| cap.id.clone()))
                .collect();
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

        Ok(PipelineResult {
            success: true,
            stage_states,
            error: None,
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
                    context.validation_result = Some(ValidationResult {
                        proposal_id: proposal.proposal_id.clone(),
                        passed: true,
                        score: 0.8,
                        issues: vec![],
                        simulation_results: None,
                    });
                }
                Ok(PipelineStageState::Completed)
            }
            PipelineStage::Evaluate => {
                context.evaluation_result = Some(EvaluationResult {
                    score: 0.8,
                    improvements: vec![],
                    regressions: vec![],
                    recommendation: EvaluationRecommendation::Accept,
                });
                Ok(PipelineStageState::Completed)
            }
            PipelineStage::Solidify => {
                context.solidified_genes = context
                    .candidates
                    .iter()
                    .map(|c| c.gene.id.clone())
                    .collect();
                Ok(PipelineStageState::Completed)
            }
            PipelineStage::Reuse => {
                context.reused_capsules = context
                    .candidates
                    .iter()
                    .flat_map(|c| c.capsules.iter().map(|cap| cap.id.clone()))
                    .collect();
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
}
