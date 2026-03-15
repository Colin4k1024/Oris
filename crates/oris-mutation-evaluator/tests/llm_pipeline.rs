//! Integration tests for the LLM mutation → evaluate pipeline.
//!
//! All tests use `MockMutationBackend` and `MockCritic` so they run without
//! any LLM API key.

use oris_mutation_evaluator::{
    critic::MockCritic,
    evaluator::MutationEvaluator,
    mutation_backend::{
        EnvRoutedBackend, LlmMutationBackend, MockMutationBackend, MutationRequest,
        ProposalContract,
    },
    types::{EvoSignal, SignalKind, Verdict},
};
use uuid::Uuid;

fn panic_signal() -> EvoSignal {
    EvoSignal {
        kind: SignalKind::Panic,
        message: "index out of bounds: the len is 0".into(),
        location: Some("src/lib.rs:42".into()),
    }
}

fn build_request(code: &str) -> MutationRequest {
    MutationRequest {
        run_id: Uuid::new_v4().to_string(),
        signals: vec![panic_signal()],
        context_code: code.into(),
        gene_hint_id: None,
    }
}

// ---------------------------------------------------------------------------
// AC 1: LLM can generate correctly formatted mutation proposals
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mock_backend_proposal_passes_contract() {
    let backend = MockMutationBackend::passing();
    let req = build_request("fn process(v: &[u32]) -> u32 { v[0] }");
    let proposal = backend.generate(&req).await.unwrap();
    // Every generated proposal must satisfy the structural contract.
    ProposalContract::validate(&proposal).expect("proposal should satisfy contract");
}

#[tokio::test]
async fn test_custom_mock_backend_proposal_passes_contract() {
    let backend = MockMutationBackend::new(
        "Return Option instead of panicking on empty slice",
        "fn process(v: &[u32]) -> Option<u32> { v.first().copied() }",
    );
    let req = build_request("fn process(v: &[u32]) -> u32 { v[0] }");
    let proposal = backend.generate(&req).await.unwrap();
    ProposalContract::validate(&proposal).expect("custom proposal should satisfy contract");
}

// ---------------------------------------------------------------------------
// AC 2: LLM evaluation outputs a confidence score — static + LLM dual-track
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mutation_evaluate_pipeline_passing_critic() {
    // Generate via mock backend, then evaluate with passing MockCritic.
    let backend = MockMutationBackend::passing();
    let req = build_request("fn process(v: &[u32]) -> u32 { v[0] }");
    let proposal = backend.generate(&req).await.unwrap();

    ProposalContract::validate(&proposal).unwrap();

    let evaluator = MutationEvaluator::new(MockCritic::passing());
    let report = evaluator.evaluate(&proposal).await.unwrap();

    // Passing critic → composite score should be well above APPLY_THRESHOLD
    assert!(
        report.composite_score > 0.45,
        "composite score too low: {}",
        report.composite_score
    );
    assert_ne!(
        report.verdict,
        Verdict::Reject,
        "should not reject a passing mutation"
    );
}

#[tokio::test]
async fn test_mutation_evaluate_pipeline_failing_critic() {
    let backend = MockMutationBackend::passing();
    let req = build_request("fn process(v: &[u32]) -> u32 { v[0] }");
    let proposal = backend.generate(&req).await.unwrap();

    let evaluator = MutationEvaluator::new(MockCritic::failing());
    let report = evaluator.evaluate(&proposal).await.unwrap();

    assert_eq!(
        report.verdict,
        Verdict::Reject,
        "failing critic scores should produce Reject"
    );
}

// ---------------------------------------------------------------------------
// AC 3: Env-var backend switching
// ---------------------------------------------------------------------------

#[test]
fn test_env_routed_no_keys_uses_mock_fallback() {
    std::env::remove_var("ANTHROPIC_API_KEY");
    std::env::remove_var("OPENAI_API_KEY");
    std::env::remove_var("OLLAMA_HOST");

    let backend = EnvRoutedBackend::from_env();
    assert_eq!(backend.selected_provider(), "mock-fallback");
}

#[tokio::test]
async fn test_env_routed_mock_fallback_generates_valid_proposal() {
    std::env::remove_var("ANTHROPIC_API_KEY");
    std::env::remove_var("OPENAI_API_KEY");
    std::env::remove_var("OLLAMA_HOST");

    let backend = EnvRoutedBackend::from_env();
    let req = build_request("fn boom() { panic!(\"oops\") }");
    let proposal = backend.generate(&req).await.unwrap();
    ProposalContract::validate(&proposal).expect("fallback proposal must pass contract");
}

#[test]
fn test_env_routed_with_custom_backend() {
    let inner = MockMutationBackend::new("Custom intent", "fn fixed() {}");
    let backend = EnvRoutedBackend::with_backend(inner);
    assert_eq!(backend.selected_provider(), "mock");
}

// ---------------------------------------------------------------------------
// AC 4: Full chain: mutation → contract validate → static analysis → evaluate
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_full_mutation_validate_evaluate_chain() {
    // Step 1: generate
    let backend = MockMutationBackend::passing();
    let req = build_request("fn get_first(v: &[u32]) -> u32 { v[0] }");
    let proposal = backend.generate(&req).await.unwrap();

    // Step 2: validate contract
    ProposalContract::validate(&proposal).expect("proposal must pass contract");

    // Step 3: evaluate (static analysis + LLM critic)
    let evaluator = MutationEvaluator::new(MockCritic::passing());
    let report = evaluator.evaluate(&proposal).await.unwrap();

    // Chain must produce a non-error verdict
    assert!(
        matches!(report.verdict, Verdict::Promote | Verdict::ApplyOnly),
        "chain should not reject a clean passing mutation, got {:?}",
        report.verdict
    );
    assert_eq!(report.proposal_id, proposal.id);
}

#[tokio::test]
async fn test_full_chain_no_op_is_rejected_by_static_analysis() {
    // When original == proposed, static analysis should block before LLM.
    let original = "fn f() -> u32 { 1 }";
    let backend = MockMutationBackend::new("no change", original); // proposed == original
    let req = MutationRequest {
        run_id: "run-noop".into(),
        signals: vec![panic_signal()],
        context_code: original.into(),
        gene_hint_id: None,
    };
    let proposal = backend.generate(&req).await.unwrap();

    // Contract should flag this as a no-op
    let contract_result = ProposalContract::validate(&proposal);
    assert!(
        contract_result.is_err(),
        "no-op proposal must fail contract validation"
    );
    let violations = contract_result.unwrap_err();
    assert!(
        violations.iter().any(|v| v.field == "proposed"),
        "contract violation should mention 'proposed'"
    );
}
