//! Production adapters for `AutonomousLoop` port traits.
//!
//! These adapters bridge external sources (intake, GitHub) to the port
//! abstractions used by `AutonomousLoop`, enabling the full Issue-to-Release
//! pipeline without manual intervention.

use crate::autonomous_loop::{DiscoveredIssue, IssueDiscoveryPort};

// ─────────────────────────────────────────────────────────────────────────────
// IntakeEvent → DiscoveredIssue bridge
// ─────────────────────────────────────────────────────────────────────────────

/// Convert an `oris_intake::IntakeEvent` into a `DiscoveredIssue`.
///
/// This bridges the intake subsystem with the autonomous loop's
/// `IssueDiscoveryPort` abstraction.
impl From<oris_intake::IntakeEvent> for DiscoveredIssue {
    fn from(event: oris_intake::IntakeEvent) -> Self {
        Self {
            issue_id: event.event_id,
            title: event.title,
            signals: event.signals,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// IntakeIssueDiscovery
// ─────────────────────────────────────────────────────────────────────────────

/// An `IssueDiscoveryPort` implementation that wraps an `oris_intake::IntakeSource`
/// and converts the resulting `IntakeEvent`s into `DiscoveredIssue` candidates.
///
/// # Example
///
/// ```no_run
/// use oris_orchestrator::loop_adapters::IntakeIssueDiscovery;
/// use oris_orchestrator::autonomous_loop::IssueDiscoveryPort;
/// use oris_intake::GithubIntakeSource;
///
/// let source = GithubIntakeSource::new("workflow_run");
/// let payload = br#"{"action":"completed","workflow":"ci","conclusion":"failure"}"#;
/// let adapter = IntakeIssueDiscovery::new(Box::new(source), payload.to_vec());
/// let issues = adapter.discover();
/// ```
pub struct IntakeIssueDiscovery {
    source: Box<dyn oris_intake::IntakeSource>,
    /// Raw payload to be processed on each `discover()` call.
    payload: Vec<u8>,
}

impl IntakeIssueDiscovery {
    /// Create a new adapter from an intake source and a raw payload.
    pub fn new(source: Box<dyn oris_intake::IntakeSource>, payload: Vec<u8>) -> Self {
        Self { source, payload }
    }
}

impl IssueDiscoveryPort for IntakeIssueDiscovery {
    fn discover(&self) -> Vec<DiscoveredIssue> {
        match self.source.process(&self.payload) {
            Ok(events) => events.into_iter().map(DiscoveredIssue::from).collect(),
            Err(e) => {
                eprintln!("[IntakeIssueDiscovery] intake processing error: {e}");
                Vec::new()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oris_intake::GithubIntakeSource;

    #[test]
    fn intake_event_converts_to_discovered_issue() {
        let event = oris_intake::IntakeEvent {
            event_id: "evt-001".to_string(),
            source_type: oris_intake::IntakeSourceType::Github,
            source_event_id: Some("12345".to_string()),
            title: "CI failure on main".to_string(),
            description: "Workflow failed".to_string(),
            severity: oris_intake::IssueSeverity::High,
            signals: vec!["workflow_conclusion:failure".to_string()],
            raw_payload: None,
            timestamp_ms: 0,
        };

        let issue: DiscoveredIssue = event.into();
        assert_eq!(issue.issue_id, "evt-001");
        assert_eq!(issue.title, "CI failure on main");
        assert_eq!(issue.signals, vec!["workflow_conclusion:failure"]);
    }

    #[test]
    fn intake_issue_discovery_converts_github_events() {
        let source = GithubIntakeSource::new("workflow_run");
        let payload = serde_json::json!({
            "action": "completed",
            "workflow": "ci.yml",
            "run_id": 42,
            "repository": {
                "full_name": "org/repo",
                "html_url": "https://github.com/org/repo"
            },
            "workflow_run": {
                "head_branch": "main",
                "head_sha": "abc123",
                "html_url": "https://github.com/org/repo/actions/runs/42",
                "logs_url": "https://api.github.com/repos/org/repo/actions/runs/42/logs",
                "artifacts_url": "https://api.github.com/repos/org/repo/actions/runs/42/artifacts"
            },
            "conclusion": "failure"
        });
        let payload_bytes = serde_json::to_vec(&payload).unwrap();

        let adapter = IntakeIssueDiscovery::new(Box::new(source), payload_bytes);
        let issues = adapter.discover();

        assert_eq!(issues.len(), 1);
        assert!(issues[0].title.contains("ci.yml"));
        assert!(issues[0].signals.iter().any(|s| s.contains("failure")));
    }

    #[test]
    fn intake_issue_discovery_returns_empty_on_bad_payload() {
        let source = GithubIntakeSource::new("workflow_run");
        let adapter = IntakeIssueDiscovery::new(Box::new(source), b"not json".to_vec());
        let issues = adapter.discover();
        assert!(issues.is_empty());
    }
}
