use oris_orchestrator::evidence::{EvidenceBundle, ValidationGate};

#[test]
fn pr_ready_requires_full_green_validation() {
    let bundle = EvidenceBundle::new("run-1", false, false, false);
    assert_eq!(ValidationGate::is_pr_ready(&bundle), false);
}
