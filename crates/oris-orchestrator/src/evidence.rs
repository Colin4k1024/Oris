#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceBundle {
    pub run_id: String,
    pub build_ok: bool,
    pub tests_ok: bool,
    pub policy_ok: bool,
}

impl EvidenceBundle {
    pub fn new(run_id: &str, build_ok: bool, tests_ok: bool, policy_ok: bool) -> Self {
        Self {
            run_id: run_id.to_string(),
            build_ok,
            tests_ok,
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
        bundle.build_ok && bundle.tests_ok && bundle.policy_ok
    }
}
