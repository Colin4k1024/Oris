use oris_orchestrator::runtime_client::A2aSessionRequest;

#[test]
fn start_session_rejects_invalid_protocol_version() {
    let req = A2aSessionRequest::start("sender-a", "0.0.1", "task-1", "summary");
    assert!(req.validate().is_err());
}
