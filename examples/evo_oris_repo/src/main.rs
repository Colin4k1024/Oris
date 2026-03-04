use std::path::PathBuf;
use std::sync::Arc;

use oris_runtime::agent_contract::{
    AgentCapabilityLevel, AgentTask, ExecutionFeedback, MutationProposal, ProposalTarget,
    ReplayFeedback,
};
use oris_runtime::evolution::{
    CommandValidator, EvoEnvFingerprint as EnvFingerprint, EvoEvolutionStore as EvolutionStore,
    EvoKernel, EvoSandboxPolicy as SandboxPolicy, EvoSelectorInput as SelectorInput,
    JsonlEvolutionStore, LocalProcessSandbox, ValidationPlan, ValidationStage,
};
use oris_runtime::governor::{DefaultGovernor, GovernorConfig};
use oris_runtime::kernel::{
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
            args: vec!["check".into(), "-p".into(), "evo_oris_repo".into()],
            timeout_ms: 180_000,
        }],
    };

    let evo = EvoKernel::new(kernel, sandbox, validator, store)
        .with_governor(Arc::new(DefaultGovernor::new(GovernorConfig {
            promote_after_successes: 1,
            ..GovernorConfig::default()
        })))
        .with_sandbox_policy(policy)
        .with_validation_plan(validation_plan);

    let base_revision = current_git_head(&workspace_root);

    let planner_task = AgentTask {
        id: "devloop-doc-sync".into(),
        description: "Add an EvoKernel example note through the proposal pipeline".into(),
    };
    let planner_capability = AgentCapabilityLevel::A2;
    let planner_target = ProposalTarget::Paths(vec!["docs/evokernel-example-generated.md".into()]);
    let planner_proposal = proposal_for(
        &planner_task,
        &planner_target,
        "planner-agent",
        "add the primary DEVLOOP note",
    );
    let planner_outcome = evo
        .capture_from_proposal(
            &"example-run".into(),
            &planner_proposal,
            proposal_diff(
                single_path(&planner_target),
                "EvoKernel Example",
                "planner-agent",
            ),
            base_revision.clone(),
        )
        .await?;
    let planner_feedback = EvoKernel::<ExampleState>::feedback_for_agent(&planner_outcome);

    let reviewer_task = AgentTask {
        id: "devloop-release-note".into(),
        description: "Add a second note from another agent source".into(),
    };
    let reviewer_capability = AgentCapabilityLevel::A1;
    let reviewer_target =
        ProposalTarget::Paths(vec!["docs/evokernel-example-review-generated.md".into()]);
    let reviewer_proposal = proposal_for(
        &reviewer_task,
        &reviewer_target,
        "review-agent",
        "add the secondary validation note",
    );
    let reviewer_outcome = evo
        .capture_from_proposal(
            &"review-run".into(),
            &reviewer_proposal,
            proposal_diff(
                single_path(&reviewer_target),
                "EvoKernel Review Example",
                "review-agent",
            ),
            base_revision,
        )
        .await?;
    let reviewer_feedback = EvoKernel::<ExampleState>::feedback_for_agent(&reviewer_outcome);
    let replay_run_id = "replay-run".to_string();

    let decision = evo
        .replay_or_fallback_for_run(
            &replay_run_id,
            SelectorInput {
                signals: planner_proposal.files.clone(),
                env: current_env_fingerprint(),
                spec_id: None,
                limit: 1,
            },
        )
        .await?;
    let replay_feedback =
        EvoKernel::<ExampleState>::replay_feedback_for_agent(&planner_proposal.files, &decision);

    print_feedback("planner-agent", &planner_capability, &planner_feedback);
    print_feedback("review-agent", &reviewer_capability, &reviewer_feedback);
    println!("captured capsule: {}", planner_outcome.capsule.id);
    print_replay_feedback(&replay_run_id, &replay_feedback);
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

fn proposal_for(
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

fn single_path(target: &ProposalTarget) -> &str {
    match target {
        ProposalTarget::WorkspaceRoot => ".",
        ProposalTarget::Paths(paths) => paths
            .first()
            .map(String::as_str)
            .unwrap_or("docs/evokernel-example-generated.md"),
    }
}

fn proposal_diff(path: &str, title: &str, source: &str) -> String {
    format!(
        "diff --git a/{path} b/{path}\nnew file mode 100644\nindex 0000000..1111111\n--- /dev/null\n+++ b/{path}\n@@ -0,0 +1,4 @@\n+# {title}\n+\n+This file is created only inside the sandbox copy.\n+Source: {source}\n"
    )
}

fn current_env_fingerprint() -> EnvFingerprint {
    EnvFingerprint {
        rustc_version: "rustc".into(),
        cargo_lock_hash: "lock".into(),
        target_triple: format!(
            "{}-unknown-{}",
            std::env::consts::ARCH,
            std::env::consts::OS
        ),
        os: std::env::consts::OS.into(),
    }
}

fn print_feedback(source: &str, capability: &AgentCapabilityLevel, feedback: &ExecutionFeedback) {
    println!(
        "{source} ({capability:?}) feedback: accepted={}, asset_state={:?}, summary={}",
        feedback.accepted, feedback.asset_state, feedback.summary
    );
}

fn print_replay_feedback(run_id: &str, feedback: &ReplayFeedback) {
    println!(
        "replay feedback: run_id={}, planner_directive={:?}, used_capsule={}, reasoning_steps_avoided={}, task_label={}, summary={}",
        run_id,
        feedback.planner_directive,
        feedback.used_capsule,
        feedback.reasoning_steps_avoided,
        feedback.task_label,
        feedback.summary
    );
}
