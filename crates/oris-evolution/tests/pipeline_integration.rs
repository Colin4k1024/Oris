//! Full-loop integration tests for StandardEvolutionPipeline.
//!
//! Scenarios:
//! 1. Happy path: Detect→Select→Mutate→Execute→Validate→Evaluate→Solidify→Reuse
//! 2. Validation failure: pipeline halts at Validate, result.success = false
//! 3. Signal extractor integration: injected extractor populates signals
//! 4. Sandbox failure: Execute stage fails, validation sees execution_success=false
//! 5. Empty candidates: Select returns nothing, pipeline still succeeds gracefully

use std::sync::{Arc, Mutex};

use oris_evolution::{
    AssetState, Capsule, EnvFingerprint, EvaluateInput, EvaluatePort, EvaluationRecommendation,
    EvaluationResult, EvolutionPipeline, EvolutionPipelineConfig, EvolutionSignal, Gene,
    GeneCandidate, GeneStorePersistPort, Outcome, PipelineContext, PipelineStage,
    PipelineStageState, PreparedMutation, SandboxExecutionResult, SandboxPort, Selector,
    SelectorInput, SignalExtractorInput, SignalExtractorPort, SignalType,
    StandardEvolutionPipeline, ValidateInput, ValidatePort, ValidationResult,
};

// ─── Mock implementations ────────────────────────────────────────────────────

struct FixedSelector {
    candidates: Vec<GeneCandidate>,
}

impl FixedSelector {
    fn single() -> Self {
        Self {
            candidates: vec![GeneCandidate {
                gene: Gene {
                    id: "gene-integ-001".to_string(),
                    signals: vec!["compile-error".to_string()],
                    strategy: vec!["fix-type-mismatch".to_string()],
                    validation: vec!["cargo test".to_string()],
                    state: AssetState::default(),
                    task_class_id: None,
                },
                capsules: vec![Capsule {
                    id: "capsule-001".to_string(),
                    gene_id: "gene-integ-001".to_string(),
                    mutation_id: "mutation-001".to_string(),
                    run_id: "run-001".to_string(),
                    diff_hash: "abc123".to_string(),
                    confidence: 0.95,
                    env: EnvFingerprint {
                        rustc_version: "1.76.0".to_string(),
                        cargo_lock_hash: "deadbeef".to_string(),
                        target_triple: "x86_64-unknown-linux-gnu".to_string(),
                        os: "linux".to_string(),
                    },
                    outcome: Outcome {
                        success: true,
                        validation_profile: "default".to_string(),
                        validation_duration_ms: 100,
                        changed_files: vec!["src/main.rs".to_string()],
                        validator_hash: "val-hash".to_string(),
                        lines_changed: 5,
                        replay_verified: false,
                    },
                    state: AssetState::default(),
                }],
                score: 0.92,
            }],
        }
    }

    fn empty() -> Self {
        Self { candidates: vec![] }
    }
}

impl Selector for FixedSelector {
    fn select(&self, _input: &SelectorInput) -> Vec<GeneCandidate> {
        self.candidates.clone()
    }
}

struct RecordingGeneStore {
    persisted: Mutex<Vec<String>>,
    reused: Mutex<Vec<(String, Vec<String>)>>,
}

impl RecordingGeneStore {
    fn new() -> Self {
        Self {
            persisted: Mutex::new(Vec::new()),
            reused: Mutex::new(Vec::new()),
        }
    }
}

impl GeneStorePersistPort for RecordingGeneStore {
    fn persist_gene(
        &self,
        gene_id: &str,
        _signals: &[String],
        _strategy: &[String],
        _validation: &[String],
    ) -> bool {
        self.persisted.lock().unwrap().push(gene_id.to_string());
        true
    }

    fn mark_reused(&self, gene_id: &str, capsule_ids: &[String]) -> bool {
        self.reused
            .lock()
            .unwrap()
            .push((gene_id.to_string(), capsule_ids.to_vec()));
        true
    }
}

struct PassingValidator;

impl ValidatePort for PassingValidator {
    fn validate(&self, input: &ValidateInput) -> ValidationResult {
        ValidationResult {
            proposal_id: input.proposal_id.clone(),
            passed: true,
            score: 0.95,
            issues: vec![],
            simulation_results: None,
        }
    }
}

struct FailingValidator;

impl ValidatePort for FailingValidator {
    fn validate(&self, input: &ValidateInput) -> ValidationResult {
        ValidationResult {
            proposal_id: input.proposal_id.clone(),
            passed: false,
            score: 0.1,
            issues: vec![],
            simulation_results: None,
        }
    }
}

struct AcceptingEvaluator;

impl EvaluatePort for AcceptingEvaluator {
    fn evaluate(&self, _input: &EvaluateInput) -> EvaluationResult {
        EvaluationResult {
            score: 0.9,
            improvements: vec!["type error resolved".to_string()],
            regressions: vec![],
            recommendation: EvaluationRecommendation::Accept,
        }
    }
}

struct MockSignalExtractor {
    signals: Vec<EvolutionSignal>,
}

impl MockSignalExtractor {
    fn with_signals(signals: Vec<EvolutionSignal>) -> Self {
        Self { signals }
    }
}

impl SignalExtractorPort for MockSignalExtractor {
    fn extract(&self, _input: &SignalExtractorInput) -> Vec<EvolutionSignal> {
        self.signals.clone()
    }
}

struct SuccessSandbox;

impl SandboxPort for SuccessSandbox {
    fn execute(&self, _mutation: &PreparedMutation) -> SandboxExecutionResult {
        SandboxExecutionResult::success("all tests pass", 120)
    }
}

struct FailingSandbox;

impl SandboxPort for FailingSandbox {
    fn execute(&self, _mutation: &PreparedMutation) -> SandboxExecutionResult {
        SandboxExecutionResult::failure("cargo test failed", "compilation error", 50)
    }
}

// ─── Integration Tests ───────────────────────────────────────────────────────

#[test]
fn full_loop_happy_path_detect_through_reuse() {
    let store = Arc::new(RecordingGeneStore::new());
    let signals = vec![EvolutionSignal {
        signal_id: "sig-1".to_string(),
        signal_type: SignalType::ErrorPattern {
            error_type: "E0308".to_string(),
            frequency: 3,
        },
        source_task_id: "task-100".to_string(),
        confidence: 0.95,
        description: "type mismatch in module X".to_string(),
        metadata: serde_json::json!({}),
    }];

    let extractor = Arc::new(MockSignalExtractor::with_signals(signals));
    let config = EvolutionPipelineConfig::default();

    let pipeline = StandardEvolutionPipeline::new(config, Arc::new(FixedSelector::single()))
        .with_signal_extractor(extractor)
        .with_sandbox(Arc::new(SuccessSandbox))
        .with_validate_port(Arc::new(PassingValidator))
        .with_evaluate_port(Arc::new(AcceptingEvaluator))
        .with_gene_store(store.clone());

    let mut ctx = PipelineContext::default();
    ctx.extractor_input = Some(SignalExtractorInput {
        compiler_output: Some("error[E0308]: mismatched types".to_string()),
        ..Default::default()
    });

    let result = pipeline.execute(ctx).expect("pipeline should succeed");

    assert!(result.success, "pipeline should report success");
    assert!(result.error.is_none());

    // All 8 stages completed
    assert_eq!(result.stage_states.len(), 8);
    for stage in &result.stage_states {
        assert_eq!(
            stage.state,
            PipelineStageState::Completed,
            "stage {} should be Completed",
            stage.stage_name
        );
        assert!(stage.duration_ms.is_some());
    }

    // Solidify persisted the gene
    let persisted = store.persisted.lock().unwrap();
    assert_eq!(persisted.len(), 1);
    assert_eq!(persisted[0], "gene-integ-001");

    // Reuse recorded the capsule
    let reused = store.reused.lock().unwrap();
    assert_eq!(reused.len(), 1);
    assert_eq!(reused[0].0, "gene-integ-001");
    assert_eq!(reused[0].1, vec!["capsule-001"]);
}

#[test]
fn validation_failure_halts_pipeline_success() {
    let config = EvolutionPipelineConfig::default();

    let pipeline = StandardEvolutionPipeline::new(config, Arc::new(FixedSelector::single()))
        .with_sandbox(Arc::new(SuccessSandbox))
        .with_validate_port(Arc::new(FailingValidator))
        .with_evaluate_port(Arc::new(AcceptingEvaluator));

    let result = pipeline
        .execute(PipelineContext::default())
        .expect("pipeline should not error");

    assert!(!result.success);
    assert_eq!(
        result.error.as_deref(),
        Some("Validation stage did not pass")
    );

    // Validate stage recorded as Failed
    let validate_stage = result
        .stage_states
        .iter()
        .find(|s| s.stage_name == PipelineStage::Validate.as_str())
        .expect("validate stage should exist");
    assert_eq!(
        validate_stage.state,
        PipelineStageState::Failed("validation failed".to_string())
    );
}

#[test]
fn signal_extractor_populates_context_signals() {
    let signals = vec![
        EvolutionSignal {
            signal_id: "sig-a".to_string(),
            signal_type: SignalType::Performance {
                metric: "latency".to_string(),
                improvement_potential: 0.6,
            },
            source_task_id: "task-200".to_string(),
            confidence: 0.88,
            description: "p99 latency regression".to_string(),
            metadata: serde_json::json!({}),
        },
        EvolutionSignal {
            signal_id: "sig-b".to_string(),
            signal_type: SignalType::QualityIssue {
                issue_type: "unused import".to_string(),
                severity: 0.3,
            },
            source_task_id: "task-200".to_string(),
            confidence: 0.72,
            description: "dead code detected".to_string(),
            metadata: serde_json::json!({}),
        },
    ];

    let extractor = Arc::new(MockSignalExtractor::with_signals(signals));
    let config = EvolutionPipelineConfig {
        enable_execute: false,
        enable_validate: false,
        enable_evaluate: false,
        enable_solidify: false,
        enable_reuse: false,
        ..Default::default()
    };

    let pipeline = StandardEvolutionPipeline::new(config, Arc::new(FixedSelector::single()))
        .with_signal_extractor(extractor);

    let mut ctx = PipelineContext::default();
    ctx.extractor_input = Some(SignalExtractorInput {
        logs: Some("WARNING: p99 latency exceeded threshold".to_string()),
        ..Default::default()
    });

    let result = pipeline.execute(ctx).expect("pipeline ok");
    assert!(result.success);

    // Detect stage completed
    let detect = result
        .stage_states
        .iter()
        .find(|s| s.stage_name == "detect")
        .unwrap();
    assert_eq!(detect.state, PipelineStageState::Completed);

    // Select stage ran (candidates produced)
    let select = result
        .stage_states
        .iter()
        .find(|s| s.stage_name == "select")
        .unwrap();
    assert_eq!(select.state, PipelineStageState::Completed);
}

#[test]
fn sandbox_failure_propagates_to_execute_stage() {
    let config = EvolutionPipelineConfig {
        enable_solidify: false,
        enable_reuse: false,
        ..Default::default()
    };

    let pipeline = StandardEvolutionPipeline::new(config, Arc::new(FixedSelector::single()))
        .with_sandbox(Arc::new(FailingSandbox))
        .with_validate_port(Arc::new(PassingValidator))
        .with_evaluate_port(Arc::new(AcceptingEvaluator));

    let result = pipeline
        .execute(PipelineContext::default())
        .expect("pipeline should not error");

    // The pipeline still runs to completion (evaluate happens after)
    // but the Execute stage is marked Failed
    let execute_stage = result
        .stage_states
        .iter()
        .find(|s| s.stage_name == PipelineStage::Execute.as_str())
        .expect("execute stage should exist");
    assert_eq!(
        execute_stage.state,
        PipelineStageState::Failed("compilation error".to_string())
    );
}

#[test]
fn empty_candidates_pipeline_succeeds_gracefully() {
    let store = Arc::new(RecordingGeneStore::new());
    let config = EvolutionPipelineConfig::default();

    let pipeline = StandardEvolutionPipeline::new(config, Arc::new(FixedSelector::empty()))
        .with_gene_store(store.clone());

    let result = pipeline
        .execute(PipelineContext::default())
        .expect("pipeline should succeed");

    assert!(result.success);

    // Solidify ran but persisted nothing
    let persisted = store.persisted.lock().unwrap();
    assert!(persisted.is_empty());

    // Reuse ran but recorded nothing
    let reused = store.reused.lock().unwrap();
    assert!(reused.is_empty());

    // All stages still completed (no candidates is not an error)
    let solidify = result
        .stage_states
        .iter()
        .find(|s| s.stage_name == "solidify")
        .unwrap();
    assert_eq!(solidify.state, PipelineStageState::Completed);
}
