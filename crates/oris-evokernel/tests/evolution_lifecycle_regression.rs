use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use oris_evokernel::{
    prepare_mutation, CommandValidator, EvoAssetState, EvoEnvFingerprint, EvoEvolutionStore,
    EvoKernel, EvoSandboxPolicy, EvoSelectorInput, JsonlEvolutionStore, MutationIntent,
    MutationTarget, RiskLevel, ValidationPlan, ValidationStage,
};
use oris_evolution::{EvolutionEvent, PreparedMutation};
use oris_governor::{DefaultGovernor, GovernorConfig};
use oris_kernel::{
    AllowAllPolicy, InMemoryEventStore, Kernel, KernelMode, KernelState, NoopActionExecutor,
    NoopStepFn, StateUpdatedOnlyReducer,
};
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
