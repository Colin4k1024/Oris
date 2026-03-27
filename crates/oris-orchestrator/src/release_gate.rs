use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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
