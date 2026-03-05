pub mod benchmark;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use oris_runtime::agent_contract::{AgentTask, MutationProposal, ProposalTarget};
use oris_runtime::evolution::{
    CommandValidator, EvoEnvFingerprint as EnvFingerprint, EvoEvolutionStore as EvolutionStore,
    EvoKernel, EvoSandboxPolicy as SandboxPolicy, JsonlEvolutionStore, LocalProcessSandbox,
    ValidationPlan, ValidationStage,
};
use oris_runtime::governor::{DefaultGovernor, GovernorConfig};
use oris_runtime::kernel::{
    AllowAllPolicy, InMemoryEventStore, Kernel, KernelMode, KernelState, NoopActionExecutor,
    NoopStepFn, StateUpdatedOnlyReducer,
};
use serde::{Deserialize, Serialize};

pub type ExampleResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ExampleState;

impl KernelState for ExampleState {
    fn version(&self) -> u32 {
        1
    }
}

pub fn build_demo_evo(
    label: &str,
    promote_after_successes: u64,
) -> ExampleResult<EvoKernel<ExampleState>> {
    let workspace_root = std::env::current_dir()?;
    let sandbox_root = Path::new("/tmp").join(format!("oris-evo-{label}-sandbox"));
    let store_root = std::env::temp_dir().join(format!("oris-evo-{label}-store"));

    let _ = std::fs::remove_dir_all(&sandbox_root);
    let _ = std::fs::remove_dir_all(&store_root);
    std::fs::create_dir_all(&sandbox_root)?;
    std::fs::create_dir_all(&store_root)?;

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

    let policy = demo_sandbox_policy();
    let validator = Arc::new(CommandValidator::new(policy.clone()));
    let sandbox = Arc::new(LocalProcessSandbox::new(
        label,
        &workspace_root,
        sandbox_root.as_path(),
    ));
    let store: Arc<dyn EvolutionStore> = Arc::new(JsonlEvolutionStore::new(store_root));

    let mut governor_config = GovernorConfig::default();
    governor_config.promote_after_successes = promote_after_successes;

    Ok(EvoKernel::new(kernel, sandbox, validator, store)
        .with_governor(Arc::new(DefaultGovernor::new(governor_config)))
        .with_sandbox_policy(policy)
        .with_validation_plan(demo_validation_plan(label)))
}

pub fn demo_sandbox_policy() -> SandboxPolicy {
    SandboxPolicy {
        allowed_programs: vec!["cargo".into(), "git".into()],
        max_duration_ms: 180_000,
        max_output_bytes: 1_048_576,
        denied_env_prefixes: vec!["TOKEN".into(), "KEY".into(), "SECRET".into()],
    }
}

pub fn demo_validation_plan(profile: &str) -> ValidationPlan {
    ValidationPlan {
        profile: format!("{profile}-validation"),
        stages: vec![ValidationStage::Command {
            program: "cargo".into(),
            args: vec!["check".into(), "-p".into(), "evo_oris_repo".into()],
            timeout_ms: 180_000,
        }],
    }
}

pub fn current_git_head(workspace_root: &Path) -> Option<String> {
    std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(workspace_root)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn current_env_fingerprint() -> EnvFingerprint {
    let workspace_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let cargo_lock_hash = std::fs::read(workspace_root.join("Cargo.lock"))
        .map(|bytes| format!("cargo-lock-bytes-{}", bytes.len()))
        .unwrap_or_else(|_| "missing-cargo-lock".into());

    EnvFingerprint {
        rustc_version: "rustc".into(),
        cargo_lock_hash,
        target_triple: format!(
            "{}-unknown-{}",
            std::env::consts::ARCH,
            std::env::consts::OS
        ),
        os: std::env::consts::OS.into(),
    }
}

pub fn proposal_for(
    task: &AgentTask,
    target: &ProposalTarget,
    source: &str,
    expected_effect: &str,
) -> MutationProposal {
    MutationProposal {
        intent: format!("{} ({source})", task.description),
        files: target_paths(target),
        expected_effect: expected_effect.into(),
    }
}

fn target_paths(target: &ProposalTarget) -> Vec<String> {
    match target {
        ProposalTarget::WorkspaceRoot => vec![".".into()],
        ProposalTarget::Paths(paths) => paths.clone(),
    }
}

pub fn single_path(target: &ProposalTarget) -> &str {
    match target {
        ProposalTarget::WorkspaceRoot => ".",
        ProposalTarget::Paths(paths) => paths
            .first()
            .map(String::as_str)
            .unwrap_or("docs/evokernel-example-generated.md"),
    }
}

pub fn proposal_diff(path: &str, title: &str, source: &str) -> String {
    format!(
        "diff --git a/{path} b/{path}\nnew file mode 100644\nindex 0000000..1111111\n--- /dev/null\n+++ b/{path}\n@@ -0,0 +1,4 @@\n+# {title}\n+\n+This file is created only inside the sandbox copy.\n+Source: {source}\n"
    )
}
