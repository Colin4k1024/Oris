//! Black-box regression coverage for replay determinism, sandbox boundaries,
//! governor policy, and the end-to-end EvoKernel lifecycle.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{Duration, Utc};
use oris_agent_contract::{
    AgentTask, BoundedTaskClass, HumanApproval, MutationProposal, SupervisedDevloopRequest,
    SupervisedDevloopStatus,
};
use oris_evokernel::{
    extract_deterministic_signals, prepare_mutation, CommandValidator, EvoAssetState,
    EvoEnvFingerprint, EvoEvolutionStore, EvoKernel, EvoSandboxPolicy, EvoSelectorInput,
    JsonlEvolutionStore, LocalProcessSandbox, MutationIntent, MutationTarget, RiskLevel,
    SignalExtractionInput, ValidationPlan, ValidationStage,
};
use oris_evolution::{
    compute_artifact_hash, stable_hash_json, EvolutionEvent, PreparedMutation, StoredEvolutionEvent,
};
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

fn devloop_request(task_id: &str, file: &str, approved: bool) -> SupervisedDevloopRequest {
    SupervisedDevloopRequest {
        task: AgentTask {
            id: task_id.into(),
            description: format!("Update {file} through the supervised DEVLOOP"),
        },
        proposal: MutationProposal {
            intent: format!("Update {file}"),
            files: vec![file.into()],
            expected_effect: format!("Keep {file} in sync"),
        },
        approval: HumanApproval {
            approved,
            approver: if approved {
                Some("maintainer".into())
            } else {
                None
            },
            note: Some("regression test".into()),
        },
    }
}

fn proposal_diff_for(path: &str, title: &str) -> String {
    format!(
        "diff --git a/{path} b/{path}\nnew file mode 100644\nindex 0000000..1111111\n--- /dev/null\n+++ b/{path}\n@@ -0,0 +1,2 @@\n+# {title}\n+generated by supervised devloop\n"
    )
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

fn replay_input(signal: &str, workspace: &std::path::Path) -> EvoSelectorInput {
    let rustc_version = std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_else(|| "rustc unknown".into());
    let cargo_lock_hash = std::fs::read(workspace.join("Cargo.lock"))
        .ok()
        .map(|bytes| compute_artifact_hash(&String::from_utf8_lossy(&bytes)))
        .unwrap_or_else(|| "missing-cargo-lock".into());
    EvoSelectorInput {
        signals: vec![signal.into()],
        env: EvoEnvFingerprint {
            rustc_version,
            cargo_lock_hash,
            target_triple: format!(
                "{}-unknown-{}",
                std::env::consts::ARCH,
                std::env::consts::OS
            ),
            os: std::env::consts::OS.into(),
        },
        spec_id: None,
        limit: 1,
    }
}

fn test_evo(label: &str) -> (PathBuf, Arc<JsonlEvolutionStore>, EvoKernel<TestState>) {
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
    let evo = EvoKernel::new(test_kernel(), sandbox, validator, store.clone())
        .with_governor(Arc::new(DefaultGovernor::new(GovernorConfig {
            promote_after_successes: 1,
            ..Default::default()
        })))
        .with_sandbox_policy(sandbox_policy())
        .with_validation_plan(lightweight_plan());
    (workspace, store, evo)
}

fn test_evo_with_store(
    label: &str,
    store: Arc<JsonlEvolutionStore>,
) -> (PathBuf, EvoKernel<TestState>) {
    let workspace = temp_workspace();
    let sandbox_root = unique_path(&format!("{label}-sandbox"));
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
    (workspace, evo)
}

fn backdate_store_events(store: &JsonlEvolutionStore, age: Duration) {
    let events_path = store.root_dir().join("events.jsonl");
    let contents = fs::read_to_string(&events_path).unwrap();
    let mut events = contents
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str::<StoredEvolutionEvent>(line).unwrap())
        .collect::<Vec<_>>();
    let timestamp = (Utc::now() - age).to_rfc3339();
    let mut prev_hash = String::new();
    for event in &mut events {
        event.timestamp = timestamp.clone();
        event.prev_hash = prev_hash.clone();
        event.record_hash =
            stable_hash_json(&(event.seq, &event.timestamp, &event.prev_hash, &event.event))
                .unwrap();
        prev_hash = event.record_hash.clone();
    }
    let mut serialized = events
        .into_iter()
        .map(|event| serde_json::to_string(&event).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    if !serialized.is_empty() {
        serialized.push('\n');
    }
    fs::write(events_path, serialized).unwrap();
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
        .replay_or_fallback(replay_input("missing readme", &workspace))
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
        .any(|stored| matches!(stored.event, EvolutionEvent::SignalsExtracted { .. })));
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
            .replay_or_fallback(replay_input("missing readme", &workspace))
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

#[test]
fn deterministic_signal_extraction_is_stable() {
    let input = SignalExtractionInput {
        patch_diff: "\
diff --git a/src/lib.rs b/src/lib.rs
+++ b/src/lib.rs
@@
+fn example() {}
"
        .into(),
        intent: "Fix missing README handling".into(),
        expected_effect: "Eliminate E0425 errors in tests".into(),
        declared_signals: vec!["missing readme".into(), "E0425".into()],
        changed_files: vec!["src/lib.rs".into(), "README.md".into()],
        validation_success: true,
        validation_logs: "error[E0425]: cannot find value `README` in this scope".into(),
        stage_outputs: vec![
            "stack trace mentions README loader".into(),
            "performance telemetry stable".into(),
        ],
    };

    let first = extract_deterministic_signals(&input);
    let second = extract_deterministic_signals(&input);

    assert_eq!(first.values, second.values);
    assert_eq!(first.hash, second.hash);
    assert!(first.values.contains(&"missing readme".to_string()));
    assert!(first.values.contains(&"E0425".to_string()));
}

#[tokio::test]
async fn local_selector_query_returns_captured_gene() {
    let workspace = temp_workspace();
    let sandbox_root = unique_path("selector-query-sandbox");
    let store_root = unique_path("selector-query-store");
    let store = Arc::new(JsonlEvolutionStore::new(&store_root));
    let validator = Arc::new(CommandValidator::new(sandbox_policy()));
    let sandbox = Arc::new(LocalProcessSandbox::new(
        "run-selector-query",
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

    let captured = evo
        .capture_successful_mutation(
            &"run-selector-query".to_string(),
            sample_mutation_with_id("mutation-selector-query"),
        )
        .await
        .unwrap();
    let candidates = evo.select_candidates(&replay_input("missing readme", &workspace));

    assert!(!candidates.is_empty());
    assert_eq!(candidates[0].gene.id, captured.gene_id);
    assert!(candidates[0]
        .capsules
        .iter()
        .any(|capsule| capsule.id == captured.id));
}

#[tokio::test]
async fn single_task_learns_and_replays_on_second_run() {
    let (workspace, store, evo) = test_evo("self-evolve-second-run");

    let cold_start = evo
        .replay_or_fallback(replay_input("missing readme", &workspace))
        .await
        .unwrap();
    let captured = evo
        .capture_successful_mutation(
            &"run-self-evolve-second-run".to_string(),
            sample_mutation_with_id("mutation-self-evolve-second-run"),
        )
        .await
        .unwrap();
    let learned = evo
        .replay_or_fallback(replay_input("missing readme", &workspace))
        .await
        .unwrap();
    let events = store.scan(1).unwrap();
    let metrics = evo.metrics_snapshot().unwrap();

    assert!(!cold_start.used_capsule);
    assert!(cold_start.fallback_to_planner);
    assert_eq!(cold_start.capsule_id, None);
    assert_eq!(cold_start.reason, "no matching gene");

    assert!(learned.used_capsule);
    assert!(!learned.fallback_to_planner);
    assert_eq!(learned.capsule_id, Some(captured.id.clone()));
    assert_eq!(metrics.replay_success_total, 1);
    assert_eq!(
        events
            .iter()
            .filter(|stored| matches!(
                &stored.event,
                EvolutionEvent::CapsuleReused { capsule_id, .. } if capsule_id == &captured.id
            ))
            .count(),
        1
    );
}

#[tokio::test]
async fn supervised_devloop_executes_bounded_docs_task_after_approval() {
    let (_workspace, store, evo) = test_evo("supervised-devloop-approved");
    let request = devloop_request("task-docs-approved", "docs/supervised-devloop.md", true);

    let outcome = evo
        .run_supervised_devloop(
            &"run-supervised-devloop-approved".to_string(),
            &request,
            proposal_diff_for("docs/supervised-devloop.md", "Supervised DEVLOOP"),
            None,
        )
        .await
        .unwrap();

    assert_eq!(outcome.status, SupervisedDevloopStatus::Executed);
    assert_eq!(outcome.task_class, Some(BoundedTaskClass::DocsSingleFile));
    assert!(outcome.execution_feedback.is_some());
    assert!(outcome.summary.contains("executed"));
    assert!(store
        .scan(1)
        .unwrap()
        .iter()
        .any(|stored| matches!(stored.event, EvolutionEvent::MutationDeclared { .. })));
}

#[tokio::test]
async fn supervised_devloop_stops_before_execution_without_human_approval() {
    let (_workspace, store, evo) = test_evo("supervised-devloop-awaiting-approval");
    let request = devloop_request("task-docs-await", "docs/supervised-awaiting.md", false);

    let outcome = evo
        .run_supervised_devloop(
            &"run-supervised-devloop-await".to_string(),
            &request,
            proposal_diff_for("docs/supervised-awaiting.md", "Awaiting Approval"),
            None,
        )
        .await
        .unwrap();

    assert_eq!(outcome.status, SupervisedDevloopStatus::AwaitingApproval);
    assert_eq!(outcome.task_class, Some(BoundedTaskClass::DocsSingleFile));
    assert_eq!(outcome.execution_feedback, None);
    assert!(outcome.summary.contains("approval"));
    assert!(store.scan(1).unwrap().is_empty());
}

#[tokio::test]
async fn supervised_devloop_rejects_out_of_scope_tasks_without_bypassing_policy() {
    let (_workspace, store, evo) = test_evo("supervised-devloop-rejected");
    let request = devloop_request("task-src-rejected", "src/lib.rs", true);

    let outcome = evo
        .run_supervised_devloop(
            &"run-supervised-devloop-rejected".to_string(),
            &request,
            proposal_diff_for("src/lib.rs", "Out of Scope"),
            None,
        )
        .await
        .unwrap();

    assert_eq!(outcome.status, SupervisedDevloopStatus::RejectedByPolicy);
    assert_eq!(outcome.task_class, None);
    assert_eq!(outcome.execution_feedback, None);
    assert!(outcome.summary.contains("unsupported"));
    assert!(store.scan(1).unwrap().is_empty());
}

#[tokio::test]
async fn repeated_tasks_shift_from_fallback_to_replay_after_learning() {
    let (workspace, store, evo) = test_evo("self-evolve-hit-rate");

    let mut pre_learning_hits = 0;
    for _ in 0..2 {
        let decision = evo
            .replay_or_fallback(replay_input("missing readme", &workspace))
            .await
            .unwrap();
        if decision.used_capsule {
            pre_learning_hits += 1;
        }
    }

    let captured = evo
        .capture_successful_mutation(
            &"run-self-evolve-hit-rate".to_string(),
            sample_mutation_with_id("mutation-self-evolve-hit-rate"),
        )
        .await
        .unwrap();

    let mut post_learning_hits = 0;
    for _ in 0..4 {
        let decision = evo
            .replay_or_fallback(replay_input("missing readme", &workspace))
            .await
            .unwrap();
        if decision.used_capsule {
            post_learning_hits += 1;
        }
        assert_eq!(decision.capsule_id, Some(captured.id.clone()));
    }

    let pre_learning_hit_rate = pre_learning_hits as f64 / 2.0;
    let post_learning_hit_rate = post_learning_hits as f64 / 4.0;
    let events = store.scan(1).unwrap();
    let metrics = evo.metrics_snapshot().unwrap();

    assert_eq!(pre_learning_hits, 0);
    assert_eq!(pre_learning_hit_rate, 0.0);
    assert_eq!(post_learning_hits, 4);
    assert_eq!(post_learning_hit_rate, 1.0);
    assert!(post_learning_hit_rate > pre_learning_hit_rate);
    assert_eq!(metrics.replay_success_total, 4);
    assert_eq!(
        events
            .iter()
            .filter(|stored| matches!(
                &stored.event,
                EvolutionEvent::CapsuleReused { capsule_id, .. } if capsule_id == &captured.id
            ))
            .count(),
        4
    );
}

#[tokio::test]
async fn normalized_signal_variants_can_replay_learned_capsule() {
    let (workspace, _store, evo) = test_evo("self-evolve-normalized-signal");

    let captured = evo
        .capture_successful_mutation(
            &"run-self-evolve-normalized-signal".to_string(),
            sample_mutation_with_id("mutation-self-evolve-normalized-signal"),
        )
        .await
        .unwrap();

    let decision = evo
        .replay_or_fallback(replay_input("MISSING README", &workspace))
        .await
        .unwrap();
    let candidates = evo.select_candidates(&replay_input("MISSING README", &workspace));

    assert!(decision.used_capsule);
    assert!(!decision.fallback_to_planner);
    assert_eq!(decision.capsule_id, Some(captured.id.clone()));
    assert!(!candidates.is_empty());
    assert_eq!(candidates[0].gene.id, captured.gene_id);
    assert!(candidates[0]
        .capsules
        .iter()
        .any(|capsule| capsule.id == captured.id));
}

#[tokio::test]
async fn long_repeated_sequence_reports_stable_replay_metrics_after_learning() {
    let (workspace, store, evo) = test_evo("self-evolve-long-sequence");

    let captured = evo
        .capture_successful_mutation(
            &"run-self-evolve-long-sequence".to_string(),
            sample_mutation_with_id("mutation-self-evolve-long-sequence"),
        )
        .await
        .unwrap();

    let replay_rounds = 8;
    let mut hits = 0;
    for _ in 0..replay_rounds {
        let decision = evo
            .replay_or_fallback(replay_input("missing readme", &workspace))
            .await
            .unwrap();
        if decision.used_capsule {
            hits += 1;
        }
        assert_eq!(decision.capsule_id, Some(captured.id.clone()));
    }

    let events = store.scan(1).unwrap();
    let metrics = evo.metrics_snapshot().unwrap();

    assert_eq!(hits, replay_rounds);
    assert_eq!(metrics.replay_attempts_total, replay_rounds as u64);
    assert_eq!(metrics.replay_success_total, replay_rounds as u64);
    assert_eq!(metrics.replay_success_rate, 1.0);
    assert_eq!(
        events
            .iter()
            .filter(|stored| matches!(
                &stored.event,
                EvolutionEvent::CapsuleReused { capsule_id, .. } if capsule_id == &captured.id
            ))
            .count(),
        replay_rounds
    );
}

#[tokio::test]
async fn remote_learning_requires_local_validation_before_becoming_shareable() {
    let (_producer_workspace, producer_store, producer) = test_evo("self-evolve-remote-producer");
    let captured = producer
        .capture_successful_mutation(
            &"run-self-evolve-remote-producer".to_string(),
            sample_mutation_with_id("mutation-self-evolve-remote-producer"),
        )
        .await
        .unwrap();
    let publish = oris_evokernel::EvolutionNetworkNode::new(producer_store.clone())
        .publish_local_assets("node-producer")
        .unwrap();

    let consumer_store = Arc::new(JsonlEvolutionStore::new(unique_path(
        "self-evolve-remote-consumer-store",
    )));
    let (consumer_workspace, consumer) =
        test_evo_with_store("self-evolve-remote-consumer", consumer_store.clone());
    let import = consumer.import_remote_envelope(&publish).unwrap();
    let before = consumer_store.rebuild_projection().unwrap();
    let before_publish = oris_evokernel::EvolutionNetworkNode::new(consumer_store.clone())
        .publish_local_assets("node-consumer")
        .unwrap();

    assert!(import.accepted);
    assert!(!import.imported_asset_ids.is_empty());
    let quarantined_gene = before
        .genes
        .iter()
        .find(|gene| gene.id == captured.gene_id)
        .unwrap();
    let quarantined_capsule = before
        .capsules
        .iter()
        .find(|capsule| capsule.id == captured.id)
        .unwrap();
    assert_eq!(quarantined_gene.state, EvoAssetState::Quarantined);
    assert_eq!(quarantined_capsule.state, EvoAssetState::Quarantined);
    assert!(before_publish.assets.is_empty());

    let decision = consumer
        .replay_or_fallback(replay_input("missing readme", &consumer_workspace))
        .await
        .unwrap();
    let after = consumer_store.rebuild_projection().unwrap();
    let after_publish = oris_evokernel::EvolutionNetworkNode::new(consumer_store.clone())
        .publish_local_assets("node-consumer")
        .unwrap();

    assert!(decision.used_capsule);
    assert_eq!(decision.capsule_id, Some(captured.id.clone()));
    let promoted_gene = after
        .genes
        .iter()
        .find(|gene| gene.id == captured.gene_id)
        .unwrap();
    let promoted_capsule = after
        .capsules
        .iter()
        .find(|capsule| capsule.id == captured.id)
        .unwrap();
    assert_eq!(promoted_gene.state, EvoAssetState::Promoted);
    assert_eq!(promoted_capsule.state, EvoAssetState::Promoted);
    assert!(after_publish.assets.iter().any(|asset| matches!(
        asset,
        oris_evokernel::evolution_network::NetworkAsset::Gene { gene }
            if gene.id == captured.gene_id
    )));
    assert!(after_publish.assets.iter().any(|asset| matches!(
        asset,
        oris_evokernel::evolution_network::NetworkAsset::Capsule { capsule }
            if capsule.id == captured.id
    )));
}

#[tokio::test]
async fn distributed_learning_survives_restart_and_replays_again() {
    let (_producer_workspace, producer_store, producer) =
        test_evo("self-evolve-remote-restart-producer");
    let captured = producer
        .capture_successful_mutation(
            &"run-self-evolve-remote-restart-producer".to_string(),
            sample_mutation_with_id("mutation-self-evolve-remote-restart-producer"),
        )
        .await
        .unwrap();
    let publish = oris_evokernel::EvolutionNetworkNode::new(producer_store.clone())
        .publish_local_assets("node-producer")
        .unwrap();

    let consumer_store = Arc::new(JsonlEvolutionStore::new(unique_path(
        "self-evolve-remote-restart-consumer-store",
    )));
    let (consumer_workspace, consumer) = test_evo_with_store(
        "self-evolve-remote-restart-consumer",
        consumer_store.clone(),
    );
    consumer.import_remote_envelope(&publish).unwrap();

    let first = consumer
        .replay_or_fallback(replay_input("missing readme", &consumer_workspace))
        .await
        .unwrap();

    let (recovered_workspace, recovered) = test_evo_with_store(
        "self-evolve-remote-restart-recovered",
        consumer_store.clone(),
    );
    let second = recovered
        .replay_or_fallback(replay_input("missing readme", &recovered_workspace))
        .await
        .unwrap();
    let metrics = recovered.metrics_snapshot().unwrap();
    let projection = consumer_store.rebuild_projection().unwrap();

    assert!(first.used_capsule);
    assert!(second.used_capsule);
    assert_eq!(first.capsule_id, Some(captured.id.clone()));
    assert_eq!(second.capsule_id, Some(captured.id.clone()));
    assert_eq!(metrics.replay_attempts_total, 2);
    assert_eq!(metrics.replay_success_total, 2);
    assert_eq!(metrics.replay_success_rate, 1.0);
    assert_eq!(
        projection
            .genes
            .iter()
            .find(|gene| gene.id == captured.gene_id)
            .unwrap()
            .state,
        EvoAssetState::Promoted
    );
    assert_eq!(
        projection
            .capsules
            .iter()
            .find(|capsule| capsule.id == captured.id)
            .unwrap()
            .state,
        EvoAssetState::Promoted
    );
}

#[tokio::test]
async fn unrelated_tasks_do_not_false_positive_after_learning() {
    let (workspace, _store, evo) = test_evo("self-evolve-no-false-positive");

    let captured = evo
        .capture_successful_mutation(
            &"run-self-evolve-no-false-positive".to_string(),
            sample_mutation_with_id("mutation-self-evolve-no-false-positive"),
        )
        .await
        .unwrap();

    let unrelated = replay_input("network timeout", &workspace);
    let candidates = evo.select_candidates(&unrelated);
    let decision = evo.replay_or_fallback(unrelated).await.unwrap();

    assert!(candidates.is_empty());
    assert!(!decision.used_capsule);
    assert!(decision.fallback_to_planner);
    assert_eq!(decision.capsule_id, None);
    assert_eq!(decision.reason, "no matching gene");
    assert_ne!(decision.capsule_id, Some(captured.id));
}

#[tokio::test]
async fn mixed_task_sequence_only_replays_for_learned_signals() {
    let (workspace, store, evo) = test_evo("self-evolve-mixed-sequence");

    let captured = evo
        .capture_successful_mutation(
            &"run-self-evolve-mixed-sequence".to_string(),
            sample_mutation_with_id("mutation-self-evolve-mixed-sequence"),
        )
        .await
        .unwrap();

    let sequence = [
        ("missing readme", true),
        ("network timeout", false),
        ("missing readme", true),
        ("disk full", false),
        ("missing readme", true),
    ];
    let mut hits = 0;
    let mut misses = 0;

    for (signal, should_hit) in sequence {
        let decision = evo
            .replay_or_fallback(replay_input(signal, &workspace))
            .await
            .unwrap();
        if should_hit {
            hits += 1;
            assert!(decision.used_capsule);
            assert!(!decision.fallback_to_planner);
            assert_eq!(decision.capsule_id, Some(captured.id.clone()));
        } else {
            misses += 1;
            assert!(!decision.used_capsule);
            assert!(decision.fallback_to_planner);
            assert_eq!(decision.capsule_id, None);
            assert_eq!(decision.reason, "no matching gene");
        }
    }

    let metrics = evo.metrics_snapshot().unwrap();
    let events = store.scan(1).unwrap();

    assert_eq!(hits, 3);
    assert_eq!(misses, 2);
    assert_eq!(metrics.replay_attempts_total, hits as u64);
    assert_eq!(metrics.replay_success_total, hits as u64);
    assert_eq!(metrics.replay_success_rate, 1.0);
    assert_eq!(
        events
            .iter()
            .filter(|stored| matches!(
                &stored.event,
                EvolutionEvent::CapsuleReused { capsule_id, .. } if capsule_id == &captured.id
            ))
            .count(),
        hits
    );
}

#[test]
fn bootstrap_if_empty_seeds_exactly_four_genes_and_four_capsules() {
    let (_workspace, store, evo) = test_evo("bootstrap-seed-count");

    let report = evo
        .bootstrap_if_empty(&"run-bootstrap-seed-count".to_string())
        .unwrap();
    let projection = store.rebuild_projection().unwrap();
    let events = store.scan(1).unwrap();

    assert_eq!(
        report,
        oris_evokernel::BootstrapReport {
            seeded: true,
            genes_added: 4,
            capsules_added: 4,
        }
    );
    assert_eq!(projection.genes.len(), 4);
    assert_eq!(projection.capsules.len(), 4);
    assert_eq!(events.len(), 24);
}

#[test]
fn bootstrap_capsules_start_quarantined() {
    let (_workspace, store, evo) = test_evo("bootstrap-quarantine");

    evo.bootstrap_if_empty(&"run-bootstrap-quarantine".to_string())
        .unwrap();
    let projection = store.rebuild_projection().unwrap();

    assert_eq!(projection.genes.len(), 4);
    assert!(projection
        .genes
        .iter()
        .all(|gene| gene.state == EvoAssetState::Quarantined));
    assert!(projection
        .capsules
        .iter()
        .all(|capsule| capsule.state == EvoAssetState::Quarantined));
}

#[test]
fn bootstrap_if_empty_is_idempotent() {
    let (_workspace, store, evo) = test_evo("bootstrap-idempotent");

    let first = evo
        .bootstrap_if_empty(&"run-bootstrap-idempotent-1".to_string())
        .unwrap();
    let event_count_after_first = store.scan(1).unwrap().len();
    let second = evo
        .bootstrap_if_empty(&"run-bootstrap-idempotent-2".to_string())
        .unwrap();
    let event_count_after_second = store.scan(1).unwrap().len();

    assert!(first.seeded);
    assert_eq!(
        second,
        oris_evokernel::BootstrapReport {
            seeded: false,
            genes_added: 0,
            capsules_added: 0,
        }
    );
    assert_eq!(event_count_after_first, event_count_after_second);
}

#[test]
fn bootstrap_appends_records_without_overwriting_existing_events() {
    let (_workspace, store, evo) = test_evo("bootstrap-append-only");

    store
        .append_event(EvolutionEvent::MutationDeclared {
            mutation: sample_mutation_with_id("bootstrap-preexisting"),
        })
        .unwrap();

    evo.bootstrap_if_empty(&"run-bootstrap-append-only".to_string())
        .unwrap();
    let events = store.scan(1).unwrap();

    assert_eq!(events.len(), 25);
    assert!(matches!(
        &events[0].event,
        EvolutionEvent::MutationDeclared { mutation }
            if mutation.intent.id == "bootstrap-preexisting"
    ));
}

#[tokio::test]
async fn bootstrap_seeds_remain_quarantined_and_do_not_appear_as_replay_candidates() {
    let (workspace, store, evo) = test_evo("bootstrap-discoverable");

    evo.bootstrap_if_empty(&"run-bootstrap-discoverable".to_string())
        .unwrap();

    let candidates = evo.select_candidates(&replay_input("bootstrap readme", &workspace));
    let decision = evo
        .replay_or_fallback(replay_input("bootstrap readme", &workspace))
        .await
        .unwrap();
    let projection = store.rebuild_projection().unwrap();

    assert!(candidates.is_empty());
    assert!(!decision.used_capsule);
    assert!(decision.fallback_to_planner);
    assert_eq!(decision.reason, "no matching gene");
    assert!(projection
        .genes
        .iter()
        .all(|gene| gene.state == EvoAssetState::Quarantined));
    assert!(projection
        .capsules
        .iter()
        .all(|capsule| capsule.state == EvoAssetState::Quarantined));
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
        .replay_or_fallback(replay_input("missing readme", &workspace))
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
async fn local_capture_uses_existing_confidence_context_for_governor() {
    let workspace = temp_workspace();
    let sandbox_root = unique_path("governor-confidence-sandbox");
    let store_root = unique_path("governor-confidence-store");
    let store = Arc::new(JsonlEvolutionStore::new(&store_root));
    let validator = Arc::new(CommandValidator::new(sandbox_policy()));
    let sandbox = Arc::new(LocalProcessSandbox::new(
        "run-governor-confidence",
        &workspace,
        &sandbox_root,
    ));
    let evo = EvoKernel::new(test_kernel(), sandbox, validator, store.clone())
        .with_governor(Arc::new(DefaultGovernor::new(GovernorConfig {
            promote_after_successes: 1,
            cooldown_secs: 0,
            retry_cooldown_secs: 30,
            max_mutations_per_window: 100,
            confidence_decay_rate_per_hour: 1.0,
            max_confidence_drop: 0.2,
            ..Default::default()
        })))
        .with_sandbox_policy(sandbox_policy())
        .with_validation_plan(lightweight_plan());

    let first = evo
        .capture_mutation_with_governor(
            &"run-governor-confidence".to_string(),
            sample_mutation_with_id("mutation-governor-confidence-1"),
        )
        .await
        .unwrap();
    backdate_store_events(store.as_ref(), Duration::hours(2));
    let second = evo
        .capture_mutation_with_governor(
            &"run-governor-confidence".to_string(),
            sample_mutation_with_id("mutation-governor-confidence-2"),
        )
        .await
        .unwrap();

    assert_eq!(
        first.governor_decision.target_state,
        EvoAssetState::Promoted
    );
    assert_eq!(
        second.governor_decision.target_state,
        EvoAssetState::Revoked
    );
    assert!(second
        .governor_decision
        .reason
        .contains("confidence regression"));
    assert_eq!(second.gene.state, EvoAssetState::Revoked);
    assert_eq!(second.capsule.state, EvoAssetState::Revoked);
    let events = store.scan(1).unwrap();
    let metrics = evo.metrics_snapshot().unwrap();
    assert!(events.iter().any(|stored| matches!(
        &stored.event,
        EvolutionEvent::GeneRevoked { gene_id, .. } if gene_id == &second.gene.id
    )));
    assert_eq!(metrics.gene_revocations_total, 1);
}

#[tokio::test]
async fn failed_replay_stops_immediate_reuse_without_revocation() {
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
        .replay_or_fallback(replay_input("missing readme", &workspace))
        .await
        .unwrap();
    let second = replay_evo
        .replay_or_fallback(replay_input("missing readme", &workspace))
        .await
        .unwrap();
    let projection = store.rebuild_projection().unwrap();
    let events = store.scan(1).unwrap();
    let metrics = replay_evo.metrics_snapshot().unwrap();

    assert!(!first.used_capsule);
    assert!(first.fallback_to_planner);
    assert_eq!(first.capsule_id, Some(captured.id.clone()));
    assert_eq!(first.reason, "replay validation failed");

    assert!(!second.used_capsule);
    assert!(second.fallback_to_planner);
    assert_eq!(second.capsule_id, None);
    assert!(second.reason.contains("below replay threshold"));

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
    assert_eq!(stored_gene.state, EvoAssetState::Promoted);
    assert_eq!(stored_capsule.state, EvoAssetState::Promoted);
    assert_eq!(metrics.replay_success_total, 0);
    assert_eq!(
        events
            .iter()
            .filter(|stored| matches!(
                &stored.event,
                EvolutionEvent::ValidationFailed {
                    gene_id: Some(gene_id),
                    ..
                } if gene_id == &captured.gene_id
            ))
            .count(),
        1
    );
    assert!(!events
        .iter()
        .any(|stored| matches!(stored.event, EvolutionEvent::GeneRevoked { .. })));
    assert!(!events.iter().any(|stored| matches!(
        &stored.event,
        EvolutionEvent::CapsuleReused { capsule_id, .. } if capsule_id == &captured.id
    )));
}
