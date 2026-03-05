use oris_orchestrator::coordinator::Coordinator;

#[tokio::test]
async fn flow_reaches_release_pending_approval_before_publish() {
    let coordinator = Coordinator::for_test();
    let state = coordinator.run_single_issue("issue-123").await.unwrap();
    assert_eq!(state.as_str(), "ReleasePendingApproval");
}
