//! Integration tests for the self-evolution demo pipeline.
//!
//! These tests verify the evolution pipeline behavior without requiring
//! the CLI binary output. Tests use the same components as the demo.

use std::sync::Arc;

use oris_evokernel::adapters::{RuntimeSignalExtractorAdapter, SqliteGeneStorePersistAdapter};
use oris_evolution::{
    AssetState, Capsule, EvolutionPipeline, EvolutionPipelineConfig, Gene, GeneCandidate,
    PipelineContext, Selector, SelectorInput, SignalExtractorInput, StandardEvolutionPipeline,
};

/// Demo gene pool matching the demo binary's gene pool.
struct DemoGenePool {
    genes: Vec<GeneCandidate>,
}

impl DemoGenePool {
    fn new() -> Self {
        let gene_serialize = Gene {
            id: "550e8400-e29b-41d4-a716-446655440001".to_string(),
            signals: vec![
                "E0425".to_string(),
                "cannot find".to_string(),
                "Serialize".to_string(),
                "missing".to_string(),
            ],
            strategy: vec![
                "Identify the trait mentioned in the error".to_string(),
                "Add `#[derive(serde::Serialize)]`".to_string(),
                "Run cargo check".to_string(),
            ],
            validation: vec!["cargo check".to_string()],
            state: AssetState::Promoted,
            task_class_id: Some("missing-trait".to_string()),
        };

        let capsule_serialize = Capsule {
            id: "cap-0001".to_string(),
            gene_id: gene_serialize.id.clone(),
            mutation_id: "mut-0001".to_string(),
            run_id: "run-0001".to_string(),
            diff_hash: "sha256:serialize-fix".to_string(),
            confidence: 0.85,
            env: oris_evolution::EnvFingerprint {
                rustc_version: "1.77.0".to_string(),
                cargo_lock_hash: String::new(),
                target_triple: "aarch64-apple-darwin".to_string(),
                os: "macos".to_string(),
            },
            outcome: oris_evolution::Outcome {
                success: true,
                validation_profile: "test".to_string(),
                validation_duration_ms: 0,
                changed_files: vec![],
                validator_hash: String::new(),
                lines_changed: 2,
                replay_verified: false,
            },
            state: AssetState::Promoted,
        };

        Self {
            genes: vec![GeneCandidate {
                gene: gene_serialize,
                score: 0.9,
                capsules: vec![capsule_serialize],
            }],
        }
    }
}

impl Selector for DemoGenePool {
    fn select(&self, input: &SelectorInput) -> Vec<GeneCandidate> {
        // Simple signal overlap scoring - SelectorInput.signals is Vec<String>
        let mut scored: Vec<(GeneCandidate, f32)> = self
            .genes
            .iter()
            .map(|c| {
                let overlap = c
                    .gene
                    .signals
                    .iter()
                    .filter(|s| {
                        input
                            .signals
                            .iter()
                            .any(|i| i.to_lowercase().contains(&s.to_lowercase()))
                    })
                    .count() as f32;
                let score = if c.gene.signals.is_empty() {
                    0.0
                } else {
                    overlap / c.gene.signals.len() as f32
                };
                (c.clone(), score)
            })
            .filter(|(_, s)| *s > 0.0)
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored
            .into_iter()
            .take(input.limit.max(1) as usize)
            .map(|(c, _)| c)
            .collect()
    }
}

fn build_test_pipeline() -> StandardEvolutionPipeline {
    let extractor = Arc::new(RuntimeSignalExtractorAdapter::default());
    let gene_store = Arc::new(
        SqliteGeneStorePersistAdapter::open(":memory:")
            .expect("failed to open in-memory gene store"),
    );
    let selector: Arc<dyn Selector> = Arc::new(DemoGenePool::new());

    StandardEvolutionPipeline::new(EvolutionPipelineConfig::default(), selector)
        .with_signal_extractor(extractor)
        .with_gene_store(gene_store)
}

#[test]
fn test_pipeline_executes_full_evolution_cycle() {
    let pipeline = build_test_pipeline();

    let ctx = PipelineContext {
        extractor_input: Some(SignalExtractorInput {
            compiler_output: Some(
                "error[E0425]: cannot find value `serialize_fn` in this scope\n --> src/main.rs:5:5\n  |\n5 |     serialize_fn(&data);\n  |     ^^^^^^^^^^^^^^ help: did you mean...".to_string(),
            ),
            ..Default::default()
        }),
        ..Default::default()
    };

    let result = pipeline.execute(ctx).expect("pipeline execution failed");

    // Verify pipeline succeeded
    assert!(
        result.success,
        "Pipeline should succeed: {:?}",
        result.error
    );

    // Verify all stages completed
    let stage_names = [
        "detect", "select", "mutate", "execute", "validate", "evaluate", "solidify", "reuse",
    ];

    for stage_name in stage_names {
        let stage = result
            .stage_states
            .iter()
            .find(|s| s.stage_name == stage_name);

        assert!(
            stage.is_some(),
            "Stage '{}' should be present in result",
            stage_name
        );

        let stage = stage.unwrap();
        assert_eq!(
            stage.state,
            oris_evolution::PipelineStageState::Completed,
            "Stage '{}' should be Completed, got {:?}",
            stage_name,
            stage.state
        );
    }
}

#[test]
fn test_gene_selection_with_signal_overlap() {
    let gene_pool = DemoGenePool::new();

    // Test with matching signals (Vec<String>)
    let input = SelectorInput {
        signals: vec![
            "E0425".to_string(),
            "cannot find value `serialize_fn` in this scope".to_string(),
            "missing".to_string(),
        ],
        env: oris_evolution::EnvFingerprint {
            rustc_version: "1.77.0".to_string(),
            cargo_lock_hash: String::new(),
            target_triple: "aarch64-apple-darwin".to_string(),
            os: "macos".to_string(),
        },
        spec_id: None,
        limit: 10,
    };

    let candidates = gene_pool.select(&input);

    // Should select the serialize gene
    assert!(
        !candidates.is_empty(),
        "Should select at least one candidate"
    );
    assert_eq!(
        candidates[0].gene.id, "550e8400-e29b-41d4-a716-446655440001",
        "Should select the Serialize gene"
    );
    assert!(
        candidates[0].score > 0.5,
        "Score should be > 0.5 for matching signals"
    );
}

#[test]
fn test_gene_selection_with_no_overlap() {
    let gene_pool = DemoGenePool::new();

    // Test with non-matching signals
    let input = SelectorInput {
        signals: vec!["E0601".to_string(), "main function not found".to_string()],
        env: oris_evolution::EnvFingerprint {
            rustc_version: "1.77.0".to_string(),
            cargo_lock_hash: String::new(),
            target_triple: "aarch64-apple-darwin".to_string(),
            os: "macos".to_string(),
        },
        spec_id: None,
        limit: 10,
    };

    let candidates = gene_pool.select(&input);

    // Should not select any candidates (no overlap)
    assert!(
        candidates.is_empty(),
        "Should not select candidates with no signal overlap"
    );
}

#[test]
fn test_multiple_pipeline_executions() {
    let pipeline = build_test_pipeline();

    // First execution
    let ctx1 = PipelineContext {
        extractor_input: Some(SignalExtractorInput {
            compiler_output: Some(
                "error[E0425]: cannot find value `serialize_fn` in this scope".to_string(),
            ),
            ..Default::default()
        }),
        ..Default::default()
    };

    let result1 = pipeline
        .execute(ctx1)
        .expect("first pipeline execution failed");
    assert!(result1.success, "First execution should succeed");

    // Second execution with similar error
    let ctx2 = PipelineContext {
        extractor_input: Some(SignalExtractorInput {
            compiler_output: Some(
                "error[E0425]: cannot find value `serialize_data` in this scope".to_string(),
            ),
            ..Default::default()
        }),
        ..Default::default()
    };

    let result2 = pipeline
        .execute(ctx2)
        .expect("second pipeline execution failed");
    assert!(result2.success, "Second execution should succeed");

    // Both should complete all stages
    assert_eq!(
        result1.stage_states.len(),
        result2.stage_states.len(),
        "Both executions should have same number of stages"
    );
}

#[test]
fn test_pipeline_with_different_error_types() {
    let pipeline = build_test_pipeline();

    // Test with a different error type that shouldn't match
    let ctx = PipelineContext {
        extractor_input: Some(SignalExtractorInput {
            compiler_output: Some("error[E0601]: main function not found in crate".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    };

    let result = pipeline.execute(ctx).expect("pipeline execution failed");
    // Pipeline should still succeed (selector returns empty but that's valid)
    assert!(
        result.success,
        "Pipeline should succeed even with no matching genes"
    );
}
