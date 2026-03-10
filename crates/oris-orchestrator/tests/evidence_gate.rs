use oris_orchestrator::evidence::{EvidenceBundle, ValidationGate};

#[test]
fn pr_ready_requires_full_green_validation() {
    let bundle = EvidenceBundle::new("run-1", false, false, false, false, false);
    assert_eq!(ValidationGate::is_pr_ready(&bundle), false);
}

#[test]
fn pr_ready_requires_backend_parity_and_contract_e2e_green() {
    let bundle = EvidenceBundle {
        run_id: "run-2".to_string(),
        build_ok: true,
        contract_ok: true,
        e2e_ok: true,
        backend_parity_ok: false,
        policy_ok: true,
    };
    assert!(!ValidationGate::is_pr_ready(&bundle));
}
