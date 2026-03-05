#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseDecision {
    Approved,
    Rejected,
}

pub struct ReleaseGate;

impl ReleaseGate {
    pub fn can_publish(decision: ReleaseDecision) -> bool {
        matches!(decision, ReleaseDecision::Approved)
    }
}
