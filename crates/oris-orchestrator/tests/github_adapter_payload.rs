use oris_orchestrator::github_adapter::PrPayload;

#[test]
fn pr_payload_requires_evidence_bundle_reference() {
    let payload = PrPayload::new("issue-123", "codex/issue-123", "main", "", "body");
    assert!(payload.validate().is_err());
}
