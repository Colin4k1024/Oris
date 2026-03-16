//! Black-box regression coverage for replay determinism, sandbox boundaries,
//! governor policy, and the end-to-end EvoKernel lifecycle.

use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{Duration, Utc};
use oris_agent_contract::{
    AgentTask, AutonomousApprovalMode, AutonomousCandidateSource, AutonomousIntakeInput,
    AutonomousIntakeReasonCode, AutonomousPlanReasonCode, AutonomousProposalReasonCode,
    AutonomousRiskTier, BoundedTaskClass, HumanApproval, MutationNeededFailureReasonCode,
    MutationProposal, MutationProposalContractReasonCode, MutationProposalEvidence,
    ReplayFallbackNextAction, ReplayFallbackReasonCode, ReplayPlannerDirective,
    SelfEvolutionAcceptanceGateInput, SelfEvolutionAcceptanceGateReasonCode,
    SelfEvolutionAuditConsistencyResult, SelfEvolutionCandidateIntakeRequest,
    SelfEvolutionSelectionReasonCode, SupervisedDeliveryApprovalState,
    SupervisedDeliveryReasonCode, SupervisedDeliveryStatus, SupervisedDevloopOutcome,
    SupervisedDevloopRequest, SupervisedDevloopStatus, SupervisedExecutionDecision,
    SupervisedExecutionReasonCode, SupervisedValidationOutcome,
};
use oris_evokernel::{
    extract_deterministic_signals, prepare_mutation, CommandValidator, EvoAssetState,
    EvoEnvFingerprint, EvoEvolutionStore, EvoKernel, EvoSandboxPolicy, EvoSelectorInput,
    JsonlEvolutionStore, LocalProcessSandbox, MutationIntent, MutationTarget, RiskLevel,
    SignalExtractionInput, ValidationPlan, ValidationStage,
};
use oris_evolution::{
    compute_artifact_hash, rebuild_projection_from_events, stable_hash_json, EvolutionEvent,
    PreparedMutation, StoredEvolutionEvent, TransitionReasonCode, MIN_REPLAY_CONFIDENCE,
};
use oris_governor::{DefaultGovernor, GovernorConfig};
use oris_kernel::{
    AllowAllPolicy, InMemoryEventStore, Kernel, KernelMode, KernelState, NoopActionExecutor,
    NoopStepFn, StateUpdatedOnlyReducer,
};
use oris_sandbox::Sandbox;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

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

fn create_audit_log_path(test_name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::current_dir()
        .unwrap()
        .join("target/test-audit/evolution_lifecycle_regression");
    fs::create_dir_all(&root).unwrap();
    root.join(format!("{test_name}-{nonce}.log"))
}

fn append_audit_log(path: &PathBuf, line: impl AsRef<str>) {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    file.write_all(line.as_ref().as_bytes()).unwrap();
    file.write_all(b"\n").unwrap();
}

fn create_false_positive_snapshot_path(test_name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::current_dir()
        .unwrap()
        .join("target/test-audit/evolution_lifecycle_regression/false-positive-snapshots");
    fs::create_dir_all(&root).unwrap();
    root.join(format!("{test_name}-{nonce}.json"))
}

fn write_false_positive_snapshot(test_name: &str, payload: &Value) -> PathBuf {
    let path = create_false_positive_snapshot_path(test_name);
    fs::write(&path, serde_json::to_string_pretty(payload).unwrap()).unwrap();
    path
}

struct TestAuditGuard {
    path: PathBuf,
    test_name: String,
}

impl TestAuditGuard {
    fn new(test_name: &str) -> Self {
        let path = create_audit_log_path(test_name);
        append_audit_log(
            &path,
            format!("[START] test={test_name} pid={}", std::process::id()),
        );
        Self {
            path,
            test_name: test_name.to_string(),
        }
    }
}

impl Drop for TestAuditGuard {
    fn drop(&mut self) {
        let status = if std::thread::panicking() {
            "FAIL"
        } else {
            "PASS"
        };
        append_audit_log(
            &self.path,
            format!("[END] test={} status={status}", self.test_name),
        );
    }
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

fn devloop_request_with_files(
    task_id: &str,
    files: Vec<&str>,
    approved: bool,
) -> SupervisedDevloopRequest {
    let file_list = files
        .iter()
        .map(|file| (*file).to_string())
        .collect::<Vec<_>>();
    let description = files.join(", ");

    SupervisedDevloopRequest {
        task: AgentTask {
            id: task_id.into(),
            description: format!("Update {description} through the supervised DEVLOOP"),
        },
        proposal: MutationProposal {
            intent: format!("Update {description}"),
            files: file_list,
            expected_effect: format!("Keep {description} in sync"),
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

fn github_issue_candidate_request(
    issue_number: u64,
    state: &str,
    labels: Vec<&str>,
    candidate_hint_paths: Vec<&str>,
) -> SelfEvolutionCandidateIntakeRequest {
    SelfEvolutionCandidateIntakeRequest {
        issue_number,
        title: format!("Issue {issue_number}"),
        body: "Bounded self-evolution candidate".into(),
        labels: labels.into_iter().map(|label| label.to_string()).collect(),
        state: state.into(),
        candidate_hint_paths: candidate_hint_paths
            .into_iter()
            .map(|path| path.to_string())
            .collect(),
    }
}

fn proposal_diff_for(path: &str, title: &str) -> String {
    format!(
        "diff --git a/{path} b/{path}\nnew file mode 100644\nindex 0000000..1111111\n--- /dev/null\n+++ b/{path}\n@@ -0,0 +1,2 @@\n+# {title}\n+generated by supervised devloop\n"
    )
}

fn proposal_diff_for_files(files: &[&str]) -> String {
    let mut diff = String::new();
    for (idx, path) in files.iter().enumerate() {
        diff.push_str(&format!(
            "diff --git a/{path} b/{path}\nnew file mode 100644\nindex 0000000..1111111\n--- /dev/null\n+++ b/{path}\n@@ -0,0 +1,2 @@\n+# Multi File {}\n+generated by supervised devloop\n",
            idx + 1
        ));
    }
    diff
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
    test_evo_with_policy_and_plan(label, sandbox_policy(), lightweight_plan())
}

fn test_evo_with_policy_and_plan(
    label: &str,
    policy: EvoSandboxPolicy,
    plan: ValidationPlan,
) -> (PathBuf, Arc<JsonlEvolutionStore>, EvoKernel<TestState>) {
    let workspace = temp_workspace();
    let sandbox_root = unique_path(&format!("{label}-sandbox"));
    let store_root = unique_path(&format!("{label}-store"));
    let store = Arc::new(JsonlEvolutionStore::new(&store_root));
    let validator = Arc::new(CommandValidator::new(policy.clone()));
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
        .with_sandbox_policy(policy)
        .with_validation_plan(plan);
    (workspace, store, evo)
}

fn test_evo_with_store(
    label: &str,
    store: Arc<JsonlEvolutionStore>,
) -> (PathBuf, EvoKernel<TestState>) {
    test_evo_with_store_and_plan(label, store, sandbox_policy(), lightweight_plan())
}

fn test_evo_with_store_and_plan(
    label: &str,
    store: Arc<JsonlEvolutionStore>,
    policy: EvoSandboxPolicy,
    plan: ValidationPlan,
) -> (PathBuf, EvoKernel<TestState>) {
    let workspace = temp_workspace();
    let sandbox_root = unique_path(&format!("{label}-sandbox"));
    let validator = Arc::new(CommandValidator::new(policy.clone()));
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
        .with_sandbox_policy(policy)
        .with_validation_plan(plan);
    (workspace, evo)
}

fn strategy_metadata_value(strategy: &[String], key: &str) -> Option<String> {
    strategy.iter().find_map(|entry| {
        let (entry_key, entry_value) = entry.split_once('=')?;
        if entry_key.trim().eq_ignore_ascii_case(key) {
            let value = entry_value.trim();
            if value.is_empty() {
                None
            } else {
                Some(value.to_string())
            }
        } else {
            None
        }
    })
}

struct SeededStore {
    events: Mutex<Vec<StoredEvolutionEvent>>,
}

impl SeededStore {
    fn new(events: Vec<StoredEvolutionEvent>) -> Self {
        Self {
            events: Mutex::new(events),
        }
    }
}

impl EvoEvolutionStore for SeededStore {
    fn append_event(&self, event: EvolutionEvent) -> Result<u64, oris_evolution::EvolutionError> {
        let mut events = self.events.lock().unwrap();
        let seq = events.len() as u64 + 1;
        let timestamp = Utc::now().to_rfc3339();
        let prev_hash = events
            .last()
            .map(|stored| stored.record_hash.clone())
            .unwrap_or_default();
        let record_hash = stable_hash_json(&(seq, &timestamp, &prev_hash, &event))
            .unwrap_or_else(|_| format!("hash-{seq}"));
        events.push(StoredEvolutionEvent {
            seq,
            timestamp,
            prev_hash,
            record_hash,
            event,
        });
        Ok(seq)
    }

    fn scan(
        &self,
        from_seq: u64,
    ) -> Result<Vec<StoredEvolutionEvent>, oris_evolution::EvolutionError> {
        Ok(self
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|stored| stored.seq >= from_seq)
            .cloned()
            .collect())
    }

    fn rebuild_projection(
        &self,
    ) -> Result<oris_evolution::EvolutionProjection, oris_evolution::EvolutionError> {
        Ok(rebuild_projection_from_events(&self.events.lock().unwrap()))
    }
}

fn test_evo_with_seeded_store(
    label: &str,
    store: Arc<SeededStore>,
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
    let _audit = TestAuditGuard::new("capture_then_replay_records_full_lifecycle");
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
    let _audit =
        TestAuditGuard::new("replay_selection_is_deterministic_across_repeated_identical_inputs");
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
    let _audit = TestAuditGuard::new("deterministic_signal_extraction_is_stable");
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
    let _audit = TestAuditGuard::new("local_selector_query_returns_captured_gene");
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
    let _audit = TestAuditGuard::new("single_task_learns_and_replays_on_second_run");
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
async fn replay_feedback_surfaces_planner_hints_and_reasoning_savings() {
    let _audit =
        TestAuditGuard::new("replay_feedback_surfaces_planner_hints_and_reasoning_savings");
    let (workspace, store, evo) = test_evo("replay-feedback");
    let signals = vec!["missing readme".to_string()];

    let cold_start = evo
        .replay_or_fallback(replay_input("missing readme", &workspace))
        .await
        .unwrap();
    let cold_feedback = EvoKernel::<TestState>::replay_feedback_for_agent(&signals, &cold_start);

    assert_eq!(
        cold_feedback.planner_directive,
        ReplayPlannerDirective::PlanFallback
    );
    assert_eq!(cold_feedback.reasoning_steps_avoided, 0);
    assert_eq!(
        cold_feedback.fallback_reason.as_deref(),
        Some("no matching gene")
    );
    assert_eq!(
        cold_feedback.reason_code,
        Some(ReplayFallbackReasonCode::NoCandidateAfterSelect)
    );
    assert_eq!(
        cold_feedback.next_action,
        Some(ReplayFallbackNextAction::PlanFromScratch)
    );
    assert!(cold_feedback.repair_hint.is_some());
    assert_eq!(cold_feedback.confidence, Some(92));
    assert!(!cold_feedback.task_class_id.is_empty());
    assert_eq!(cold_feedback.task_label, "missing readme");

    let captured = evo
        .capture_successful_mutation(
            &"run-replay-feedback".to_string(),
            sample_mutation_with_id("mutation-replay-feedback"),
        )
        .await
        .unwrap();
    let replay = evo
        .replay_or_fallback(replay_input("missing readme", &workspace))
        .await
        .unwrap();
    let replay_feedback = EvoKernel::<TestState>::replay_feedback_for_agent(&signals, &replay);
    let events = store.scan(1).unwrap();

    assert_eq!(
        replay_feedback.planner_directive,
        ReplayPlannerDirective::SkipPlanner
    );
    assert_eq!(replay_feedback.reasoning_steps_avoided, 1);
    assert!(replay_feedback.used_capsule);
    assert_eq!(replay_feedback.capsule_id, Some(captured.id));
    assert_eq!(replay_feedback.fallback_reason, None);
    assert_eq!(replay_feedback.reason_code, None);
    assert_eq!(replay_feedback.repair_hint, None);
    assert_eq!(replay_feedback.next_action, None);
    assert_eq!(replay_feedback.confidence, None);
    assert_eq!(replay_feedback.task_class_id, cold_feedback.task_class_id);
    assert_eq!(replay_feedback.task_label, "missing readme");
    assert!(events.iter().any(|stored| matches!(
        &stored.event,
        EvolutionEvent::PromotionEvaluated {
            gene_id,
            state,
            reason_code,
            ..
        } if gene_id == &captured.gene_id
            && *state == EvoAssetState::Promoted
            && reason_code == &TransitionReasonCode::PromotionSuccessThreshold
    )));
}

#[tokio::test]
async fn supervised_devloop_executes_bounded_docs_task_after_approval() {
    let _audit =
        TestAuditGuard::new("supervised_devloop_executes_bounded_docs_task_after_approval");
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
    assert_eq!(
        outcome.execution_decision,
        SupervisedExecutionDecision::PlannerFallback
    );
    assert_eq!(outcome.task_class, Some(BoundedTaskClass::DocsSingleFile));
    assert!(outcome.execution_feedback.is_some());
    assert_eq!(outcome.failure_contract, None);
    assert_eq!(
        outcome.validation_outcome,
        SupervisedValidationOutcome::Passed
    );
    assert_eq!(
        outcome.reason_code,
        Some(SupervisedExecutionReasonCode::ReplayFallback)
    );
    assert!(outcome.summary.contains("executed"));
    assert!(store
        .scan(1)
        .unwrap()
        .iter()
        .any(|stored| matches!(stored.event, EvolutionEvent::MutationDeclared { .. })));
}

#[tokio::test]
async fn supervised_devloop_executes_bounded_multifile_docs_task_after_approval() {
    let _audit = TestAuditGuard::new(
        "supervised_devloop_executes_bounded_multifile_docs_task_after_approval",
    );
    let (_workspace, store, evo) = test_evo("supervised-devloop-multifile-approved");
    let request = devloop_request_with_files(
        "task-docs-multifile-approved",
        vec!["docs/supervised-a.md", "docs/supervised-b.md"],
        true,
    );

    let outcome = evo
        .run_supervised_devloop(
            &"run-supervised-devloop-multifile-approved".to_string(),
            &request,
            proposal_diff_for_files(&["docs/supervised-a.md", "docs/supervised-b.md"]),
            None,
        )
        .await
        .unwrap();

    assert_eq!(outcome.status, SupervisedDevloopStatus::Executed);
    assert_eq!(outcome.task_class, Some(BoundedTaskClass::DocsMultiFile));
    assert!(outcome.execution_feedback.is_some());
    assert_eq!(outcome.failure_contract, None);
    assert!(outcome.summary.contains("executed"));
    assert!(store
        .scan(1)
        .unwrap()
        .iter()
        .any(|stored| matches!(stored.event, EvolutionEvent::MutationDeclared { .. })));
}

#[tokio::test]
async fn supervised_devloop_stops_before_execution_without_human_approval() {
    let _audit =
        TestAuditGuard::new("supervised_devloop_stops_before_execution_without_human_approval");
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
    assert_eq!(
        outcome.execution_decision,
        SupervisedExecutionDecision::AwaitingApproval
    );
    assert_eq!(outcome.task_class, Some(BoundedTaskClass::DocsSingleFile));
    assert_eq!(outcome.execution_feedback, None);
    assert_eq!(outcome.failure_contract, None);
    assert_eq!(
        outcome.validation_outcome,
        SupervisedValidationOutcome::NotRun
    );
    assert!(outcome.summary.contains("approval"));
    assert!(store.scan(1).unwrap().is_empty());
}

#[tokio::test]
async fn supervised_devloop_rejects_multifile_docs_request_with_out_of_scope_path() {
    let _audit = TestAuditGuard::new(
        "supervised_devloop_rejects_multifile_docs_request_with_out_of_scope_path",
    );
    let (_workspace, store, evo) = test_evo("supervised-devloop-multifile-oos");
    let request = devloop_request_with_files(
        "task-docs-multifile-oos",
        vec!["docs/supervised-a.md", "src/lib.rs"],
        true,
    );

    let outcome = evo
        .run_supervised_devloop(
            &"run-supervised-devloop-multifile-oos".to_string(),
            &request,
            proposal_diff_for_files(&["docs/supervised-a.md", "src/lib.rs"]),
            None,
        )
        .await
        .unwrap();

    assert_eq!(outcome.status, SupervisedDevloopStatus::RejectedByPolicy);
    assert_eq!(
        outcome.execution_decision,
        SupervisedExecutionDecision::RejectedByPolicy
    );
    assert_eq!(outcome.task_class, None);
    assert_eq!(outcome.execution_feedback, None);
    assert_eq!(
        outcome
            .failure_contract
            .as_ref()
            .map(|contract| contract.reason_code),
        Some(MutationNeededFailureReasonCode::PolicyDenied)
    );
    assert!(store.scan(1).unwrap().iter().any(|stored| matches!(
        &stored.event,
        EvolutionEvent::MutationRejected {
            reason_code: Some(reason_code),
            fail_closed,
            ..
        } if reason_code == "policy_denied" && *fail_closed
    )));
}

#[tokio::test]
async fn supervised_devloop_rejects_multifile_docs_request_over_file_limit() {
    let _audit =
        TestAuditGuard::new("supervised_devloop_rejects_multifile_docs_request_over_file_limit");
    let (_workspace, store, evo) = test_evo("supervised-devloop-multifile-over-limit");
    let request = devloop_request_with_files(
        "task-docs-multifile-over-limit",
        vec![
            "docs/supervised-1.md",
            "docs/supervised-2.md",
            "docs/supervised-3.md",
            "docs/supervised-4.md",
        ],
        true,
    );

    let outcome = evo
        .run_supervised_devloop(
            &"run-supervised-devloop-multifile-over-limit".to_string(),
            &request,
            proposal_diff_for_files(&[
                "docs/supervised-1.md",
                "docs/supervised-2.md",
                "docs/supervised-3.md",
                "docs/supervised-4.md",
            ]),
            None,
        )
        .await
        .unwrap();

    assert_eq!(outcome.status, SupervisedDevloopStatus::RejectedByPolicy);
    assert_eq!(outcome.task_class, None);
    assert_eq!(
        outcome
            .failure_contract
            .as_ref()
            .map(|contract| contract.reason_code),
        Some(MutationNeededFailureReasonCode::PolicyDenied)
    );
    assert!(store.scan(1).unwrap().iter().any(|stored| matches!(
        &stored.event,
        EvolutionEvent::MutationRejected {
            reason_code: Some(reason_code),
            fail_closed,
            ..
        } if reason_code == "policy_denied" && *fail_closed
    )));
}

#[tokio::test]
async fn supervised_devloop_rejects_out_of_scope_tasks_without_bypassing_policy() {
    let _audit = TestAuditGuard::new(
        "supervised_devloop_rejects_out_of_scope_tasks_without_bypassing_policy",
    );
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
    assert_eq!(
        outcome
            .failure_contract
            .as_ref()
            .map(|contract| contract.reason_code),
        Some(MutationNeededFailureReasonCode::PolicyDenied)
    );
    assert!(outcome.summary.contains("unsupported"));
    let events = store.scan(1).unwrap();
    assert!(events.iter().any(|stored| matches!(
        &stored.event,
        EvolutionEvent::MutationRejected {
            reason_code: Some(reason_code),
            fail_closed,
            ..
        } if reason_code == "policy_denied" && *fail_closed
    )));
}

#[tokio::test]
async fn supervised_devloop_fails_closed_for_unsafe_patch_and_records_reason_code() {
    let _audit = TestAuditGuard::new(
        "supervised_devloop_fails_closed_for_unsafe_patch_and_records_reason_code",
    );
    let (_workspace, store, evo) = test_evo("supervised-devloop-unsafe-patch");
    let request = devloop_request("task-docs-unsafe", "docs/supervised-unsafe.md", true);

    let outcome = evo
        .run_supervised_devloop(
            &"run-supervised-devloop-unsafe".to_string(),
            &request,
            proposal_diff_for("src/lib.rs", "Unsafe Patch"),
            None,
        )
        .await
        .unwrap();

    assert_eq!(outcome.status, SupervisedDevloopStatus::FailedClosed);
    assert_eq!(
        outcome.execution_decision,
        SupervisedExecutionDecision::FailedClosed
    );
    assert_eq!(
        outcome
            .failure_contract
            .as_ref()
            .map(|contract| contract.reason_code),
        Some(MutationNeededFailureReasonCode::UnsafePatch)
    );
    assert!(outcome.summary.contains("unsafe_patch"));
    assert!(store.scan(1).unwrap().iter().any(|stored| matches!(
        &stored.event,
        EvolutionEvent::MutationRejected {
            reason_code: Some(reason_code),
            fail_closed,
            ..
        } if reason_code == "unsafe_patch" && *fail_closed
    )));
}

#[tokio::test]
async fn replay_supervised_execution_reuses_matching_capsule_after_learning() {
    let _audit =
        TestAuditGuard::new("replay_supervised_execution_reuses_matching_capsule_after_learning");
    let (_workspace, store, evo) = test_evo("replay-supervised-execution-hit");
    let request = devloop_request("task-docs-replay-hit", "docs/replay-hit.md", true);
    let diff = proposal_diff_for("docs/replay-hit.md", "Replay Hit");

    let first = evo
        .run_supervised_devloop(
            &"run-supervised-devloop-replay-first".to_string(),
            &request,
            diff.clone(),
            None,
        )
        .await
        .unwrap();
    let second = evo
        .run_supervised_devloop(
            &"run-supervised-devloop-replay-second".to_string(),
            &request,
            diff,
            None,
        )
        .await
        .unwrap();

    assert_eq!(
        first.execution_decision,
        SupervisedExecutionDecision::PlannerFallback
    );
    assert_eq!(second.status, SupervisedDevloopStatus::Executed);
    assert_eq!(
        second.execution_decision,
        SupervisedExecutionDecision::ReplayHit
    );
    assert_eq!(
        second.validation_outcome,
        SupervisedValidationOutcome::Passed
    );
    assert_eq!(
        second.reason_code,
        Some(SupervisedExecutionReasonCode::ReplayHit)
    );
    assert!(second
        .replay_outcome
        .as_ref()
        .is_some_and(|feedback| feedback.used_capsule));
    assert_eq!(second.fallback_reason, None);
    assert!(second.evidence_summary.contains("ReplayHit"));

    let events = store.scan(1).unwrap();
    assert!(events
        .iter()
        .any(|stored| matches!(&stored.event, EvolutionEvent::CapsuleReused { .. })));
}

#[tokio::test]
async fn replay_supervised_execution_fails_closed_when_replay_validation_fails() {
    let _audit = TestAuditGuard::new(
        "replay_supervised_execution_fails_closed_when_replay_validation_fails",
    );
    let store = Arc::new(JsonlEvolutionStore::new(unique_path(
        "replay-supervised-execution-validation-store",
    )));
    let (_learning_workspace, learning_evo) = test_evo_with_store(
        "replay-supervised-execution-validation-learn",
        store.clone(),
    );
    let (_failing_workspace, failing_evo) = test_evo_with_store_and_plan(
        "replay-supervised-execution-validation-fail",
        store.clone(),
        sandbox_policy(),
        failing_plan(),
    );
    let request = devloop_request(
        "task-docs-replay-validation-fail",
        "docs/replay-validation-fail.md",
        true,
    );
    let diff = proposal_diff_for("docs/replay-validation-fail.md", "Replay Validation Fail");

    let learned = learning_evo
        .run_supervised_devloop(
            &"run-replay-supervised-validation-first".to_string(),
            &request,
            diff.clone(),
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        learned.execution_decision,
        SupervisedExecutionDecision::PlannerFallback
    );

    let failed = failing_evo
        .run_supervised_devloop(
            &"run-replay-supervised-validation-second".to_string(),
            &request,
            diff,
            None,
        )
        .await
        .unwrap();

    assert_eq!(failed.status, SupervisedDevloopStatus::FailedClosed);
    assert_eq!(
        failed.execution_decision,
        SupervisedExecutionDecision::FailedClosed
    );
    assert_eq!(
        failed.validation_outcome,
        SupervisedValidationOutcome::FailedClosed
    );
    assert_eq!(
        failed.reason_code,
        Some(SupervisedExecutionReasonCode::ValidationFailed)
    );
    assert!(failed
        .replay_outcome
        .as_ref()
        .and_then(|feedback| feedback.reason_code)
        .is_some_and(|code| code == ReplayFallbackReasonCode::ValidationFailed));
    assert!(failed
        .fallback_reason
        .as_deref()
        .is_some_and(|reason| reason.contains("validation failed")));
    assert_eq!(
        failed
            .failure_contract
            .as_ref()
            .map(|contract| contract.reason_code),
        Some(MutationNeededFailureReasonCode::ValidationFailed)
    );
}

#[tokio::test]
async fn supervised_devloop_fails_closed_on_validation_failure_with_recovery_contract() {
    let _audit = TestAuditGuard::new(
        "supervised_devloop_fails_closed_on_validation_failure_with_recovery_contract",
    );
    let (_workspace, store, evo) = test_evo_with_policy_and_plan(
        "supervised-devloop-validation-fail",
        sandbox_policy(),
        failing_plan(),
    );
    let request = devloop_request("task-docs-validation-fail", "docs/supervised-fail.md", true);

    let outcome = evo
        .run_supervised_devloop(
            &"run-supervised-devloop-validation-fail".to_string(),
            &request,
            proposal_diff_for("docs/supervised-fail.md", "Validation Fail"),
            None,
        )
        .await
        .unwrap();

    assert_eq!(outcome.status, SupervisedDevloopStatus::FailedClosed);
    let failure_contract = outcome.failure_contract.as_ref().expect("missing contract");
    assert_eq!(
        failure_contract.reason_code,
        MutationNeededFailureReasonCode::ValidationFailed
    );
    assert!(failure_contract.recovery_hint.contains("Repair"));
    let events = store.scan(1).unwrap();
    assert!(events
        .iter()
        .any(|stored| matches!(&stored.event, EvolutionEvent::ValidationFailed { .. })));
    assert!(events.iter().any(|stored| matches!(
        &stored.event,
        EvolutionEvent::MutationRejected {
            reason_code: Some(reason_code),
            fail_closed,
            ..
        } if reason_code == "validation_failed" && *fail_closed
    )));
}

#[tokio::test]
async fn supervised_devloop_rejects_validation_budget_over_limit_with_policy_reason() {
    let _audit = TestAuditGuard::new(
        "supervised_devloop_rejects_validation_budget_over_limit_with_policy_reason",
    );
    let over_budget_plan = ValidationPlan {
        profile: "regression-over-budget".into(),
        stages: vec![ValidationStage::Command {
            program: "git".into(),
            args: vec!["--version".into()],
            timeout_ms: 900_001,
        }],
    };
    let (_workspace, store, evo) = test_evo_with_policy_and_plan(
        "supervised-devloop-budget-reject",
        sandbox_policy(),
        over_budget_plan,
    );
    let request = devloop_request("task-docs-budget-reject", "docs/supervised-budget.md", true);

    let outcome = evo
        .run_supervised_devloop(
            &"run-supervised-devloop-budget-reject".to_string(),
            &request,
            proposal_diff_for("docs/supervised-budget.md", "Budget Reject"),
            None,
        )
        .await
        .unwrap();

    assert_eq!(outcome.status, SupervisedDevloopStatus::RejectedByPolicy);
    assert_eq!(
        outcome
            .failure_contract
            .as_ref()
            .map(|contract| contract.reason_code),
        Some(MutationNeededFailureReasonCode::PolicyDenied)
    );
    assert!(store.scan(1).unwrap().iter().any(|stored| matches!(
        &stored.event,
        EvolutionEvent::MutationRejected {
            reason_code: Some(reason_code),
            fail_closed,
            ..
        } if reason_code == "policy_denied" && *fail_closed
    )));
}

#[tokio::test]
async fn supervised_devloop_fails_closed_on_timeout_with_consistent_reason_code() {
    let _audit = TestAuditGuard::new(
        "supervised_devloop_fails_closed_on_timeout_with_consistent_reason_code",
    );
    let mut timeout_policy = sandbox_policy();
    timeout_policy.max_duration_ms = 0;
    let (_workspace, store, evo) = test_evo_with_policy_and_plan(
        "supervised-devloop-timeout",
        timeout_policy,
        lightweight_plan(),
    );
    let request = devloop_request("task-docs-timeout", "docs/supervised-timeout.md", true);

    let outcome = evo
        .run_supervised_devloop(
            &"run-supervised-devloop-timeout".to_string(),
            &request,
            proposal_diff_for("docs/supervised-timeout.md", "Timeout"),
            None,
        )
        .await
        .unwrap();

    assert_eq!(outcome.status, SupervisedDevloopStatus::FailedClosed);
    assert_eq!(
        outcome
            .failure_contract
            .as_ref()
            .map(|contract| contract.reason_code),
        Some(MutationNeededFailureReasonCode::Timeout)
    );
    assert!(store.scan(1).unwrap().iter().any(|stored| matches!(
        &stored.event,
        EvolutionEvent::MutationRejected {
            reason_code: Some(reason_code),
            fail_closed,
            ..
        } if reason_code == "timeout" && *fail_closed
    )));
}

#[tokio::test]
async fn delivery_summary_prepares_bounded_branch_and_pr_after_supervised_execution() {
    let _audit = TestAuditGuard::new(
        "delivery_summary_prepares_bounded_branch_and_pr_after_supervised_execution",
    );
    let (_workspace, store, evo) = test_evo("delivery-summary-prepared");
    let request = devloop_request("task-docs-delivery", "docs/delivery-summary.md", true);

    let outcome = evo
        .run_supervised_devloop(
            &"run-delivery-summary-prepared".to_string(),
            &request,
            proposal_diff_for("docs/delivery-summary.md", "Delivery Summary"),
            None,
        )
        .await
        .unwrap();

    let delivery = evo.prepare_supervised_delivery(&request, &outcome).unwrap();

    assert_eq!(delivery.delivery_status, SupervisedDeliveryStatus::Prepared);
    assert_eq!(
        delivery.approval_state,
        SupervisedDeliveryApprovalState::Approved
    );
    assert_eq!(
        delivery.reason_code,
        SupervisedDeliveryReasonCode::DeliveryPrepared
    );
    assert!(delivery
        .branch_name
        .as_deref()
        .is_some_and(|value| value.starts_with("self-evolution/docs/")));
    assert!(delivery
        .pr_title
        .as_deref()
        .is_some_and(|value| value.contains("self-evolution")));
    assert!(delivery
        .pr_summary
        .as_deref()
        .is_some_and(|value| value.contains("validation_summary=")));
    assert!(store.scan(1).unwrap().iter().any(|stored| matches!(
        &stored.event,
        EvolutionEvent::DeliveryPrepared {
            reason_code,
            delivery_status,
            approval_state,
            ..
        } if reason_code == "delivery_prepared"
            && delivery_status == "prepared"
            && approval_state == "approved"
    )));
}

#[tokio::test]
async fn delivery_summary_denies_when_execution_evidence_is_missing() {
    let _audit = TestAuditGuard::new("delivery_summary_denies_when_execution_evidence_is_missing");
    let (_workspace, store, evo) = test_evo("delivery-summary-denied");
    let request = devloop_request("task-docs-delivery-denied", "docs/delivery-denied.md", true);
    let outcome = SupervisedDevloopOutcome {
        task_id: request.task.id.clone(),
        task_class: Some(BoundedTaskClass::DocsSingleFile),
        status: SupervisedDevloopStatus::Executed,
        execution_decision: SupervisedExecutionDecision::PlannerFallback,
        replay_outcome: None,
        fallback_reason: Some("replay_miss".into()),
        validation_outcome: SupervisedValidationOutcome::Passed,
        evidence_summary: "executed without retained feedback".into(),
        reason_code: Some(SupervisedExecutionReasonCode::ReplayFallback),
        recovery_hint: Some("retain execution feedback before delivery preparation".into()),
        execution_feedback: None,
        failure_contract: None,
        summary: "simulated executed outcome with missing feedback".into(),
    };

    let delivery = evo.prepare_supervised_delivery(&request, &outcome).unwrap();

    assert_eq!(delivery.delivery_status, SupervisedDeliveryStatus::Denied);
    assert_eq!(
        delivery.reason_code,
        SupervisedDeliveryReasonCode::DeliveryEvidenceMissing
    );
    assert!(delivery.fail_closed);
    assert!(store.scan(1).unwrap().iter().any(|stored| matches!(
        &stored.event,
        EvolutionEvent::MutationRejected {
            reason_code: Some(reason_code),
            fail_closed,
            ..
        } if reason_code == "delivery_evidence_missing" && *fail_closed
    )));
}

#[tokio::test]
async fn acceptance_gate_accepts_consistent_supervised_closed_loop() {
    let _audit = TestAuditGuard::new("acceptance_gate_accepts_consistent_supervised_closed_loop");
    let (_workspace, store, evo) = test_evo("acceptance-gate-success");
    let issue_request = github_issue_candidate_request(
        238,
        "OPEN",
        vec!["area/evolution", "type/feature"],
        vec!["docs/w8-acceptance-gate.md"],
    );
    let selection = evo.select_self_evolution_candidate(&issue_request).unwrap();
    let proposal = evo
        .prepare_self_evolution_mutation_proposal(&issue_request)
        .unwrap();
    let request = devloop_request(
        "task-w8-acceptance-gate",
        "docs/w8-acceptance-gate.md",
        true,
    );
    let outcome = evo
        .run_supervised_devloop(
            &"w8-acceptance-gate-run".to_string(),
            &request,
            proposal_diff_for("docs/w8-acceptance-gate.md", "W8 Acceptance Gate"),
            None,
        )
        .await
        .unwrap();
    let delivery = evo.prepare_supervised_delivery(&request, &outcome).unwrap();

    let gate = evo
        .evaluate_self_evolution_acceptance_gate(&SelfEvolutionAcceptanceGateInput {
            selection_decision: selection,
            proposal_contract: proposal,
            supervised_request: request,
            execution_outcome: outcome,
            delivery_contract: delivery,
        })
        .unwrap();

    assert_eq!(
        gate.audit_consistency_result,
        SelfEvolutionAuditConsistencyResult::Consistent
    );
    assert_eq!(
        gate.reason_code,
        SelfEvolutionAcceptanceGateReasonCode::Accepted
    );
    assert!(!gate.fail_closed);
    assert!(gate.approval_evidence.approved);
    assert_eq!(
        gate.reason_code_matrix.proposal_reason_code,
        MutationProposalContractReasonCode::Accepted
    );
    assert!(store.scan(1).unwrap().iter().any(|stored| matches!(
        &stored.event,
        EvolutionEvent::AcceptanceGateEvaluated {
            audit_consistency_result,
            fail_closed,
            reason_code,
            ..
        } if audit_consistency_result == "consistent"
            && !fail_closed
            && reason_code == "accepted"
    )));
}

#[tokio::test]
async fn acceptance_gate_fails_closed_when_reason_code_matrix_conflicts() {
    let _audit =
        TestAuditGuard::new("acceptance_gate_fails_closed_when_reason_code_matrix_conflicts");
    let (_workspace, store, evo) = test_evo("acceptance-gate-conflict");
    let issue_request = github_issue_candidate_request(
        239,
        "OPEN",
        vec!["area/evolution", "type/feature"],
        vec!["docs/w8-acceptance-conflict.md"],
    );
    let selection = evo.select_self_evolution_candidate(&issue_request).unwrap();
    let proposal = evo
        .prepare_self_evolution_mutation_proposal(&issue_request)
        .unwrap();
    let request = devloop_request(
        "task-w8-acceptance-conflict",
        "docs/w8-acceptance-conflict.md",
        true,
    );
    let mut outcome = evo
        .run_supervised_devloop(
            &"w8-acceptance-conflict-run".to_string(),
            &request,
            proposal_diff_for("docs/w8-acceptance-conflict.md", "W8 Acceptance Conflict"),
            None,
        )
        .await
        .unwrap();
    let delivery = evo.prepare_supervised_delivery(&request, &outcome).unwrap();
    outcome.reason_code = Some(SupervisedExecutionReasonCode::PolicyDenied);

    let gate = evo
        .evaluate_self_evolution_acceptance_gate(&SelfEvolutionAcceptanceGateInput {
            selection_decision: selection,
            proposal_contract: proposal,
            supervised_request: request,
            execution_outcome: outcome,
            delivery_contract: delivery,
        })
        .unwrap();

    assert_eq!(
        gate.audit_consistency_result,
        SelfEvolutionAuditConsistencyResult::Inconsistent
    );
    assert_eq!(
        gate.reason_code,
        SelfEvolutionAcceptanceGateReasonCode::InconsistentReasonCodeMatrix
    );
    assert!(gate.fail_closed);
    assert!(store.scan(1).unwrap().iter().any(|stored| matches!(
        &stored.event,
        EvolutionEvent::AcceptanceGateEvaluated {
            audit_consistency_result,
            fail_closed,
            reason_code,
            ..
        } if audit_consistency_result == "inconsistent"
            && *fail_closed
            && reason_code == "inconsistent_reason_code_matrix"
    )));
}

#[test]
fn candidate_intake_accepts_open_evolution_feature_docs_issue() {
    let _audit = TestAuditGuard::new("candidate_intake_accepts_open_evolution_feature_docs_issue");
    let (_workspace, _store, evo) = test_evo("candidate-intake-accept");
    let request = github_issue_candidate_request(
        234,
        "OPEN",
        vec!["area/evolution", "type/feature"],
        vec!["docs/w8-intake.md"],
    );

    let decision = evo.select_self_evolution_candidate(&request).unwrap();

    assert!(decision.selected);
    assert_eq!(
        decision.candidate_class,
        Some(BoundedTaskClass::DocsSingleFile)
    );
    assert_eq!(
        decision.reason_code,
        Some(SelfEvolutionSelectionReasonCode::Accepted)
    );
    assert_eq!(decision.failure_reason, None);
    assert_eq!(decision.recovery_hint, None);
    assert!(!decision.fail_closed);
}

#[test]
fn mutation_proposal_accepts_selected_candidate_and_declares_contract_shape() {
    let _audit = TestAuditGuard::new(
        "mutation_proposal_accepts_selected_candidate_and_declares_contract_shape",
    );
    let (_workspace, _store, evo) = test_evo("mutation-proposal-accept");
    let request = github_issue_candidate_request(
        235,
        "OPEN",
        vec!["area/evolution", "type/feature"],
        vec!["docs/w8-mutation-proposal.md"],
    );

    let contract = evo
        .prepare_self_evolution_mutation_proposal(&request)
        .unwrap();

    assert_eq!(
        contract.reason_code,
        MutationProposalContractReasonCode::Accepted
    );
    assert!(!contract.fail_closed);
    assert!(contract.approval_required);
    assert_eq!(
        contract
            .proposal_scope
            .as_ref()
            .map(|scope| scope.task_class.clone()),
        Some(BoundedTaskClass::DocsSingleFile)
    );
    assert_eq!(
        contract
            .proposal_scope
            .as_ref()
            .map(|scope| scope.target_files.clone()),
        Some(vec!["docs/w8-mutation-proposal.md".to_string()])
    );
    assert_eq!(
        contract.mutation_proposal.files,
        vec!["docs/w8-mutation-proposal.md".to_string()]
    );
    assert_eq!(
        contract.expected_evidence,
        vec![
            MutationProposalEvidence::HumanApproval,
            MutationProposalEvidence::BoundedScope,
            MutationProposalEvidence::ValidationPass,
            MutationProposalEvidence::ExecutionAudit,
        ]
    );
    assert_eq!(contract.validation_budget.max_diff_bytes, 128 * 1024);
    assert_eq!(contract.validation_budget.max_changed_lines, 600);
    assert!(contract.summary.contains("prepared"));
}

#[test]
fn mutation_proposal_rejects_out_of_bounds_candidate_scope_fail_closed() {
    let _audit =
        TestAuditGuard::new("mutation_proposal_rejects_out_of_bounds_candidate_scope_fail_closed");
    let (_workspace, _store, evo) = test_evo("mutation-proposal-out-of-bounds");
    let request = github_issue_candidate_request(
        236,
        "OPEN",
        vec!["area/evolution", "type/feature"],
        vec!["src/lib.rs"],
    );

    let contract = evo
        .prepare_self_evolution_mutation_proposal(&request)
        .unwrap();

    assert_eq!(
        contract.reason_code,
        MutationProposalContractReasonCode::OutOfBoundsPath
    );
    assert!(contract.fail_closed);
    assert!(contract.proposal_scope.is_none());
    assert!(contract
        .failure_reason
        .as_deref()
        .is_some_and(|reason| reason.contains("unsupported candidate scope")));
    assert!(contract.recovery_hint.is_some());
}

#[test]
fn candidate_intake_rejects_closed_issue_fail_closed() {
    let _audit = TestAuditGuard::new("candidate_intake_rejects_closed_issue_fail_closed");
    let (_workspace, _store, evo) = test_evo("candidate-intake-closed");
    let request = github_issue_candidate_request(
        235,
        "CLOSED",
        vec!["area/evolution", "type/feature"],
        vec!["docs/w8-intake.md"],
    );

    let decision = evo.select_self_evolution_candidate(&request).unwrap();

    assert!(!decision.selected);
    assert_eq!(decision.candidate_class, None);
    assert_eq!(
        decision.reason_code,
        Some(SelfEvolutionSelectionReasonCode::IssueClosed)
    );
    assert!(decision.fail_closed);
    assert!(decision
        .failure_reason
        .as_deref()
        .is_some_and(|reason| reason.contains("closed")));
    assert!(decision.recovery_hint.is_some());
}

#[test]
fn candidate_intake_rejects_missing_evolution_label() {
    let _audit = TestAuditGuard::new("candidate_intake_rejects_missing_evolution_label");
    let (_workspace, _store, evo) = test_evo("candidate-intake-missing-evolution");
    let request = github_issue_candidate_request(
        236,
        "OPEN",
        vec!["type/feature"],
        vec!["docs/w8-intake.md"],
    );

    let decision = evo.select_self_evolution_candidate(&request).unwrap();

    assert!(!decision.selected);
    assert_eq!(decision.candidate_class, None);
    assert_eq!(
        decision.reason_code,
        Some(SelfEvolutionSelectionReasonCode::MissingEvolutionLabel)
    );
    assert!(decision.fail_closed);
    assert!(decision.recovery_hint.is_some());
}

#[test]
fn candidate_intake_rejects_missing_feature_label() {
    let _audit = TestAuditGuard::new("candidate_intake_rejects_missing_feature_label");
    let (_workspace, _store, evo) = test_evo("candidate-intake-missing-feature");
    let request = github_issue_candidate_request(
        237,
        "OPEN",
        vec!["area/evolution"],
        vec!["docs/w8-intake.md"],
    );

    let decision = evo.select_self_evolution_candidate(&request).unwrap();

    assert!(!decision.selected);
    assert_eq!(decision.candidate_class, None);
    assert_eq!(
        decision.reason_code,
        Some(SelfEvolutionSelectionReasonCode::MissingFeatureLabel)
    );
    assert!(decision.fail_closed);
    assert!(decision.recovery_hint.is_some());
}

#[test]
fn candidate_intake_rejects_excluded_label() {
    let _audit = TestAuditGuard::new("candidate_intake_rejects_excluded_label");
    let (_workspace, _store, evo) = test_evo("candidate-intake-excluded-label");
    let request = github_issue_candidate_request(
        238,
        "OPEN",
        vec!["area/evolution", "type/feature", "duplicate"],
        vec!["docs/w8-intake.md"],
    );

    let decision = evo.select_self_evolution_candidate(&request).unwrap();

    assert!(!decision.selected);
    assert_eq!(decision.candidate_class, None);
    assert_eq!(
        decision.reason_code,
        Some(SelfEvolutionSelectionReasonCode::ExcludedByLabel)
    );
    assert!(decision.fail_closed);
    assert!(decision.recovery_hint.is_some());
}

#[test]
fn candidate_intake_rejects_unsupported_scope() {
    let _audit = TestAuditGuard::new("candidate_intake_rejects_unsupported_scope");
    let (_workspace, _store, evo) = test_evo("candidate-intake-unsupported-scope");
    let request = github_issue_candidate_request(
        239,
        "OPEN",
        vec!["area/evolution", "type/feature"],
        vec!["src/lib.rs"],
    );

    let decision = evo.select_self_evolution_candidate(&request).unwrap();

    assert!(!decision.selected);
    assert_eq!(decision.candidate_class, None);
    assert_eq!(
        decision.reason_code,
        Some(SelfEvolutionSelectionReasonCode::UnsupportedCandidateScope)
    );
    assert!(decision.fail_closed);
    assert!(decision
        .failure_reason
        .as_deref()
        .is_some_and(|reason| reason.contains("scope")));
}

#[tokio::test]
async fn mutation_proposal_rejects_missing_target_files_before_execution() {
    let _audit =
        TestAuditGuard::new("mutation_proposal_rejects_missing_target_files_before_execution");
    let (_workspace, store, evo) = test_evo("mutation-proposal-missing-target-files");
    let request = SupervisedDevloopRequest {
        task: AgentTask {
            id: "task-missing-target-files".into(),
            description: "Attempt to run with an empty proposal target list".into(),
        },
        proposal: MutationProposal {
            intent: "Update docs without declared target files".into(),
            files: Vec::new(),
            expected_effect: "Should be rejected before execution".into(),
        },
        approval: HumanApproval {
            approved: true,
            approver: Some("maintainer".into()),
            note: Some("regression test".into()),
        },
    };

    let outcome = evo
        .run_supervised_devloop(
            &"run-mutation-proposal-missing-target-files".to_string(),
            &request,
            proposal_diff_for("docs/unused.md", "Should Not Execute"),
            None,
        )
        .await
        .unwrap();

    assert_eq!(outcome.status, SupervisedDevloopStatus::RejectedByPolicy);
    assert_eq!(
        outcome
            .failure_contract
            .as_ref()
            .map(|contract| contract.reason_code),
        Some(MutationNeededFailureReasonCode::PolicyDenied)
    );
    let events = store.scan(1).unwrap();
    assert!(events.iter().any(|stored| matches!(
        &stored.event,
        EvolutionEvent::MutationRejected {
            reason_code: Some(reason_code),
            fail_closed,
            ..
        } if reason_code == "policy_denied" && *fail_closed
    )));
    assert!(!events
        .iter()
        .any(|stored| matches!(&stored.event, EvolutionEvent::MutationDeclared { .. })));
}

#[tokio::test]
async fn repeated_tasks_shift_from_fallback_to_replay_after_learning() {
    let _audit = TestAuditGuard::new("repeated_tasks_shift_from_fallback_to_replay_after_learning");
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
    let _audit = TestAuditGuard::new("normalized_signal_variants_can_replay_learned_capsule");
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
async fn adjacent_task_class_signal_variants_can_replay_learned_capsule() {
    let _audit =
        TestAuditGuard::new("adjacent_task_class_signal_variants_can_replay_learned_capsule");
    let (workspace, _store, evo) = test_evo("self-evolve-adjacent-task-class");

    let captured = evo
        .capture_successful_mutation(
            &"run-self-evolve-adjacent-task-class".to_string(),
            sample_mutation_with_id("mutation-self-evolve-adjacent-task-class"),
        )
        .await
        .unwrap();

    let decision = evo
        .replay_or_fallback(replay_input("README file missing", &workspace))
        .await
        .unwrap();
    let candidates = evo.select_candidates(&replay_input("README file missing", &workspace));

    assert!(decision.used_capsule);
    assert!(!decision.fallback_to_planner);
    assert_eq!(decision.capsule_id, Some(captured.id.clone()));
    assert!(!candidates.is_empty());
    assert_eq!(candidates[0].gene.id, captured.gene_id);
}

#[tokio::test]
async fn multi_signal_semantic_variants_keep_task_class_feedback_stable() {
    let _audit =
        TestAuditGuard::new("multi_signal_semantic_variants_keep_task_class_feedback_stable");
    let (workspace, _store, evo) = test_evo("self-evolve-multisignal-task-class");

    evo.capture_successful_mutation(
        &"run-self-evolve-multisignal-task-class".to_string(),
        sample_mutation_with_id("mutation-self-evolve-multisignal-task-class"),
    )
    .await
    .unwrap();

    let mut input = replay_input("README file absent", &workspace);
    input.signals = vec![
        "README file absent".to_string(),
        "repository readme unavailable".to_string(),
    ];
    let decision = evo.replay_or_fallback(input.clone()).await.unwrap();
    let feedback = EvoKernel::<TestState>::replay_feedback_for_agent(&input.signals, &decision);

    assert!(decision.used_capsule);
    assert!(!decision.fallback_to_planner);
    assert_eq!(
        feedback.task_class_id,
        decision.detect_evidence.task_class_id
    );
    assert_eq!(feedback.task_label, decision.detect_evidence.task_label);
    assert_eq!(feedback.task_label, "missing readme");
}

#[tokio::test]
async fn unrelated_task_class_signal_variants_do_not_replay() {
    let _audit = TestAuditGuard::new("unrelated_task_class_signal_variants_do_not_replay");
    let (workspace, _store, evo) = test_evo("self-evolve-negative-task-class");

    evo.capture_successful_mutation(
        &"run-self-evolve-negative-task-class".to_string(),
        sample_mutation_with_id("mutation-self-evolve-negative-task-class"),
    )
    .await
    .unwrap();

    let decision = evo
        .replay_or_fallback(replay_input("Cargo lock missing", &workspace))
        .await
        .unwrap();
    let candidates = evo.select_candidates(&replay_input("Cargo lock missing", &workspace));

    assert!(!decision.used_capsule);
    assert!(decision.fallback_to_planner);
    assert_eq!(decision.capsule_id, None);
    assert!(candidates.is_empty());
}

#[tokio::test]
async fn stale_confidence_forces_revalidation_before_replay() {
    let _audit = TestAuditGuard::new("stale_confidence_forces_revalidation_before_replay");
    let old_timestamp = (Utc::now() - Duration::hours(48)).to_rfc3339();
    let mutation = sample_mutation_with_id("mutation-stale-confidence");
    let workspace = temp_workspace();
    let capsule_env = replay_input("missing readme", &workspace).env;
    let gene = oris_evolution::Gene {
        id: "gene-stale-confidence".into(),
        signals: vec!["missing readme".into()],
        strategy: vec!["README.md".into()],
        validation: vec!["regression".into()],
        state: EvoAssetState::Promoted,
        task_class_id: None,
    };
    let capsule = oris_evolution::Capsule {
        id: "capsule-stale-confidence".into(),
        gene_id: gene.id.clone(),
        mutation_id: mutation.intent.id.clone(),
        run_id: "run-stale-confidence".into(),
        diff_hash: mutation.artifact.content_hash.clone(),
        confidence: 0.8,
        env: capsule_env,
        outcome: oris_evolution::Outcome {
            success: true,
            validation_profile: "regression".into(),
            validation_duration_ms: 1,
            changed_files: vec!["README.md".into()],
            validator_hash: "validator".into(),
            lines_changed: 1,
            replay_verified: false,
        },
        state: EvoAssetState::Promoted,
    };
    let store = Arc::new(SeededStore::new(vec![
        StoredEvolutionEvent {
            seq: 1,
            timestamp: old_timestamp.clone(),
            prev_hash: String::new(),
            record_hash: "seed-1".into(),
            event: EvolutionEvent::MutationDeclared {
                mutation: mutation.clone(),
            },
        },
        StoredEvolutionEvent {
            seq: 2,
            timestamp: old_timestamp.clone(),
            prev_hash: "seed-1".into(),
            record_hash: "seed-2".into(),
            event: EvolutionEvent::GeneProjected { gene: gene.clone() },
        },
        StoredEvolutionEvent {
            seq: 3,
            timestamp: old_timestamp,
            prev_hash: "seed-2".into(),
            record_hash: "seed-3".into(),
            event: EvolutionEvent::CapsuleCommitted {
                capsule: capsule.clone(),
            },
        },
    ]));
    let (_seeded_workspace, evo) = test_evo_with_seeded_store("stale-confidence", store.clone());

    let decision = evo
        .replay_or_fallback(replay_input("missing readme", &workspace))
        .await
        .unwrap();
    let projection = store.rebuild_projection().unwrap();
    let events = store.scan(1).unwrap();
    let metrics = evo.metrics_snapshot().unwrap();

    assert!(!decision.used_capsule);
    assert!(decision.fallback_to_planner);
    assert_eq!(decision.capsule_id, None);
    assert_eq!(projection.genes[0].state, EvoAssetState::Quarantined);
    assert_eq!(projection.capsules[0].state, EvoAssetState::Quarantined);
    assert!(events.iter().any(|stored| matches!(
        &stored.event,
        EvolutionEvent::PromotionEvaluated {
            gene_id,
            state,
            reason,
            reason_code,
            ..
        }
            if gene_id == &gene.id
                && *state == EvoAssetState::Quarantined
                && reason_code == &TransitionReasonCode::RevalidationConfidenceDecay
                && reason.contains("confidence decayed")
    )));
    let revalidation_evidence = events.iter().find_map(|stored| match &stored.event {
        EvolutionEvent::PromotionEvaluated {
            gene_id,
            state: EvoAssetState::Quarantined,
            reason_code,
            evidence,
            ..
        } if gene_id == &gene.id
            && reason_code == &TransitionReasonCode::RevalidationConfidenceDecay =>
        {
            evidence.clone()
        }
        _ => None,
    });
    let revalidation_evidence =
        revalidation_evidence.expect("expected confidence revalidation evidence");
    assert!(
        revalidation_evidence
            .decayed_confidence
            .expect("expected decayed confidence")
            < MIN_REPLAY_CONFIDENCE
    );
    assert!(revalidation_evidence
        .summary
        .as_deref()
        .unwrap_or_default()
        .contains("phase=confidence_revalidation"));
    assert_eq!(metrics.confidence_revalidations_total, 1);
}

#[tokio::test]
async fn long_repeated_sequence_reports_stable_replay_metrics_after_learning() {
    let _audit =
        TestAuditGuard::new("long_repeated_sequence_reports_stable_replay_metrics_after_learning");
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
    let _audit =
        TestAuditGuard::new("remote_learning_requires_local_validation_before_becoming_shareable");
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
    let events_after_import = consumer_store.scan(1).unwrap();
    let before = consumer_store.rebuild_projection().unwrap();
    let before_publish = oris_evokernel::EvolutionNetworkNode::new(consumer_store.clone())
        .publish_local_assets("node-consumer")
        .unwrap();

    assert!(import.accepted);
    assert!(!import.imported_asset_ids.is_empty());
    assert!(events_after_import.iter().any(|stored| matches!(
        &stored.event,
        EvolutionEvent::ManifestValidated {
            accepted: true,
            reason,
            sender_id: Some(sender_id),
            publisher: Some(publisher),
            asset_ids,
        } if reason == "manifest validated"
            && sender_id == "node-producer"
            && publisher == "node-producer"
            && !asset_ids.is_empty()
    )));
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

    let first_decision = consumer
        .replay_or_fallback(replay_input("missing readme", &consumer_workspace))
        .await
        .unwrap();
    let after_first = consumer_store.rebuild_projection().unwrap();
    let after_first_publish = oris_evokernel::EvolutionNetworkNode::new(consumer_store.clone())
        .publish_local_assets("node-consumer")
        .unwrap();

    assert!(first_decision.used_capsule);
    assert_eq!(first_decision.capsule_id, Some(captured.id.clone()));
    let shadow_gene = after_first
        .genes
        .iter()
        .find(|gene| gene.id == captured.gene_id)
        .unwrap();
    let shadow_capsule = after_first
        .capsules
        .iter()
        .find(|capsule| capsule.id == captured.id)
        .unwrap();
    assert_eq!(shadow_gene.state, EvoAssetState::ShadowValidated);
    assert_eq!(shadow_capsule.state, EvoAssetState::ShadowValidated);
    assert!(after_first_publish.assets.is_empty());

    let second_decision = consumer
        .replay_or_fallback(replay_input("missing readme", &consumer_workspace))
        .await
        .unwrap();
    let after_second = consumer_store.rebuild_projection().unwrap();
    let after_second_publish = oris_evokernel::EvolutionNetworkNode::new(consumer_store.clone())
        .publish_local_assets("node-consumer")
        .unwrap();

    assert!(second_decision.used_capsule);
    assert_eq!(second_decision.capsule_id, Some(captured.id.clone()));
    let promoted_gene = after_second
        .genes
        .iter()
        .find(|gene| gene.id == captured.gene_id)
        .unwrap();
    let promoted_capsule = after_second
        .capsules
        .iter()
        .find(|capsule| capsule.id == captured.id)
        .unwrap();
    assert_eq!(promoted_gene.state, EvoAssetState::Promoted);
    assert_eq!(promoted_capsule.state, EvoAssetState::Promoted);
    assert!(after_second_publish.assets.iter().any(|asset| matches!(
        asset,
        oris_evokernel::evolution_network::NetworkAsset::Gene { gene }
            if gene.id == captured.gene_id
    )));
    assert!(after_second_publish.assets.iter().any(|asset| matches!(
        asset,
        oris_evokernel::evolution_network::NetworkAsset::Capsule { capsule }
            if capsule.id == captured.id
    )));
}

#[tokio::test]
async fn distributed_learning_survives_restart_and_replays_again() {
    let _audit = TestAuditGuard::new("distributed_learning_survives_restart_and_replays_again");
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
async fn remote_revoke_notice_revokes_owned_imported_assets() {
    let _audit = TestAuditGuard::new("remote_revoke_notice_revokes_owned_imported_assets");
    let (_producer_workspace, producer_store, producer) = test_evo("remote-revoke-owner-producer");
    let captured = producer
        .capture_successful_mutation(
            &"run-remote-revoke-owner-producer".to_string(),
            sample_mutation_with_id("mutation-remote-revoke-owner-producer"),
        )
        .await
        .unwrap();
    let publish = oris_evokernel::EvolutionNetworkNode::new(producer_store.clone())
        .publish_local_assets("node-producer")
        .unwrap();

    let consumer_store = Arc::new(JsonlEvolutionStore::new(unique_path(
        "remote-revoke-owner-consumer-store",
    )));
    let (_consumer_workspace, consumer) =
        test_evo_with_store("remote-revoke-owner-consumer", consumer_store.clone());
    consumer.import_remote_envelope(&publish).unwrap();

    let outcome = consumer
        .revoke_assets(&oris_evokernel::evolution_network::RevokeNotice {
            sender_id: "node-producer".into(),
            asset_ids: vec![captured.id.clone()],
            reason: "producer requested remote revoke".into(),
        })
        .unwrap();

    assert_eq!(outcome.sender_id, "node-producer");
    assert!(outcome.asset_ids.contains(&captured.gene_id));
    assert!(outcome.asset_ids.contains(&captured.id));

    let projection = consumer_store.rebuild_projection().unwrap();
    let revoked_gene = projection
        .genes
        .iter()
        .find(|gene| gene.id == captured.gene_id)
        .unwrap();
    let quarantined_capsule = projection
        .capsules
        .iter()
        .find(|capsule| capsule.id == captured.id)
        .unwrap();

    assert_eq!(revoked_gene.state, EvoAssetState::Revoked);
    assert_eq!(quarantined_capsule.state, EvoAssetState::Quarantined);
}

#[tokio::test]
async fn remote_revoke_notice_fails_closed_on_mixed_sender_assets() {
    let _audit = TestAuditGuard::new("remote_revoke_notice_fails_closed_on_mixed_sender_assets");
    let (_producer_a_workspace, producer_a_store, producer_a) = test_evo("remote-revoke-mixed-a");
    let captured_a = producer_a
        .capture_successful_mutation(
            &"run-remote-revoke-mixed-a".to_string(),
            sample_mutation_with_id("mutation-remote-revoke-mixed-a"),
        )
        .await
        .unwrap();
    let publish_a = oris_evokernel::EvolutionNetworkNode::new(producer_a_store.clone())
        .publish_local_assets("node-a")
        .unwrap();

    let (_producer_b_workspace, producer_b_store, producer_b) = test_evo("remote-revoke-mixed-b");
    let captured_b = producer_b
        .capture_successful_mutation(
            &"run-remote-revoke-mixed-b".to_string(),
            sample_mutation_with_id("mutation-remote-revoke-mixed-b"),
        )
        .await
        .unwrap();
    let publish_b = oris_evokernel::EvolutionNetworkNode::new(producer_b_store.clone())
        .publish_local_assets("node-b")
        .unwrap();

    let consumer_store = Arc::new(JsonlEvolutionStore::new(unique_path(
        "remote-revoke-mixed-consumer-store",
    )));
    let (_consumer_workspace, consumer) =
        test_evo_with_store("remote-revoke-mixed-consumer", consumer_store.clone());
    consumer.import_remote_envelope(&publish_a).unwrap();
    consumer.import_remote_envelope(&publish_b).unwrap();

    let before_events = consumer_store.scan(1).unwrap();
    let before_revokes = before_events
        .iter()
        .filter(|stored| matches!(&stored.event, EvolutionEvent::GeneRevoked { .. }))
        .count();
    let before_quarantines = before_events
        .iter()
        .filter(|stored| matches!(&stored.event, EvolutionEvent::CapsuleQuarantined { .. }))
        .count();

    let error = consumer
        .revoke_assets(&oris_evokernel::evolution_network::RevokeNotice {
            sender_id: "node-a".into(),
            asset_ids: vec![captured_a.id.clone(), captured_b.id.clone()],
            reason: "mixed ownership revoke request".into(),
        })
        .unwrap_err();

    assert!(
        error.to_string().contains("owned"),
        "expected ownership validation error, got {error}"
    );

    let after_events = consumer_store.scan(1).unwrap();
    let after_revokes = after_events
        .iter()
        .filter(|stored| matches!(&stored.event, EvolutionEvent::GeneRevoked { .. }))
        .count();
    let after_quarantines = after_events
        .iter()
        .filter(|stored| matches!(&stored.event, EvolutionEvent::CapsuleQuarantined { .. }))
        .count();

    assert_eq!(after_revokes, before_revokes);
    assert_eq!(after_quarantines, before_quarantines);

    let projection = consumer_store.rebuild_projection().unwrap();
    assert!(projection
        .genes
        .iter()
        .filter(|gene| gene.id == captured_a.gene_id || gene.id == captured_b.gene_id)
        .all(|gene| gene.state == EvoAssetState::Quarantined));
}

#[tokio::test]
async fn remote_replay_failure_revocation_evidence_names_source_sender() {
    let _audit =
        TestAuditGuard::new("remote_replay_failure_revocation_evidence_names_source_sender");
    let (_producer_workspace, producer_store, producer) =
        test_evo("remote-replay-revocation-source-producer");
    let captured = producer
        .capture_successful_mutation(
            &"run-remote-replay-revocation-source-producer".to_string(),
            sample_mutation_with_id("mutation-remote-replay-revocation-source-producer"),
        )
        .await
        .unwrap();
    let publish = oris_evokernel::EvolutionNetworkNode::new(producer_store.clone())
        .publish_local_assets("node-remote")
        .unwrap();

    let (consumer_workspace, consumer_store, consumer) = test_evo_with_policy_and_plan(
        "remote-replay-revocation-source-consumer",
        sandbox_policy(),
        failing_plan(),
    );
    consumer.import_remote_envelope(&publish).unwrap();

    let first = consumer
        .replay_or_fallback(replay_input("missing readme", &consumer_workspace))
        .await
        .unwrap();
    let second = consumer
        .replay_or_fallback(replay_input("missing readme", &consumer_workspace))
        .await
        .unwrap();

    assert!(first.fallback_to_planner);
    assert!(second.fallback_to_planner);

    let events = consumer_store.scan(1).unwrap();
    let summary = events
        .iter()
        .find_map(|stored| match &stored.event {
            EvolutionEvent::PromotionEvaluated {
                gene_id,
                state: EvoAssetState::Revoked,
                evidence: Some(evidence),
                ..
            } if gene_id == &captured.gene_id => evidence.summary.clone(),
            _ => None,
        })
        .unwrap_or_default();

    assert!(
        summary.contains("phase=replay_failure_revocation"),
        "expected replay failure phase summary, got {summary}"
    );
    assert!(
        summary.contains("source_sender_id=node-remote"),
        "expected remote source sender summary, got {summary}"
    );
}

#[tokio::test]
async fn unrelated_tasks_do_not_false_positive_after_learning() {
    let _audit = TestAuditGuard::new("unrelated_tasks_do_not_false_positive_after_learning");
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
    let _audit = TestAuditGuard::new("mixed_task_sequence_only_replays_for_learned_signals");
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
    assert_eq!(metrics.replay_attempts_total, (hits + misses) as u64);
    assert_eq!(metrics.replay_success_total, hits as u64);
    assert_eq!(
        metrics.replay_success_rate,
        hits as f64 / (hits + misses) as f64
    );
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

#[tokio::test]
async fn task_class_false_positive_goldens_keep_false_replay_rate_zero() {
    let _audit =
        TestAuditGuard::new("task_class_false_positive_goldens_keep_false_replay_rate_zero");
    let (workspace, _store, evo) = test_evo("self-evolve-task-class-fp-goldens");

    let captured = evo
        .capture_successful_mutation(
            &"run-self-evolve-task-class-fp-goldens".to_string(),
            sample_mutation_with_id("mutation-self-evolve-task-class-fp-goldens"),
        )
        .await
        .unwrap();

    let goldens = [
        ("near_signal", "missing readmee"),
        ("single_token_overlap", "missing cargo"),
        ("cross_domain_noise", "network timeout in grpc gateway"),
    ];
    let total_cases = goldens.len() as u64;
    let mut false_hits = 0_u64;
    let mut cases = Vec::<Value>::new();

    for (case_id, signal) in goldens {
        let input_for_select = replay_input(signal, &workspace);
        let candidates = evo.select_candidates(&input_for_select);
        let decision = evo
            .replay_or_fallback(replay_input(signal, &workspace))
            .await
            .unwrap();
        let feedback =
            EvoKernel::<TestState>::replay_feedback_for_agent(&[signal.to_string()], &decision);

        let false_hit = !candidates.is_empty() || decision.used_capsule;
        if false_hit {
            false_hits += 1;
        }

        assert!(
            candidates.is_empty(),
            "case={case_id} should not produce select candidates"
        );
        assert!(
            !decision.used_capsule,
            "case={case_id} should not replay-hit capsule_id={:?}",
            decision.capsule_id
        );
        assert!(decision.fallback_to_planner, "case={case_id} must fallback");
        assert_eq!(decision.reason, "no matching gene", "case={case_id}");
        assert_eq!(
            feedback.reason_code,
            Some(ReplayFallbackReasonCode::NoCandidateAfterSelect),
            "case={case_id}"
        );
        assert_eq!(
            feedback.planner_directive,
            ReplayPlannerDirective::PlanFallback,
            "case={case_id}"
        );
        assert_eq!(
            feedback.next_action,
            Some(ReplayFallbackNextAction::PlanFromScratch),
            "case={case_id}"
        );

        cases.push(json!({
            "case_id": case_id,
            "signal": signal,
            "select_candidates": candidates.len(),
            "used_capsule": decision.used_capsule,
            "fallback_to_planner": decision.fallback_to_planner,
            "reason": decision.reason,
            "reason_code": feedback.reason_code.map(|code| format!("{:?}", code)),
            "planner_directive": format!("{:?}", feedback.planner_directive),
            "next_action": feedback.next_action.map(|action| format!("{:?}", action)),
        }));
    }

    let false_replay_rate = false_hits as f64 / total_cases as f64;
    let payload = json!({
        "suite": "task-class-false-positive-goldens",
        "captured_capsule_id": captured.id,
        "total_cases": total_cases,
        "false_hits": false_hits,
        "false_replay_rate": false_replay_rate,
        "cases": cases,
    });
    let snapshot_path =
        write_false_positive_snapshot("task-class-false-positive-goldens", &payload);
    assert!(snapshot_path.exists());
    assert_eq!(
        false_hits,
        0,
        "task-class false-positive drift detected; snapshot={}",
        snapshot_path.display()
    );
}

#[test]
fn bootstrap_if_empty_seeds_exactly_four_genes_and_four_capsules() {
    let _audit =
        TestAuditGuard::new("bootstrap_if_empty_seeds_exactly_four_genes_and_four_capsules");
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
    let _audit = TestAuditGuard::new("bootstrap_capsules_start_quarantined");
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
    let _audit = TestAuditGuard::new("bootstrap_if_empty_is_idempotent");
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
    let _audit =
        TestAuditGuard::new("bootstrap_appends_records_without_overwriting_existing_events");
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
    let _audit = TestAuditGuard::new(
        "bootstrap_seeds_remain_quarantined_and_do_not_appear_as_replay_candidates",
    );
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
    let _audit = TestAuditGuard::new("sandbox_boundary_blocks_out_of_scope_patch");
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
    let _audit = TestAuditGuard::new("governor_blast_radius_gate_blocks_promotion_and_replay");
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
    let _audit = TestAuditGuard::new("governor_rate_limit_blocks_rapid_successive_mutations");
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
    let _audit = TestAuditGuard::new("governor_cooling_window_blocks_rapid_repromotion");
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
    let _audit = TestAuditGuard::new("local_capture_uses_existing_confidence_context_for_governor");
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
    let revocation_evidence = events.iter().find_map(|stored| match &stored.event {
        EvolutionEvent::PromotionEvaluated {
            gene_id,
            state: EvoAssetState::Revoked,
            reason_code,
            evidence,
            ..
        } if gene_id == &second.gene.id
            && reason_code == &TransitionReasonCode::DowngradeConfidenceRegression =>
        {
            evidence.clone()
        }
        _ => None,
    });
    let revocation_evidence =
        revocation_evidence.expect("expected confidence regression revocation evidence");
    assert!(
        revocation_evidence
            .confidence_decay_ratio
            .expect("expected confidence decay ratio")
            < 1.0
    );
    assert!(revocation_evidence
        .summary
        .as_deref()
        .unwrap_or_default()
        .contains("phase=confidence_regression"));
    assert!(events.iter().any(|stored| matches!(
        &stored.event,
        EvolutionEvent::GeneRevoked { gene_id, .. } if gene_id == &second.gene.id
    )));
    assert_eq!(metrics.gene_revocations_total, 1);
}

#[tokio::test]
async fn failed_replay_stops_immediate_reuse_without_revocation() {
    let _audit = TestAuditGuard::new("failed_replay_stops_immediate_reuse_without_revocation");
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

#[test]
fn evomap_snapshot_can_be_loaded_and_mapped() {
    let _audit = TestAuditGuard::new("evomap_snapshot_can_be_loaded_and_mapped");
    let store_root = unique_path("evomap-snapshot-mapped-store");
    let store = Arc::new(JsonlEvolutionStore::new(&store_root));
    let node = oris_evokernel::EvolutionNetworkNode::new(store.clone());

    let import = node
        .ensure_builtin_experience_assets("runtime-bootstrap")
        .unwrap();
    assert!(!import.imported_asset_ids.is_empty());

    let (_, projection) = EvoEvolutionStore::scan_projection(store.as_ref()).unwrap();
    let evomap_gene_ids = projection
        .genes
        .iter()
        .filter(|gene| {
            strategy_metadata_value(&gene.strategy, "asset_origin").as_deref()
                == Some("builtin_evomap")
        })
        .map(|gene| gene.id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let evomap_capsules = projection
        .capsules
        .iter()
        .filter(|capsule| evomap_gene_ids.contains(&capsule.gene_id))
        .count();

    assert!(!evomap_gene_ids.is_empty());
    assert!(evomap_capsules > 0);
}

#[test]
fn ensure_builtin_assets_imports_genes_and_capsules() {
    let _audit = TestAuditGuard::new("ensure_builtin_assets_imports_genes_and_capsules");
    let store_root = unique_path("evomap-imports-store");
    let store = Arc::new(JsonlEvolutionStore::new(&store_root));
    let node = oris_evokernel::EvolutionNetworkNode::new(store.clone());

    node.ensure_builtin_experience_assets("runtime-bootstrap")
        .unwrap();
    let (_, projection) = EvoEvolutionStore::scan_projection(store.as_ref()).unwrap();

    let promoted_gene_ids = projection
        .genes
        .iter()
        .filter(|gene| {
            gene.state == EvoAssetState::Promoted
                && strategy_metadata_value(&gene.strategy, "asset_origin").as_deref()
                    == Some("builtin_evomap")
        })
        .map(|gene| gene.id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let promoted_capsules = projection
        .capsules
        .iter()
        .filter(|capsule| {
            capsule.state == EvoAssetState::Promoted && promoted_gene_ids.contains(&capsule.gene_id)
        })
        .count();

    assert!(!promoted_gene_ids.is_empty());
    assert!(promoted_capsules > 0);
}

#[test]
fn ensure_builtin_assets_is_idempotent_with_snapshot() {
    let _audit = TestAuditGuard::new("ensure_builtin_assets_is_idempotent_with_snapshot");
    let store_root = unique_path("evomap-idempotent-store");
    let store = Arc::new(JsonlEvolutionStore::new(&store_root));
    let node = oris_evokernel::EvolutionNetworkNode::new(store.clone());

    let first = node
        .ensure_builtin_experience_assets("runtime-bootstrap")
        .unwrap();
    assert!(!first.imported_asset_ids.is_empty());

    let second = node
        .ensure_builtin_experience_assets("runtime-bootstrap")
        .unwrap();
    assert!(second.imported_asset_ids.is_empty());
}

#[test]
fn builtin_evomap_assets_are_fetchable() {
    let _audit = TestAuditGuard::new("builtin_evomap_assets_are_fetchable");
    let store_root = unique_path("evomap-fetch-store");
    let store = Arc::new(JsonlEvolutionStore::new(&store_root));
    let node = oris_evokernel::EvolutionNetworkNode::new(store.clone());

    node.ensure_builtin_experience_assets("runtime-bootstrap")
        .unwrap();

    let fetch = node
        .fetch_assets(
            "execution-api",
            &oris_evokernel::FetchQuery {
                sender_id: "compat-agent".into(),
                signals: vec!["error".into()],
                since_cursor: None,
                resume_token: None,
            },
        )
        .unwrap();

    let has_gene = fetch.assets.iter().any(|asset| {
        matches!(
            asset,
            oris_evolution_network::NetworkAsset::Gene { gene }
                if strategy_metadata_value(&gene.strategy, "asset_origin").as_deref() == Some("builtin_evomap")
                    && gene.state == EvoAssetState::Promoted
        )
    });
    let has_capsule = fetch.assets.iter().any(|asset| {
        matches!(
            asset,
            oris_evolution_network::NetworkAsset::Capsule { capsule }
                if capsule.state == EvoAssetState::Promoted
        )
    });
    assert!(has_gene);
    assert!(has_capsule);
}

#[test]
fn builtin_evomap_replay_path_has_declared_mutation() {
    let _audit = TestAuditGuard::new("builtin_evomap_replay_path_has_declared_mutation");
    let store_root = unique_path("evomap-mutation-store");
    let store = Arc::new(JsonlEvolutionStore::new(&store_root));
    let node = oris_evokernel::EvolutionNetworkNode::new(store.clone());

    node.ensure_builtin_experience_assets("runtime-bootstrap")
        .unwrap();

    let events = store.scan(1).unwrap();
    let (_, projection) = EvoEvolutionStore::scan_projection(store.as_ref()).unwrap();
    let capsule_mutation_ids = projection
        .capsules
        .iter()
        .map(|capsule| capsule.mutation_id.clone())
        .collect::<std::collections::BTreeSet<_>>();

    assert!(!capsule_mutation_ids.is_empty());
    for mutation_id in capsule_mutation_ids {
        assert!(events.iter().any(|stored| {
            matches!(
                &stored.event,
                EvolutionEvent::MutationDeclared { mutation }
                    if mutation.intent.id == mutation_id
            )
        }));
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// EVO26-AUTO-01: autonomous candidate intake regression tests
// ──────────────────────────────────────────────────────────────────────────────

fn make_evo_kernel_for_autonomous_intake(label: &str) -> EvoKernel<TestState> {
    let (_, _, evo) = test_evo(label);
    evo
}

#[test]
fn autonomous_intake_accepts_compile_regression_signal() {
    let kernel = make_evo_kernel_for_autonomous_intake(
        "autonomous_intake_accepts_compile_regression_signal",
    );
    let input = AutonomousIntakeInput {
        source_id: "ci-run-001".to_string(),
        candidate_source: AutonomousCandidateSource::CompileRegression,
        raw_signals: vec![
            "error[E0308]: mismatched types".to_string(),
            "  --> crates/oris-runtime/src/lib.rs:42:5".to_string(),
        ],
    };

    let output = kernel.discover_autonomous_candidates(&input);
    assert_eq!(output.accepted_count, 1, "expected one accepted candidate");
    assert_eq!(output.denied_count, 0);
    let candidate = &output.candidates[0];
    assert!(candidate.accepted, "candidate should be accepted");
    assert_eq!(candidate.reason_code, AutonomousIntakeReasonCode::Accepted);
    assert!(!candidate.fail_closed);
    assert_eq!(candidate.candidate_class, Some(BoundedTaskClass::LintFix));
    assert!(!candidate.dedupe_key.is_empty());
}

#[test]
fn autonomous_intake_accepts_test_failure_signal() {
    let kernel =
        make_evo_kernel_for_autonomous_intake("autonomous_intake_accepts_test_failure_signal");
    let input = AutonomousIntakeInput {
        source_id: "ci-run-002".to_string(),
        candidate_source: AutonomousCandidateSource::TestRegression,
        raw_signals: vec![
            "test evolution_lifecycle_regression::some_test ... FAILED".to_string(),
            "failures: evolution_lifecycle_regression::some_test".to_string(),
        ],
    };

    let output = kernel.discover_autonomous_candidates(&input);
    assert_eq!(output.accepted_count, 1);
    let candidate = &output.candidates[0];
    assert!(candidate.accepted);
    assert_eq!(candidate.candidate_class, Some(BoundedTaskClass::LintFix));
}

#[test]
fn autonomous_intake_deduplicates_equivalent_signals() {
    let kernel =
        make_evo_kernel_for_autonomous_intake("autonomous_intake_deduplicates_equivalent_signals");
    let signals = vec![
        "  error[E0308]: mismatched types  ".to_string(), // extra whitespace
        "error[E0308]: mismatched types".to_string(),     // duplicate after trim
    ];
    let input = AutonomousIntakeInput {
        source_id: "ci-run-003".to_string(),
        candidate_source: AutonomousCandidateSource::LintRegression,
        raw_signals: signals,
    };

    let output = kernel.discover_autonomous_candidates(&input);
    // After normalisation the two tokens collapse into one; still accepted.
    assert_eq!(
        output.accepted_count, 1,
        "deduplicated signals still accepted"
    );
    assert_eq!(output.candidates.len(), 1);
    // Normalised signals should have no duplicates.
    let cand = &output.candidates[0];
    let mut seen = std::collections::BTreeSet::new();
    for s in &cand.signals {
        assert!(
            seen.insert(s.clone()),
            "signal {s:?} appears twice after dedupe"
        );
    }
}

#[test]
fn autonomous_intake_denies_empty_signals_fail_closed() {
    let kernel =
        make_evo_kernel_for_autonomous_intake("autonomous_intake_denies_empty_signals_fail_closed");
    let input = AutonomousIntakeInput {
        source_id: "ci-run-empty".to_string(),
        candidate_source: AutonomousCandidateSource::CiFailure,
        raw_signals: vec![],
    };

    let output = kernel.discover_autonomous_candidates(&input);
    assert_eq!(output.accepted_count, 0);
    assert_eq!(output.denied_count, 1);
    let candidate = &output.candidates[0];
    assert!(!candidate.accepted);
    assert!(candidate.fail_closed, "empty signals must fail closed");
    assert_eq!(
        candidate.reason_code,
        AutonomousIntakeReasonCode::UnknownFailClosed
    );
}

#[test]
fn autonomous_intake_denies_ambiguous_signals_fail_closed() {
    let kernel = make_evo_kernel_for_autonomous_intake(
        "autonomous_intake_denies_ambiguous_signals_fail_closed",
    );
    // RuntimeIncident is currently unsupported / maps to None in classify_autonomous_signals.
    let input = AutonomousIntakeInput {
        source_id: "incident-001".to_string(),
        candidate_source: AutonomousCandidateSource::RuntimeIncident,
        raw_signals: vec![
            "PANIC: index out of bounds".to_string(),
            "thread 'tokio-worker' panicked".to_string(),
        ],
    };

    let output = kernel.discover_autonomous_candidates(&input);
    assert_eq!(output.accepted_count, 0);
    assert_eq!(output.denied_count, 1);
    let candidate = &output.candidates[0];
    assert!(!candidate.accepted);
    assert!(candidate.fail_closed, "unsupported source must fail closed");
    assert_eq!(
        candidate.reason_code,
        AutonomousIntakeReasonCode::AmbiguousSignal
    );
    assert!(candidate.candidate_class.is_none());
}

// ── AUTO-02: Bounded task planning and risk scoring ───────────────────────────

#[test]
fn autonomous_planning_approves_lint_fix_candidate() {
    let kernel = make_evo_kernel_for_autonomous_intake("autonomous_planning_approves_lint_fix");
    let intake_input = AutonomousIntakeInput {
        source_id: "ci-run-plan-001".to_string(),
        candidate_source: AutonomousCandidateSource::LintRegression,
        raw_signals: vec!["error[E0308]: mismatched types".to_string()],
    };
    let output = kernel.discover_autonomous_candidates(&intake_input);
    assert_eq!(output.accepted_count, 1, "precondition: intake accepted");
    let candidate = &output.candidates[0];

    let plan = kernel.plan_autonomous_candidate(candidate);
    assert!(plan.approved, "LintFix candidate must be approved");
    assert_eq!(plan.reason_code, AutonomousPlanReasonCode::Approved);
    assert_eq!(plan.task_class, Some(BoundedTaskClass::LintFix));
    assert!(
        plan.risk_tier <= AutonomousRiskTier::Medium,
        "LintFix must be low/medium risk"
    );
    assert!(
        plan.feasibility_score >= 40,
        "feasibility must meet policy floor"
    );
    assert!(!plan.plan_id.is_empty(), "plan_id must be set");
    assert!(!plan.dedupe_key.is_empty(), "dedupe_key must be set");
    assert!(!plan.fail_closed, "approved plan must not be fail_closed");
    assert!(
        !plan.expected_evidence.is_empty(),
        "must list expected evidence"
    );
}

#[test]
fn autonomous_planning_denies_denied_candidate_fail_closed() {
    let kernel =
        make_evo_kernel_for_autonomous_intake("autonomous_planning_denies_denied_candidate");
    let intake_input = AutonomousIntakeInput {
        source_id: "ci-run-plan-002".to_string(),
        candidate_source: AutonomousCandidateSource::CiFailure,
        raw_signals: vec![],
    };
    let output = kernel.discover_autonomous_candidates(&intake_input);
    assert_eq!(output.denied_count, 1, "precondition: intake denied");
    let candidate = &output.candidates[0];

    let plan = kernel.plan_autonomous_candidate(candidate);
    assert!(!plan.approved, "denied intake must yield denied plan");
    assert_eq!(plan.reason_code, AutonomousPlanReasonCode::DeniedNoEvidence);
    assert!(plan.fail_closed, "denied plan must be fail_closed");
    assert!(
        plan.denial_condition.is_some(),
        "denial_condition must be populated"
    );
}

#[test]
fn autonomous_planning_approves_docs_single_file_candidate() {
    use oris_agent_contract::accept_discovered_candidate;

    let kernel = make_evo_kernel_for_autonomous_intake("autonomous_planning_approves_docs");
    let candidate = accept_discovered_candidate(
        "docs-key-001",
        AutonomousCandidateSource::LintRegression,
        BoundedTaskClass::DocsSingleFile,
        vec!["doc comment outdated in src/lib.rs".to_string()],
        None,
    );

    let plan = kernel.plan_autonomous_candidate(&candidate);
    assert!(plan.approved, "DocsSingleFile candidate must be approved");
    assert_eq!(plan.reason_code, AutonomousPlanReasonCode::Approved);
    assert_eq!(plan.task_class, Some(BoundedTaskClass::DocsSingleFile));
    assert!(
        plan.risk_tier <= AutonomousRiskTier::Medium,
        "DocsSingleFile must be low/medium risk"
    );
    assert!(
        plan.feasibility_score >= 40,
        "feasibility must meet policy floor"
    );
    assert!(!plan.fail_closed, "approved plan must not be fail_closed");
    assert!(
        !plan.expected_evidence.is_empty(),
        "must list expected evidence"
    );
}

#[test]
fn autonomous_planning_denies_missing_class_fail_closed() {
    use oris_agent_contract::DiscoveredCandidate;

    // Craft a candidate that is accepted but has no class.
    let candidate = DiscoveredCandidate {
        dedupe_key: "test-no-class-key".to_string(),
        candidate_source: AutonomousCandidateSource::LintRegression,
        signals: vec!["some signal".to_string()],
        candidate_class: None,
        accepted: true,
        reason_code: AutonomousIntakeReasonCode::Accepted,
        summary: "test candidate".to_string(),
        failure_reason: None,
        recovery_hint: None,
        fail_closed: false,
    };
    let kernel = make_evo_kernel_for_autonomous_intake("autonomous_planning_denies_no_class");
    let plan = kernel.plan_autonomous_candidate(&candidate);

    assert!(!plan.approved, "missing class must be denied");
    assert_eq!(
        plan.reason_code,
        AutonomousPlanReasonCode::DeniedUnsupportedClass
    );
    assert!(plan.fail_closed, "must be fail_closed");
    assert!(plan.denial_condition.is_some());
}

#[test]
fn autonomous_planning_reason_codes_are_stable() {
    // Verify discriminant stability so wire format never silently shifts.
    assert_eq!(
        format!("{:?}", AutonomousPlanReasonCode::Approved),
        "Approved"
    );
    assert_eq!(
        format!("{:?}", AutonomousPlanReasonCode::DeniedHighRisk),
        "DeniedHighRisk"
    );
    assert_eq!(
        format!("{:?}", AutonomousPlanReasonCode::DeniedLowFeasibility),
        "DeniedLowFeasibility"
    );
    assert_eq!(
        format!("{:?}", AutonomousPlanReasonCode::DeniedUnsupportedClass),
        "DeniedUnsupportedClass"
    );
    assert_eq!(
        format!("{:?}", AutonomousPlanReasonCode::DeniedNoEvidence),
        "DeniedNoEvidence"
    );
    assert_eq!(
        format!("{:?}", AutonomousPlanReasonCode::UnknownFailClosed),
        "UnknownFailClosed"
    );
    assert!(AutonomousRiskTier::Low < AutonomousRiskTier::Medium);
    assert!(AutonomousRiskTier::Medium < AutonomousRiskTier::High);
}

// ── AUTO-03: Autonomous mutation proposal contracts ───────────────────────────

#[test]
fn autonomous_proposal_approves_lint_fix_plan() {
    let kernel = make_evo_kernel_for_autonomous_intake("autonomous_proposal_approves_lint_fix");
    let intake = AutonomousIntakeInput {
        source_id: "ci-prop-001".to_string(),
        candidate_source: AutonomousCandidateSource::LintRegression,
        raw_signals: vec!["error[E0308]: mismatched types".to_string()],
    };
    let candidate = &kernel.discover_autonomous_candidates(&intake).candidates[0];
    let plan = kernel.plan_autonomous_candidate(candidate);
    assert!(plan.approved, "precondition: plan must be approved");

    let proposal = kernel.propose_autonomous_mutation(&plan);
    assert!(
        proposal.proposed,
        "LintFix plan must produce an approved proposal"
    );
    assert_eq!(proposal.reason_code, AutonomousProposalReasonCode::Proposed);
    assert!(proposal.scope.is_some(), "scope must be set");
    let scope = proposal.scope.unwrap();
    assert!(
        !scope.target_paths.is_empty(),
        "target_paths must not be empty"
    );
    assert!(scope.max_files >= 1, "max_files must be at least 1");
    assert!(
        !proposal.expected_evidence.is_empty(),
        "expected_evidence must not be empty"
    );
    assert!(
        !proposal.rollback_conditions.is_empty(),
        "rollback_conditions must not be empty"
    );
    assert_eq!(proposal.approval_mode, AutonomousApprovalMode::AutoApproved);
    assert!(
        !proposal.fail_closed,
        "approved proposal must not be fail_closed"
    );
    assert!(!proposal.proposal_id.is_empty());
    assert_eq!(proposal.plan_id, plan.plan_id);
    assert_eq!(proposal.dedupe_key, plan.dedupe_key);
}

#[test]
fn autonomous_proposal_denies_unapproved_plan_fail_closed() {
    use oris_agent_contract::{deny_autonomous_task_plan, AutonomousPlanReasonCode};

    let kernel = make_evo_kernel_for_autonomous_intake("autonomous_proposal_denies_unapproved");
    let denied_plan = deny_autonomous_task_plan(
        "plan-id-denied".to_string(),
        "dedupe-denied".to_string(),
        AutonomousRiskTier::High,
        AutonomousPlanReasonCode::DeniedHighRisk,
    );
    assert!(!denied_plan.approved, "precondition: plan must be denied");

    let proposal = kernel.propose_autonomous_mutation(&denied_plan);
    assert!(
        !proposal.proposed,
        "unapproved plan must yield denied proposal"
    );
    assert_eq!(
        proposal.reason_code,
        AutonomousProposalReasonCode::DeniedPlanNotApproved
    );
    assert!(proposal.fail_closed, "denied proposal must be fail_closed");
    assert!(
        proposal.scope.is_none(),
        "denied proposal must have no scope"
    );
    assert!(
        proposal.denial_condition.is_some(),
        "denial_condition must be set"
    );
}

#[test]
fn autonomous_proposal_approves_docs_single_file_plan() {
    use oris_agent_contract::{approve_autonomous_task_plan, AutonomousRiskTier};

    let kernel = make_evo_kernel_for_autonomous_intake("autonomous_proposal_approves_docs");
    let plan = approve_autonomous_task_plan(
        "plan-docs-001".to_string(),
        "dedupe-docs-001".to_string(),
        BoundedTaskClass::DocsSingleFile,
        AutonomousRiskTier::Low,
        90u8,
        1u8,
        vec!["docs review diff".to_string()],
        Some("docs single-file plan"),
    );

    let proposal = kernel.propose_autonomous_mutation(&plan);
    assert!(proposal.proposed, "DocsSingleFile plan must be approved");
    assert_eq!(proposal.reason_code, AutonomousProposalReasonCode::Proposed);
    assert_eq!(proposal.approval_mode, AutonomousApprovalMode::AutoApproved);
    let scope = proposal.scope.expect("scope must be set");
    assert_eq!(scope.max_files, 1, "DocsSingleFile must allow max 1 file");
}

#[test]
fn autonomous_proposal_medium_risk_requires_human_review() {
    use oris_agent_contract::{approve_autonomous_task_plan, AutonomousRiskTier};

    let kernel = make_evo_kernel_for_autonomous_intake("autonomous_proposal_medium_risk_review");
    let plan = approve_autonomous_task_plan(
        "plan-dep-001".to_string(),
        "dedupe-dep-001".to_string(),
        BoundedTaskClass::CargoDepUpgrade,
        AutonomousRiskTier::Medium,
        70u8,
        3u8,
        vec!["cargo audit".to_string(), "cargo test".to_string()],
        None,
    );

    let proposal = kernel.propose_autonomous_mutation(&plan);
    assert!(
        proposal.proposed,
        "CargoDepUpgrade plan must produce a proposal"
    );
    assert_eq!(
        proposal.approval_mode,
        AutonomousApprovalMode::RequiresHumanReview,
        "medium-risk proposals must require human review"
    );
    let scope = proposal.scope.expect("scope must be set");
    assert_eq!(scope.max_files, 2, "CargoDepUpgrade must allow max 2 files");
}

#[test]
fn autonomous_proposal_reason_codes_are_stable() {
    assert_eq!(
        format!("{:?}", AutonomousProposalReasonCode::Proposed),
        "Proposed"
    );
    assert_eq!(
        format!("{:?}", AutonomousProposalReasonCode::DeniedPlanNotApproved),
        "DeniedPlanNotApproved"
    );
    assert_eq!(
        format!("{:?}", AutonomousProposalReasonCode::DeniedNoTargetScope),
        "DeniedNoTargetScope"
    );
    assert_eq!(
        format!("{:?}", AutonomousProposalReasonCode::DeniedWeakEvidence),
        "DeniedWeakEvidence"
    );
    assert_eq!(
        format!("{:?}", AutonomousProposalReasonCode::DeniedOutOfBounds),
        "DeniedOutOfBounds"
    );
    assert_eq!(
        format!("{:?}", AutonomousApprovalMode::AutoApproved),
        "AutoApproved"
    );
    assert_eq!(
        format!("{:?}", AutonomousApprovalMode::RequiresHumanReview),
        "RequiresHumanReview"
    );
}
