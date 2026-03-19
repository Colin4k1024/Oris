use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceBundle {
    pub run_id: String,
    pub build_ok: bool,
    pub contract_ok: bool,
    pub e2e_ok: bool,
    pub backend_parity_ok: bool,
    pub policy_ok: bool,
}

impl EvidenceBundle {
    pub fn new(
        run_id: &str,
        build_ok: bool,
        contract_ok: bool,
        e2e_ok: bool,
        backend_parity_ok: bool,
        policy_ok: bool,
    ) -> Self {
        Self {
            run_id: run_id.to_string(),
            build_ok,
            contract_ok,
            e2e_ok,
            backend_parity_ok,
            policy_ok,
        }
    }

    pub fn bundle_id(&self) -> String {
        format!("evidence-{}", self.run_id)
    }
}

pub struct ValidationGate;

impl ValidationGate {
    pub fn is_pr_ready(bundle: &EvidenceBundle) -> bool {
        bundle.build_ok
            && bundle.contract_ok
            && bundle.e2e_ok
            && bundle.backend_parity_ok
            && bundle.policy_ok
    }
}
