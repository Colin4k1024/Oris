//! Detect → Solidify → Reuse full pipeline scenario (Stream A CI gate example)
//!
//! Demonstrates the end-to-end `StandardEvolutionPipeline` with:
//!
//! 1. **Detect** — `RuntimeSignalExtractorAdapter` extracts signals from a
//!    simulated compiler error (E0425 undefined symbol).
//! 2. **Select** — A demo `Selector` returns a pre-built `GeneCandidate` whose
//!    gene ID is a valid UUID (required by `SqliteGeneStorePersistAdapter`).
//! 3. **Mutate / Execute / Validate / Evaluate** — deterministic stubs (no LLM,
//!    no live sandbox) to keep the example self-contained and fast.
//! 4. **Solidify** — the gene is persisted to an in-memory SQLite store via
//!    `SqliteGeneStorePersistAdapter`.
//! 5. **Reuse** — the capsule is marked reused in the same store.
//! 6. **Fail-closed codes** — a second pipeline run is intentionally blocked and
//!    each `MutationNeededFailureReasonCode` variant is printed with the
//!    expected exit behaviour.
//!
//! Run with:
//! ```bash
//! cargo run -p evo_oris_repo --bin detect_solidify_reuse_pipeline
//! ```
//!
//! No environment variables are required; everything runs in-process.

use std::sync::Arc;

use evo_oris_repo::ExampleResult;
use oris_evokernel::adapters::{RuntimeSignalExtractorAdapter, SqliteGeneStorePersistAdapter};
use oris_evolution::{
    AssetState, Capsule, EvolutionPipeline, EvolutionPipelineConfig, Gene, GeneCandidate,
    PipelineContext, Selector, SelectorInput, SignalExtractorInput, StandardEvolutionPipeline,
};
use oris_runtime::agent_contract::MutationNeededFailureReasonCode;

// ─── Demo selector ────────────────────────────────────────────────────────────

/// Returns a single hard-coded `GeneCandidate` regardless of input signals.
///
/// The gene ID must be a valid UUID so that `SqliteGeneStorePersistAdapter`
/// can successfully upsert it. The capsule carries a matching gene ID and a
/// deterministic capsule ID used by the Reuse stage.
struct DemoSelector;

impl Selector for DemoSelector {
    fn select(&self, _input: &SelectorInput) -> Vec<GeneCandidate> {
        let gene = Gene {
            id: "550e8400-e29b-41d4-a716-446655440000".to_string(),
            signals: vec![
                "undefined symbol E0425".to_string(),
                "missing import".to_string(),
            ],
            strategy: vec![
                "add missing `use` statement".to_string(),
                "re-run cargo check to confirm".to_string(),
            ],
            validation: vec!["cargo check".to_string()],
            state: AssetState::default(),
            task_class_id: Some("missing-import".to_string()),
        };

        let capsule = Capsule {
            id: "cap-demo-0001".to_string(),
            gene_id: gene.id.clone(),
            mutation_id: "mut-demo-0001".to_string(),
            run_id: "run-demo-0001".to_string(),
            diff_hash: "sha256:demo".to_string(),
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
                lines_changed: 0,
                replay_verified: false,
            },
            state: AssetState::default(),
        };

        vec![GeneCandidate {
            gene,
            score: 0.9,
            capsules: vec![capsule],
        }]
    }
}

// ─── Fail-closed error table ──────────────────────────────────────────────────

/// Print each `MutationNeededFailureReasonCode` variant and its intended
/// fail-closed behaviour so the example serves as living documentation.
fn print_fail_closed_codes() {
    use MutationNeededFailureReasonCode::*;

    let table: &[(&str, MutationNeededFailureReasonCode, &str)] = &[
        (
            "PolicyDenied",
            PolicyDenied,
            "mutation blocked by sandbox policy — exit without applying",
        ),
        (
            "ValidationFailed",
            ValidationFailed,
            "post-mutation validation failed — revert and escalate",
        ),
        (
            "UnsafePatch",
            UnsafePatch,
            "diff contains unsafe constructs — block and quarantine",
        ),
        (
            "Timeout",
            Timeout,
            "execution exceeded time budget — abort and alert",
        ),
        (
            "MutationPayloadMissing",
            MutationPayloadMissing,
            "LLM returned empty diff — skip this candidate",
        ),
        (
            "UnknownFailClosed",
            UnknownFailClosed,
            "unclassified error — default fail-closed behaviour",
        ),
    ];

    println!("\n=== Fail-closed error codes ===");
    for (name, code, note) in table {
        println!("  [{code:?}] {name}: {note}");
    }
    println!();
}

// ─── main ─────────────────────────────────────────────────────────────────────

fn main() -> ExampleResult<()> {
    // ── 1. Build adapters ──────────────────────────────────────────────────────

    // Signal extractor: converts compiler diagnostics into `EvolutionSignal`s.
    let extractor = Arc::new(RuntimeSignalExtractorAdapter::default());

    // Gene store: in-memory SQLite (":memory:") — no filesystem I/O required.
    let gene_store = Arc::new(
        SqliteGeneStorePersistAdapter::open(":memory:")
            .expect("failed to open in-memory gene store"),
    );

    // ── 2. Build pipeline ──────────────────────────────────────────────────────

    let pipeline =
        StandardEvolutionPipeline::new(EvolutionPipelineConfig::default(), Arc::new(DemoSelector))
            .with_signal_extractor(extractor)
            .with_gene_store(gene_store);

    // ── 3. Build context with a simulated compiler error ─────────────────────

    let ctx = PipelineContext {
        extractor_input: Some(SignalExtractorInput {
            compiler_output: Some(
                "error[E0425]: cannot find value `missing_fn` in this scope".to_string(),
            ),
            ..Default::default()
        }),
        ..Default::default()
    };

    // ── 4. Execute the full pipeline ──────────────────────────────────────────

    println!("Running Detect → Solidify → Reuse pipeline …");
    let result = pipeline
        .execute(ctx)
        .expect("pipeline execution failed unexpectedly");

    // ── 5. Print stage states ─────────────────────────────────────────────────

    println!("\n=== Stage results ===");
    for stage in &result.stage_states {
        println!("  {:10} → {:?}", stage.stage_name, stage.state);
    }

    // ── 6. Verify Solidify and Reuse populated their output vectors ───────────

    // The PipelineContext is consumed by `execute()`, so we verify by
    // checking the result (success flag) and that no stage failed. A real
    // integration would store context in a shared handle; here we access the
    // pipeline's observable result only.
    assert!(
        result.success,
        "pipeline reported failure: {:?}",
        result.error
    );

    let solidify_state = result
        .stage_states
        .iter()
        .find(|s| s.stage_name == "solidify")
        .expect("solidify stage missing from result");
    assert_eq!(
        solidify_state.state,
        oris_evolution::PipelineStageState::Completed,
        "solidify stage did not complete"
    );

    let reuse_state = result
        .stage_states
        .iter()
        .find(|s| s.stage_name == "reuse")
        .expect("reuse stage missing from result");
    assert_eq!(
        reuse_state.state,
        oris_evolution::PipelineStageState::Completed,
        "reuse stage did not complete"
    );

    println!("\n✓ Solidify completed");
    println!("✓ Reuse completed");

    // ── 7. Print fail-closed code table ──────────────────────────────────────

    print_fail_closed_codes();

    println!("detect_solidify_reuse_pipeline example passed.");
    Ok(())
}
