//! Verifies that OTel-compatible tracing spans are emitted for pipeline stages.

use std::sync::{Arc, Mutex};

use oris_evolution::{
    AssetState, Capsule, EnvFingerprint, EvaluateInput, EvaluatePort, EvaluationRecommendation,
    EvaluationResult, EvolutionPipeline, EvolutionPipelineConfig, Gene, GeneCandidate, Outcome,
    PipelineContext, PreparedMutation, SandboxExecutionResult, SandboxPort, Selector,
    SelectorInput, StandardEvolutionPipeline, ValidateInput, ValidatePort, ValidationResult,
};
use tracing_subscriber::layer::SubscriberExt;

struct SingleSelector;

impl Selector for SingleSelector {
    fn select(&self, _input: &SelectorInput) -> Vec<GeneCandidate> {
        vec![GeneCandidate {
            gene: Gene {
                id: "gene-span-test".to_string(),
                signals: vec!["test".to_string()],
                strategy: vec!["fix".to_string()],
                validation: vec!["cargo test".to_string()],
                state: AssetState::default(),
                task_class_id: None,
            },
            capsules: vec![Capsule {
                id: "cap-1".to_string(),
                gene_id: "gene-span-test".to_string(),
                mutation_id: "m-1".to_string(),
                run_id: "r-1".to_string(),
                diff_hash: "h".to_string(),
                confidence: 0.9,
                env: EnvFingerprint {
                    rustc_version: "1.76".to_string(),
                    cargo_lock_hash: "x".to_string(),
                    target_triple: "x86_64".to_string(),
                    os: "linux".to_string(),
                },
                outcome: Outcome {
                    success: true,
                    validation_profile: "default".to_string(),
                    validation_duration_ms: 50,
                    changed_files: vec![],
                    validator_hash: "v".to_string(),
                    lines_changed: 1,
                    replay_verified: false,
                },
                state: AssetState::default(),
            }],
            score: 0.9,
        }]
    }
}

struct OkSandbox;
impl SandboxPort for OkSandbox {
    fn execute(&self, _m: &PreparedMutation) -> SandboxExecutionResult {
        SandboxExecutionResult::success("ok", 10)
    }
}

struct OkValidator;
impl ValidatePort for OkValidator {
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

struct OkEvaluator;
impl EvaluatePort for OkEvaluator {
    fn evaluate(&self, _input: &EvaluateInput) -> EvaluationResult {
        EvaluationResult {
            score: 0.9,
            improvements: vec![],
            regressions: vec![],
            recommendation: EvaluationRecommendation::Accept,
        }
    }
}

/// Collects span names entered during a pipeline execution.
struct SpanCollector {
    spans: Arc<Mutex<Vec<String>>>,
}

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for SpanCollector {
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        _id: &tracing::span::Id,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        self.spans
            .lock()
            .unwrap()
            .push(attrs.metadata().name().to_string());
    }
}

#[test]
fn pipeline_emits_five_stage_spans() {
    let spans: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let collector = SpanCollector {
        spans: spans.clone(),
    };

    let subscriber = tracing_subscriber::registry().with(collector);
    let _guard = tracing::subscriber::set_default(subscriber);

    let config = EvolutionPipelineConfig {
        enable_solidify: false,
        enable_reuse: false,
        ..Default::default()
    };

    let pipeline = StandardEvolutionPipeline::new(config, Arc::new(SingleSelector))
        .with_sandbox(Arc::new(OkSandbox))
        .with_validate_port(Arc::new(OkValidator))
        .with_evaluate_port(Arc::new(OkEvaluator));

    let result = pipeline.execute(PipelineContext::default()).unwrap();
    assert!(result.success);

    let captured = spans.lock().unwrap();
    assert!(
        captured.contains(&"evolution.detect".to_string()),
        "missing detect span"
    );
    assert!(
        captured.contains(&"evolution.select".to_string()),
        "missing select span"
    );
    assert!(
        captured.contains(&"evolution.mutate".to_string()),
        "missing mutate span"
    );
    assert!(
        captured.contains(&"evolution.execute".to_string()),
        "missing execute span"
    );
    assert!(
        captured.contains(&"evolution.validate".to_string()),
        "missing validate span"
    );
}

#[test]
fn disabled_stages_emit_no_spans() {
    let spans: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let collector = SpanCollector {
        spans: spans.clone(),
    };

    let subscriber = tracing_subscriber::registry().with(collector);
    let _guard = tracing::subscriber::set_default(subscriber);

    let config = EvolutionPipelineConfig {
        enable_detect: true,
        enable_select: true,
        enable_mutate: false,
        enable_execute: false,
        enable_validate: false,
        enable_evaluate: false,
        enable_solidify: false,
        enable_reuse: false,
        ..Default::default()
    };

    let pipeline = StandardEvolutionPipeline::new(config, Arc::new(SingleSelector));
    let _ = pipeline.execute(PipelineContext::default());

    let captured = spans.lock().unwrap();
    assert!(captured.contains(&"evolution.detect".to_string()));
    assert!(captured.contains(&"evolution.select".to_string()));
    assert!(!captured.contains(&"evolution.mutate".to_string()));
    assert!(!captured.contains(&"evolution.execute".to_string()));
    assert!(!captured.contains(&"evolution.validate".to_string()));
}
