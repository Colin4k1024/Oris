use std::sync::Arc;

use oris_orchestrator::coordinator::{Coordinator, CoordinatorConfig};
use oris_orchestrator::github_adapter::InMemoryGitHubAdapter;
use oris_orchestrator::runtime_client::InMemoryRuntimeA2aClient;
use oris_orchestrator::task_spec::TaskSpec;

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
