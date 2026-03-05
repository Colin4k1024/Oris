#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrPayload {
    pub issue_id: String,
    pub head: String,
    pub base: String,
    pub evidence_bundle_id: String,
    pub body: String,
}

impl PrPayload {
    pub fn new(
        issue_id: &str,
        head: &str,
        base: &str,
        evidence_bundle_id: &str,
        body: &str,
    ) -> Self {
        Self {
            issue_id: issue_id.to_string(),
            head: head.to_string(),
            base: base.to_string(),
            evidence_bundle_id: evidence_bundle_id.to_string(),
            body: body.to_string(),
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.evidence_bundle_id.trim().is_empty() {
            return Err("evidence_bundle_id is required");
        }
        Ok(())
    }
}
