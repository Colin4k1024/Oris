pub struct Coordinator;

impl Coordinator {
    pub fn for_test() -> Self {
        Self
    }

    pub async fn run_single_issue(
        &self,
        _issue_id: &str,
    ) -> Result<CoordinatorState, &'static str> {
        Ok(CoordinatorState::ReleasePendingApproval)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinatorState {
    ReleasePendingApproval,
}

impl CoordinatorState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ReleasePendingApproval => "ReleasePendingApproval",
        }
    }
}
