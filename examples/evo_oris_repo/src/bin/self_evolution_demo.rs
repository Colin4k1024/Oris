//! Self-Evolution Case Study Demo
//!
//! Demonstrates the complete evolution pipeline with a realistic Rust
//! compilation error scenario: agent encounters missing `use` statements,
//! the system detects signals, selects a gene, applies a mutation, validates
//! the fix, and solidifies the successful pattern into the gene pool.
//!
//! Run with:
//! ```bash
//! cargo run -p evo_oris_repo --bin self_evolution_demo
//! ```
//!
//! This demo showcases:
//! - Signal extraction from compiler errors (E0425)
//! - Gene selection based on signal overlap
//! - Full pipeline execution with visualization
//! - Confidence scoring and time-based decay

use std::sync::Arc;

use evo_oris_repo::visualization::{
    print_phase_header, render_confidence_gauge, render_evolution_cycle, render_gene_pool,
    render_stage_summary,
};
use evo_oris_repo::ExampleResult;
use oris_evokernel::adapters::{RuntimeSignalExtractorAdapter, SqliteGeneStorePersistAdapter};
use oris_evolution::{
    AssetState, Capsule, EvolutionPipeline, EvolutionPipelineConfig, Gene, GeneCandidate,
    PipelineContext, Selector, SelectorInput, SignalExtractorInput, StandardConfidenceScheduler,
    StandardEvolutionPipeline, MIN_REPLAY_CONFIDENCE,
};

// ─── Demo Gene Pool ─────────────────────────────────────────────────────────────

/// A gene pool with realistic genes for fixing missing trait imports.
struct DemoGenePool {
    genes: Vec<GeneCandidate>,
}

impl DemoGenePool {
    fn new() -> Self {
        // Gene 1: Fix missing Serialize derive
        let gene_serialize = Gene {
            id: "550e8400-e29b-41d4-a716-446655440001".to_string(),
            signals: vec![
                "E0425".to_string(),
                "cannot find".to_string(),
                "Serialize".to_string(),
                "missing".to_string(),
            ],
            strategy: vec![
                "Identify the trait mentioned in the error (Serialize, Deserialize, etc.)"
                    .to_string(),
                "Add `#[derive(serde::Serialize)]` or `use serde::Serialize;`".to_string(),
                "Run cargo check to verify fix".to_string(),
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
                validation_profile: "demo".to_string(),
                validation_duration_ms: 0,
                changed_files: vec![],
                validator_hash: String::new(),
                lines_changed: 2,
                replay_verified: false,
            },
            state: AssetState::Promoted,
        };

        // Gene 2: Fix missing use statement for std traits
        let gene_std = Gene {
            id: "550e8400-e29b-41d4-a716-446655440002".to_string(),
            signals: vec![
                "E0437".to_string(),
                "type does not implement".to_string(),
                "missing".to_string(),
            ],
            strategy: vec![
                "Identify the trait the type is missing".to_string(),
                "Add `use std::trait::TraitName;`".to_string(),
                "Run cargo check to verify fix".to_string(),
            ],
            validation: vec!["cargo check".to_string()],
            state: AssetState::Promoted,
            task_class_id: Some("missing-trait".to_string()),
        };

        let capsule_std = Capsule {
            id: "cap-0002".to_string(),
            gene_id: gene_std.id.clone(),
            mutation_id: "mut-0002".to_string(),
            run_id: "run-0002".to_string(),
            diff_hash: "sha256:std-trait-fix".to_string(),
            confidence: 0.72,
            env: oris_evolution::EnvFingerprint {
                rustc_version: "1.77.0".to_string(),
                cargo_lock_hash: String::new(),
                target_triple: "aarch64-apple-darwin".to_string(),
                os: "macos".to_string(),
            },
            outcome: oris_evolution::Outcome {
                success: true,
                validation_profile: "demo".to_string(),
                validation_duration_ms: 0,
                changed_files: vec![],
                validator_hash: String::new(),
                lines_changed: 1,
                replay_verified: false,
            },
            state: AssetState::Promoted,
        };

        Self {
            genes: vec![
                GeneCandidate {
                    gene: gene_serialize,
                    score: 0.9,
                    capsules: vec![capsule_serialize],
                },
                GeneCandidate {
                    gene: gene_std,
                    score: 0.75,
                    capsules: vec![capsule_std],
                },
            ],
        }
    }
}

impl Selector for DemoGenePool {
    fn select(&self, input: &SelectorInput) -> Vec<GeneCandidate> {
        // Simple signal overlap scoring
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

// ─── Main Demo ─────────────────────────────────────────────────────────────────

fn main() -> ExampleResult<()> {
    println!("\n");
    println!("╔═══════════════════════════════════════════════════════════════════╗");
    println!("║                                                                   ║");
    println!("║         ORIS SELF-EVOLUTION CASE STUDY DEMO                      ║");
    println!("║                                                                   ║");
    println!("║   Problem: Fix Missing Trait Derives in Rust                     ║");
    println!("║   Agent encounters E0425: cannot find value `serialize_fn`        ║");
    println!("║                                                                   ║");
    println!("╚═══════════════════════════════════════════════════════════════════╝");

    // ── Phase 1: First Problem ───────────────────────────────────────────────

    print_phase_header(
        1,
        "FIRST COMPILATION ERROR",
        "Agent encounters E0425: cannot find value `serialize_fn` in this scope",
    );

    println!("Setting up evolution pipeline with gene pool...");

    let extractor = Arc::new(RuntimeSignalExtractorAdapter::default());
    let gene_store = Arc::new(
        SqliteGeneStorePersistAdapter::open(":memory:")
            .expect("failed to open in-memory gene store"),
    );
    let selector = Arc::new(DemoGenePool::new());

    let pipeline = StandardEvolutionPipeline::new(EvolutionPipelineConfig::default(), selector)
        .with_signal_extractor(extractor)
        .with_gene_store(gene_store.clone());

    println!("Pipeline configured. Executing...\n");

    let ctx1 = PipelineContext {
        extractor_input: Some(SignalExtractorInput {
            compiler_output: Some(
                "error[E0425]: cannot find value `serialize_fn` in this scope\n --> src/main.rs:5:5\n  |\n5 |     serialize_fn(&data);\n  |     ^^^^^^^^^^^^^^ help: did you mean...".to_string(),
            ),
            ..Default::default()
        }),
        ..Default::default()
    };

    let result1 = pipeline
        .execute(ctx1)
        .expect("pipeline execution failed unexpectedly");

    println!("{}", render_evolution_cycle(&result1, Some("solidify")));
    println!("{}", render_stage_summary(&result1));

    if result1.success {
        println!("  ✓ Pipeline completed successfully!");
    } else {
        println!("  ✗ Pipeline failed: {:?}", result1.error);
    }

    if let Some(ref task_class) = result1.inferred_task_class_id {
        println!("  Inferred Task Class: {}", task_class);
    }

    // ── Phase 2: Gene Reuse ─────────────────────────────────────────────────

    print_phase_header(
        2,
        "GENE REUSE",
        "Similar error - should reuse gene from Phase 1 with confidence boost",
    );

    println!("Pipeline configured. Executing with similar error...\n");

    let ctx2 = PipelineContext {
        extractor_input: Some(SignalExtractorInput {
            compiler_output: Some(
                "error[E0425]: cannot find value `serialize_data` in this scope\n --> src/main.rs:12:5\n  |\n12 |     serialize_data(&user);\n  |     ^^^^^^^^^^^^^^^".to_string(),
            ),
            ..Default::default()
        }),
        ..Default::default()
    };

    let result2 = pipeline
        .execute(ctx2)
        .expect("pipeline execution failed unexpectedly");

    println!("{}", render_evolution_cycle(&result2, Some("reuse")));
    println!("{}", render_stage_summary(&result2));

    if result2.success {
        println!("  ✓ Gene successfully reused from Phase 1!");
    } else {
        println!("  ✗ Pipeline failed: {:?}", result2.error);
    }

    // ── Phase 3: Confidence Lifecycle ───────────────────────────────────────

    print_phase_header(
        3,
        "CONFIDENCE LIFECYCLE",
        "Tracking gene confidence over successful reuses",
    );

    println!("Gene confidence tracking after successful reuses:");
    println!("\n  Initial confidence: 0.85");
    println!("  After reuse 1: +0.10 boost (capped at 1.0)");
    println!("  After reuse 2: +0.10 boost (already at 1.0)");
    println!("  Final expected: 1.0 (capped)");

    // Demonstrate initial confidence
    let initial_confidence = 0.85f32;
    let after_reuse_1 = (initial_confidence + 0.10).min(1.0);
    let after_reuse_2 = (after_reuse_1 + 0.10).min(1.0);

    println!(
        "{}",
        render_confidence_gauge("550e8400-...440001", initial_confidence, 40)
    );
    println!("  → After reuse 1: +0.10");
    println!(
        "{}",
        render_confidence_gauge("550e8400-...440001", after_reuse_1, 40)
    );
    println!("  → After reuse 2: +0.10 (capped)");
    println!(
        "{}",
        render_confidence_gauge("550e8400-...440001", after_reuse_2, 40)
    );

    // ── Phase 4: Time-Based Decay ──────────────────────────────────────────

    print_phase_header(
        4,
        "TIME-BASED DECAY",
        "Confidence decays when genes are not reused",
    );

    const DECAY_RATE_PER_HOUR: f32 = 0.05;
    let hours_until_stale =
        (initial_confidence.ln() - MIN_REPLAY_CONFIDENCE.ln()) / DECAY_RATE_PER_HOUR;

    println!(
        "Confidence decays at REPLAY_CONFIDENCE_DECAY_RATE_PER_HOUR = {:.2}",
        DECAY_RATE_PER_HOUR
    );
    println!("MIN_REPLAY_CONFIDENCE = {:.2}", MIN_REPLAY_CONFIDENCE);
    println!();

    // Show decay over time
    let decay_points = [
        (0, initial_confidence, "initial"),
        (
            6,
            StandardConfidenceScheduler::calculate_decay(initial_confidence, 6.0),
            "6 hours",
        ),
        (
            12,
            StandardConfidenceScheduler::calculate_decay(initial_confidence, 12.0),
            "12 hours",
        ),
        (
            24,
            StandardConfidenceScheduler::calculate_decay(initial_confidence, 24.0),
            "24 hours",
        ),
        (
            48,
            StandardConfidenceScheduler::calculate_decay(initial_confidence, 48.0),
            "48 hours",
        ),
    ];

    println!("  Time        Confidence    State");
    println!("  ──────────  ────────────  ─────────────");

    for (hours, conf, _label) in decay_points {
        let state = if conf < MIN_REPLAY_CONFIDENCE {
            "RE-EVOLVE NEEDED"
        } else {
            "reusable"
        };
        println!("  {:>6}h      {:>9.3}     {}", hours, conf, state);
    }

    println!();
    println!(
        "  Gene becomes stale after ~{:.0} hours without reuse",
        hours_until_stale
    );

    // ── Phase 5: Gene Pool Summary ───────────────────────────────────────────

    print_phase_header(
        5,
        "GENE POOL",
        "Summary of genes available for future reuse",
    );

    let gene_summary = vec![
        ("550e8400-...440001 (Serialize)", 0.95, 3),
        ("550e8400-...440002 (std traits)", 0.82, 1),
    ];

    println!("{}", render_gene_pool(&gene_summary));

    // ── Final Summary ───────────────────────────────────────────────────────

    println!("\n");
    println!("╔═══════════════════════════════════════════════════════════════════╗");
    println!("║                   DEMO COMPLETE                                 ║");
    println!("╠═══════════════════════════════════════════════════════════════════╣");
    println!("║  Demonstrated:                                                  ║");
    println!("║  • Signal extraction from E0425 compiler errors                 ║");
    println!("║  • Gene selection based on signal overlap scoring               ║");
    println!("║  • Full pipeline: Detect → Select → Mutate → Execute →         ║");
    println!("║                    Validate → Evaluate → Solidify → Reuse       ║");
    println!("║  • Confidence boosting on successful reuse                     ║");
    println!("║  • Time-based decay for stale genes                            ║");
    println!("║  • Gene pool tracking                                          ║");
    println!("╚═══════════════════════════════════════════════════════════════════╝\n");

    Ok(())
}
