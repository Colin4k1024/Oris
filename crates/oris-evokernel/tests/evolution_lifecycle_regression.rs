//! Black-box regression coverage for replay determinism, sandbox boundaries,
//! governor policy, and the end-to-end EvoKernel lifecycle.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use oris_evokernel::{
    prepare_mutation, CommandValidator, EvoAssetState, EvoEnvFingerprint, EvoEvolutionStore,
    EvoKernel, EvoSandboxPolicy, EvoSelectorInput, JsonlEvolutionStore, LocalProcessSandbox,
    MutationIntent, MutationTarget, RiskLevel, ValidationPlan, ValidationStage,
};
use oris_evolution::{EvolutionEvent, PreparedMutation};
use oris_governor::{DefaultGovernor, GovernorConfig};
use oris_kernel::{
    AllowAllPolicy, InMemoryEventStore, Kernel, KernelMode, KernelState, NoopActionExecutor,
    NoopStepFn, StateUpdatedOnlyReducer,
};
use oris_sandbox::Sandbox;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct TestState;

impl KernelState for TestState {
    fn version(&self) -> u32 {
        1
    }
}

fn unique_path(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "oris-evokernel-regression-{label}-{}-{nonce}",
        std::process::id()
    ))
}

fn temp_workspace() -> PathBuf {
    let root = unique_path("workspace");
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
        profile: "regression".into(),
        stages: vec![ValidationStage::Command {
            program: "git".into(),
            args: vec!["--version".into()],
            timeout_ms: 5_000,
        }],
    }
}

fn failing_plan() -> ValidationPlan {
    ValidationPlan {
        profile: "regression-fail".into(),
        stages: vec![ValidationStage::Command {
            program: "git".into(),
            args: vec!["rev-parse".into(), "--verify".into(), "missing-ref".into()],
            timeout_ms: 5_000,
        }],
    }
}

fn sandbox_policy() -> EvoSandboxPolicy {
    EvoSandboxPolicy {
        allowed_programs: vec!["git".into()],
        max_duration_ms: 30_000,
        max_output_bytes: 1024 * 1024,
        denied_env_prefixes: Vec::new(),
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
            expected_effect: "replay should remain valid".into(),
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

fn sample_mutation_with_id(id: &str) -> PreparedMutation {
    let mut mutation = sample_mutation();
    mutation.intent.id = id.into();
    mutation
}

fn out_of_scope_mutation() -> PreparedMutation {
    prepare_mutation(
        MutationIntent {
            id: "mutation-outside".into(),
            intent: "touch manifest".into(),
            target: MutationTarget::Paths {
                allow: vec!["src".into()],
            },
            expected_effect: "should fail".into(),
            risk: RiskLevel::Low,
            signals: vec!["sandbox boundary".into()],
            spec_id: None,
        },
        "\
diff --git a/Cargo.toml b/Cargo.toml
new file mode 100644
index 0000000..1111111
--- /dev/null
+++ b/Cargo.toml
@@ -0,0 +1 @@
+[package]
"
        .into(),
        Some("HEAD".into()),
    )
}

fn replay_input(signal: &str) -> EvoSelectorInput {
    EvoSelectorInput {
        signals: vec![signal.into()],
        env: EvoEnvFingerprint {
            rustc_version: "rustc".into(),
            cargo_lock_hash: "lock".into(),
            target_triple: "x86_64-unknown-linux-gnu".into(),
            os: std::env::consts::OS.into(),
        },
        spec_id: None,
        limit: 1,
    }
}

#[tokio::test]
async fn capture_then_replay_records_full_lifecycle() {
    let workspace = temp_workspace();
    let sandbox_root = unique_path("sandbox");
    let store_root = unique_path("store");
    let store = Arc::new(JsonlEvolutionStore::new(&store_root));
    let validator = Arc::new(CommandValidator::new(sandbox_policy()));
    let sandbox = Arc::new(oris_evokernel::LocalProcessSandbox::new(
        "run-regression",
        &workspace,
        &sandbox_root,
    ));
    let evo = EvoKernel::new(test_kernel(), sandbox, validator, store.clone())
        .with_governor(Arc::new(DefaultGovernor::new(GovernorConfig {
            promote_after_successes: 1,
            ..Default::default()
        })))
        .with_sandbox_policy(sandbox_policy())
        .with_validation_plan(lightweight_plan());
    let run_id = "run-regression".to_string();

    let capsule = evo
        .capture_successful_mutation(&run_id, sample_mutation())
        .await
        .unwrap();
    let decision = evo
        .replay_or_fallback(replay_input("missing readme"))
        .await
        .unwrap();
    let events = store.scan(1).unwrap();
    let projection = store.rebuild_projection().unwrap();

    assert_eq!(capsule.state, EvoAssetState::Promoted);
    assert!(decision.used_capsule);
    assert_eq!(decision.capsule_id, Some(capsule.id.clone()));
    assert!(!decision.fallback_to_planner);
    assert!(!decision.reason.is_empty());
    assert!(events
        .iter()
        .any(|stored| matches!(stored.event, EvolutionEvent::MutationDeclared { .. })));
    assert!(events
        .iter()
        .any(|stored| matches!(stored.event, EvolutionEvent::MutationApplied { .. })));
    assert!(events
        .iter()
        .any(|stored| matches!(stored.event, EvolutionEvent::ValidationPassed { .. })));
    assert!(events
        .iter()
        .any(|stored| matches!(stored.event, EvolutionEvent::CapsuleCommitted { .. })));
    assert!(events
        .iter()
        .any(|stored| matches!(stored.event, EvolutionEvent::CapsuleReused { .. })));

    let gene = projection
        .genes
        .iter()
        .find(|gene| gene.id == capsule.gene_id)
        .unwrap();
    assert_eq!(gene.state, EvoAssetState::Promoted);

    let stored_capsule = projection
        .capsules
        .iter()
        .find(|current| current.id == capsule.id)
        .unwrap();
    assert_eq!(stored_capsule.state, EvoAssetState::Promoted);
}

#[tokio::test]
async fn replay_selection_is_deterministic_across_repeated_identical_inputs() {
    async fn run_once(label: &str) -> (String, oris_evokernel::ReplayDecision) {
        let workspace = temp_workspace();
        let sandbox_root = unique_path(&format!("{label}-sandbox"));
        let store_root = unique_path(&format!("{label}-store"));
        let store = Arc::new(JsonlEvolutionStore::new(&store_root));
        let validator = Arc::new(CommandValidator::new(sandbox_policy()));
        let sandbox = Arc::new(LocalProcessSandbox::new(
            format!("run-{label}"),
            &workspace,
            &sandbox_root,
        ));
        let evo = EvoKernel::new(test_kernel(), sandbox, validator, store)
            .with_governor(Arc::new(DefaultGovernor::new(GovernorConfig {
                promote_after_successes: 1,
                ..Default::default()
            })))
            .with_sandbox_policy(sandbox_policy())
            .with_validation_plan(lightweight_plan());

        let capsule_a = evo
            .capture_successful_mutation(
                &"run-determinism-a".to_string(),
                sample_mutation_with_id("mutation-determinism-a"),
            )
            .await
            .unwrap();
        let capsule_b = evo
            .capture_successful_mutation(
                &"run-determinism-b".to_string(),
                sample_mutation_with_id("mutation-determinism-b"),
            )
            .await
            .unwrap();

        let expected_id = std::cmp::min(capsule_a.id, capsule_b.id);
        let decision = evo
            .replay_or_fallback(replay_input("missing readme"))
            .await
            .unwrap();
        (expected_id, decision)
    }

    let (expected_a, first) = run_once("determinism-a").await;
    let (expected_b, second) = run_once("determinism-b").await;

    assert_eq!(expected_a, expected_b);
    assert!(first.used_capsule);
    assert!(second.used_capsule);
    assert_eq!(first.capsule_id, Some(expected_a.clone()));
    assert_eq!(second.capsule_id, Some(expected_b));
}

#[tokio::test]
async fn sandbox_boundary_blocks_out_of_scope_patch() {
    let workspace = temp_workspace();
    let sandbox_root = unique_path("sandbox-boundary");
    let sandbox = LocalProcessSandbox::new("run-sandbox-boundary", &workspace, &sandbox_root);

    let err = sandbox
        .apply(&out_of_scope_mutation(), &sandbox_policy())
        .await
        .unwrap_err();

    assert!(err.to_string().contains("target violation"));
}

#[tokio::test]
async fn governor_blast_radius_gate_blocks_promotion_and_replay() {
    let workspace = temp_workspace();
    let sandbox_root = unique_path("governor-candidate-sandbox");
    let store_root = unique_path("governor-candidate-store");
    let store = Arc::new(JsonlEvolutionStore::new(&store_root));
    let validator = Arc::new(CommandValidator::new(sandbox_policy()));
    let sandbox = Arc::new(LocalProcessSandbox::new(
        "run-governor-candidate",
        &workspace,
        &sandbox_root,
    ));
    let evo = EvoKernel::new(test_kernel(), sandbox, validator, store.clone())
        .with_governor(Arc::new(DefaultGovernor::new(GovernorConfig {
            promote_after_successes: 1,
            max_lines_changed: 0,
            ..Default::default()
        })))
        .with_sandbox_policy(sandbox_policy())
        .with_validation_plan(lightweight_plan());

    let outcome = evo
        .capture_mutation_with_governor(
            &"run-governor-candidate".to_string(),
            sample_mutation_with_id("mutation-governor-candidate"),
        )
        .await
        .unwrap();
    let decision = evo
        .replay_or_fallback(replay_input("missing readme"))
        .await
        .unwrap();
    let projection = store.rebuild_projection().unwrap();

    assert_eq!(
        outcome.governor_decision.target_state,
        EvoAssetState::Candidate
    );
    assert_eq!(outcome.gene.state, EvoAssetState::Candidate);
    assert_eq!(outcome.capsule.state, EvoAssetState::Candidate);
    assert!(outcome.governor_decision.reason.contains("blast radius"));
    assert!(!decision.used_capsule);
    assert!(decision.fallback_to_planner);
    assert_eq!(decision.capsule_id, None);
    assert_eq!(decision.reason, "no matching gene");

    let stored_gene = projection
        .genes
        .iter()
        .find(|gene| gene.id == outcome.gene.id)
        .unwrap();
    let stored_capsule = projection
        .capsules
        .iter()
        .find(|capsule| capsule.id == outcome.capsule.id)
        .unwrap();
    assert_eq!(stored_gene.state, EvoAssetState::Candidate);
    assert_eq!(stored_capsule.state, EvoAssetState::Candidate);
}

#[tokio::test]
async fn governor_rate_limit_blocks_rapid_successive_mutations() {
    let workspace = temp_workspace();
    let sandbox_root = unique_path("governor-rate-limit-sandbox");
    let store_root = unique_path("governor-rate-limit-store");
    let store = Arc::new(JsonlEvolutionStore::new(&store_root));
    let validator = Arc::new(CommandValidator::new(sandbox_policy()));
    let sandbox = Arc::new(LocalProcessSandbox::new(
        "run-governor-rate-limit",
        &workspace,
        &sandbox_root,
    ));
    let evo = EvoKernel::new(test_kernel(), sandbox, validator, store.clone())
        .with_governor(Arc::new(DefaultGovernor::new(GovernorConfig {
            promote_after_successes: 1,
            cooldown_secs: 0,
            max_mutations_per_window: 1,
            mutation_window_secs: 60 * 60,
            ..Default::default()
        })))
        .with_sandbox_policy(sandbox_policy())
        .with_validation_plan(lightweight_plan());

    let first = evo
        .capture_mutation_with_governor(
            &"run-governor-rate-limit".to_string(),
            sample_mutation_with_id("mutation-governor-rate-limit-1"),
        )
        .await
        .unwrap();
    let second = evo
        .capture_mutation_with_governor(
            &"run-governor-rate-limit".to_string(),
            sample_mutation_with_id("mutation-governor-rate-limit-2"),
        )
        .await
        .unwrap();

    assert_eq!(
        first.governor_decision.target_state,
        EvoAssetState::Promoted
    );
    assert_eq!(
        second.governor_decision.target_state,
        EvoAssetState::Candidate
    );
    assert!(second.governor_decision.reason.contains("rate limit"));
    assert_eq!(second.gene.state, EvoAssetState::Candidate);
    assert_eq!(second.capsule.state, EvoAssetState::Candidate);
    assert!(second.governor_decision.cooling_window.is_some());
}

#[tokio::test]
async fn governor_cooling_window_blocks_rapid_repromotion() {
    let workspace = temp_workspace();
    let sandbox_root = unique_path("governor-cooling-sandbox");
    let store_root = unique_path("governor-cooling-store");
    let store = Arc::new(JsonlEvolutionStore::new(&store_root));
    let validator = Arc::new(CommandValidator::new(sandbox_policy()));
    let sandbox = Arc::new(LocalProcessSandbox::new(
        "run-governor-cooling",
        &workspace,
        &sandbox_root,
    ));
    let evo = EvoKernel::new(test_kernel(), sandbox, validator, store)
        .with_governor(Arc::new(DefaultGovernor::new(GovernorConfig {
            promote_after_successes: 1,
            retry_cooldown_secs: 5 * 60,
            max_mutations_per_window: 100,
            ..Default::default()
        })))
        .with_sandbox_policy(sandbox_policy())
        .with_validation_plan(lightweight_plan());

    let first = evo
        .capture_mutation_with_governor(
            &"run-governor-cooling".to_string(),
            sample_mutation_with_id("mutation-governor-cooling-1"),
        )
        .await
        .unwrap();
    let second = evo
        .capture_mutation_with_governor(
            &"run-governor-cooling".to_string(),
            sample_mutation_with_id("mutation-governor-cooling-2"),
        )
        .await
        .unwrap();

    assert_eq!(
        first.governor_decision.target_state,
        EvoAssetState::Promoted
    );
    assert_eq!(
        second.governor_decision.target_state,
        EvoAssetState::Candidate
    );
    assert!(second.governor_decision.reason.contains("cooling"));
    assert!(second.governor_decision.cooling_window.is_some());
}

#[tokio::test]
async fn replay_failure_threshold_revokes_promoted_gene() {
    let workspace = temp_workspace();
    let sandbox_root = unique_path("replay-failure-sandbox");
    let store_root = unique_path("replay-failure-store");
    let store = Arc::new(JsonlEvolutionStore::new(&store_root));

    let capture_evo = EvoKernel::new(
        test_kernel(),
        Arc::new(LocalProcessSandbox::new(
            "run-replay-failure-capture",
            &workspace,
            &sandbox_root,
        )),
        Arc::new(CommandValidator::new(sandbox_policy())),
        store.clone(),
    )
    .with_governor(Arc::new(DefaultGovernor::new(GovernorConfig {
        promote_after_successes: 1,
        ..Default::default()
    })))
    .with_sandbox_policy(sandbox_policy())
    .with_validation_plan(lightweight_plan());

    let captured = capture_evo
        .capture_successful_mutation(
            &"run-replay-failure-capture".to_string(),
            sample_mutation_with_id("mutation-replay-failure"),
        )
        .await
        .unwrap();

    let replay_evo = EvoKernel::new(
        test_kernel(),
        Arc::new(LocalProcessSandbox::new(
            "run-replay-failure-replay",
            &workspace,
            &sandbox_root,
        )),
        Arc::new(CommandValidator::new(sandbox_policy())),
        store.clone(),
    )
    .with_governor(Arc::new(DefaultGovernor::new(GovernorConfig {
        promote_after_successes: 1,
        ..Default::default()
    })))
    .with_sandbox_policy(sandbox_policy())
    .with_validation_plan(failing_plan());

    let first = replay_evo
        .replay_or_fallback(replay_input("missing readme"))
        .await
        .unwrap();
    let second = replay_evo
        .replay_or_fallback(replay_input("missing readme"))
        .await
        .unwrap();
    let projection = store.rebuild_projection().unwrap();
    let events = store.scan(1).unwrap();

    assert!(!first.used_capsule);
    assert!(first.fallback_to_planner);
    assert_eq!(first.capsule_id, Some(captured.id.clone()));
    assert_eq!(first.reason, "replay validation failed");

    assert!(!second.used_capsule);
    assert!(second.fallback_to_planner);
    assert_eq!(second.capsule_id, Some(captured.id.clone()));
    assert_eq!(second.reason, "replay validation failed");

    let stored_gene = projection
        .genes
        .iter()
        .find(|gene| gene.id == captured.gene_id)
        .unwrap();
    let stored_capsule = projection
        .capsules
        .iter()
        .find(|capsule| capsule.id == captured.id)
        .unwrap();
    assert_eq!(stored_gene.state, EvoAssetState::Revoked);
    assert_eq!(stored_capsule.state, EvoAssetState::Quarantined);
    assert!(events
        .iter()
        .any(|stored| matches!(stored.event, EvolutionEvent::GeneRevoked { .. })));
}
