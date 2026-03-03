use std::path::PathBuf;
use std::sync::Arc;

use oris_evokernel::{
    prepare_mutation, CommandValidator, EvoKernel, EvoSandboxPolicy as SandboxPolicy,
    LocalProcessSandbox, MutationIntent, MutationTarget, RiskLevel, ValidationPlan,
    ValidationStage,
};
use oris_evolution::{EnvFingerprint, EvolutionStore, JsonlEvolutionStore, SelectorInput};
use oris_kernel::{
    AllowAllPolicy, InMemoryEventStore, Kernel, KernelMode, KernelState, NoopActionExecutor,
    NoopStepFn, StateUpdatedOnlyReducer,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct ExampleState;

impl KernelState for ExampleState {
    fn version(&self) -> u32 {
        1
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let workspace_root = std::env::current_dir()?;
    let kernel = Arc::new(Kernel::<ExampleState> {
        events: Box::new(InMemoryEventStore::new()),
        snaps: None,
        reducer: Box::new(StateUpdatedOnlyReducer),
        exec: Box::new(NoopActionExecutor),
        step: Box::new(NoopStepFn),
        policy: Box::new(AllowAllPolicy),
        effect_sink: None,
        mode: KernelMode::Normal,
    });

    let policy = SandboxPolicy {
        allowed_programs: vec!["cargo".into(), "git".into()],
        max_duration_ms: 180_000,
        max_output_bytes: 1_048_576,
        denied_env_prefixes: vec!["TOKEN".into(), "KEY".into(), "SECRET".into()],
    };
    let validator = Arc::new(CommandValidator::new(policy.clone()));
    let sandbox = Arc::new(LocalProcessSandbox::new(
        "example-run",
        &workspace_root,
        "/tmp/oris-evo",
    ));
    let store_root = std::env::temp_dir().join("oris-evo-example-store");
    let store: Arc<dyn EvolutionStore> = Arc::new(JsonlEvolutionStore::new(store_root));

    let validation_plan = ValidationPlan {
        profile: "example".into(),
        stages: vec![ValidationStage::Command {
            program: "cargo".into(),
            args: vec!["check".into(), "--workspace".into()],
            timeout_ms: 180_000,
        }],
    };

    let evo = EvoKernel::new(kernel, sandbox, validator, store)
        .with_sandbox_policy(policy)
        .with_validation_plan(validation_plan);

    let base_revision = current_git_head(&workspace_root);
    let mutation = prepare_mutation(
        MutationIntent {
            id: "example-doc-mutation".into(),
            intent: "add evo kernel example note".into(),
            target: MutationTarget::Paths {
                allow: vec!["docs".into()],
            },
            expected_effect: "workspace still compiles".into(),
            risk: RiskLevel::Low,
            signals: vec!["evokernel example".into()],
            spec_id: None,
        },
        "\
diff --git a/docs/evokernel-example-generated.md b/docs/evokernel-example-generated.md
new file mode 100644
index 0000000..1111111
--- /dev/null
+++ b/docs/evokernel-example-generated.md
@@ -0,0 +1,3 @@
++# EvoKernel Example
++
+This file is created only inside the sandbox copy.
"
        .into(),
        base_revision,
    );

    let capsule = evo
        .capture_successful_mutation(&"example-run".into(), mutation)
        .await?;
    let decision = evo
        .replay_or_fallback(SelectorInput {
            signals: vec!["evokernel example".into()],
            env: EnvFingerprint {
                rustc_version: "rustc".into(),
                cargo_lock_hash: "lock".into(),
                target_triple: format!(
                    "{}-unknown-{}",
                    std::env::consts::ARCH,
                    std::env::consts::OS
                ),
                os: std::env::consts::OS.into(),
            },
            spec_id: None,
            limit: 1,
        })
        .await?;

    println!("captured capsule: {}", capsule.id);
    println!(
        "replay decision: used_capsule={}, reason={}",
        decision.used_capsule, decision.reason
    );
    Ok(())
}

fn current_git_head(workspace_root: &PathBuf) -> Option<String> {
    std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(workspace_root)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
}
