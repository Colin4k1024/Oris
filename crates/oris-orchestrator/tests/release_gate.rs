use oris_orchestrator::release_gate::{ReleaseDecision, ReleaseGate};

#[test]
fn publish_requires_explicit_approval() {
    let denied = ReleaseGate::can_publish(ReleaseDecision::Rejected);
    let approved = ReleaseGate::can_publish(ReleaseDecision::Approved);
    assert!(!denied);
    assert!(approved);
}
