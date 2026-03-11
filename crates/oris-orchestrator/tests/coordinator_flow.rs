use std::sync::Arc;

use oris_orchestrator::coordinator::{Coordinator, CoordinatorConfig, ValidationSummary};
use oris_orchestrator::github_adapter::{InMemoryGitHubAdapter, RemoteIssue};
use oris_orchestrator::runtime_client::InMemoryRuntimeA2aClient;
use oris_orchestrator::task_spec::TaskSpec;

fn remote_issue(
    number: u64,
    title: &str,
    labels: &[&str],
    milestone_number: Option<u64>,
    state: &str,
) -> RemoteIssue {
    RemoteIssue {
        number,
        title: title.to_string(),
        state: state.to_string(),
        url: format!("https://github.com/Colin4k1024/Oris/issues/{}", number),
        labels: labels.iter().map(|label| (*label).to_string()).collect(),
        milestone_number,
        milestone_title: milestone_number.map(|value| format!("Sprint {}", value)),
        created_at: Some("2026-03-05T14:00:00Z".to_string()),
    }
}

#[tokio::test]
async fn flow_reaches_release_pending_approval_before_publish() {
    let coordinator = Coordinator::for_test();
    let state = coordinator.run_single_issue("issue-123").await.unwrap();
    assert_eq!(state.as_str(), "ReleasePendingApproval");
}

#[tokio::test]
async fn flow_creates_pr_with_evidence_after_a2a_session() {
    let runtime = InMemoryRuntimeA2aClient::default();
    let github = InMemoryGitHubAdapter::default();
    let coordinator = Coordinator::new(
        Arc::new(runtime.clone()),
        Arc::new(github.clone()),
        CoordinatorConfig::default(),
    );
    let spec = TaskSpec::new(
        "issue-456",
        "Add a2a self-evolution wiring",
        vec!["crates/oris-orchestrator/src".to_string()],
    )
    .unwrap();

    let outcome = coordinator.run_task(spec).await.unwrap();

    assert_eq!(outcome.state.as_str(), "ReleasePendingApproval");
    assert!(outcome
        .evidence
        .bundle_id()
        .starts_with("evidence-a2a-session-"));
    assert_eq!(outcome.pull_request.number, 1);
    assert!(outcome.pull_request.url.contains("/pr/1"));
    assert_eq!(runtime.accepted_handshakes(), 1);
    assert!(runtime.completion(&outcome.session_id).is_some());

    let payloads = github.recorded_payloads();
    assert_eq!(payloads.len(), 1);
    assert_eq!(payloads[0].issue_id, "issue-456");
    assert_eq!(payloads[0].base, "main");
    assert!(!payloads[0].evidence_bundle_id.is_empty());
}

#[tokio::test]
async fn select_next_remote_issue_returns_priority_ordered_candidate() {
    let runtime = InMemoryRuntimeA2aClient::default();
    let github = InMemoryGitHubAdapter::default();
    github.set_remote_issues(vec![
        remote_issue(119, "[EVMAP-10]", &["priority/P1"], Some(9), "OPEN"),
        remote_issue(111, "[EVMAP-02]", &["priority/P0"], Some(7), "OPEN"),
        remote_issue(110, "[EVMAP-01]", &["priority/P0"], Some(7), "OPEN"),
        remote_issue(108, "[RFC] roadmap", &["enhancement"], None, "OPEN"),
    ]);
    let coordinator = Coordinator::new(
        Arc::new(runtime),
        Arc::new(github),
        CoordinatorConfig::default(),
    );

    let selected = coordinator.select_next_remote_issue().await.unwrap();
    assert_eq!(selected.number, 110);
}

#[tokio::test]
async fn run_next_remote_issue_executes_single_selected_issue() {
    let runtime = InMemoryRuntimeA2aClient::default();
    let github = InMemoryGitHubAdapter::default();
    github.set_remote_issues(vec![
        remote_issue(115, "[EVMAP-06]", &["priority/P0"], Some(8), "OPEN"),
        remote_issue(110, "[EVMAP-01]", &["priority/P0"], Some(7), "OPEN"),
    ]);
    let coordinator = Coordinator::new(
        Arc::new(runtime),
        Arc::new(github.clone()),
        CoordinatorConfig::default(),
    );

    let outcome = coordinator.run_next_remote_issue().await.unwrap();
    assert_eq!(outcome.selected_issue.number, 110);
    assert_eq!(outcome.run_outcome.state.as_str(), "ReleasePendingApproval");

    let payloads = github.recorded_payloads();
    assert_eq!(payloads.len(), 1);
    assert_eq!(payloads[0].issue_id, "issue-110");
}

#[tokio::test]
async fn run_task_blocks_when_backend_parity_gate_is_false() {
    let runtime = InMemoryRuntimeA2aClient::default();
    let github = InMemoryGitHubAdapter::default();
    let coordinator = Coordinator::new(
        Arc::new(runtime),
        Arc::new(github),
        CoordinatorConfig::default(),
    );
    let spec = TaskSpec::new("issue-999", "Gate deny path", vec![".".to_string()]).unwrap();

    let summary = ValidationSummary {
        build_ok: true,
        contract_ok: true,
        e2e_ok: true,
        backend_parity_ok: false,
        policy_ok: true,
    };

    let err = coordinator
        .run_task_with_validation(spec, summary)
        .await
        .expect_err("expected validation gate failure");
    assert_eq!(err.kind(), "validation");
}
