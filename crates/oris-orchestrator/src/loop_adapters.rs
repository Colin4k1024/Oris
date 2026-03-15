//! Production implementations of the `AutonomousLoop` port traits.
//!
//! These adapters bridge the synchronous `IssueDiscoveryPort`, `ProposalGeneratorPort`,
//! and `PrDeliveryPort` traits to their respective async or heuristic backends:
//!
//! * [`GitHubIssueDiscovery`] — lists open GitHub issues via `GitHubAdapter`.
//! * [`GitHubPrDelivery`] — creates pull-requests via `GitHubAdapter`.
//! * [`SignalBasedProposalGenerator`] — derives a mutation proposal from signal
//!   keyword matching (fully synchronous, no LLM dependency).
//!
//! All async calls are bridged synchronously using a dedicated thread so that
//! the `AutonomousLoop` execution model remains single-threaded and deterministic.

use std::sync::Arc;

use crate::autonomous_loop::{
    DiscoveredIssue, GeneratedProposal, IssueDiscoveryPort, PrDeliveryPort, ProposalGeneratorPort,
};
use crate::github_adapter::{CreatedPullRequest, GitHubAdapter, IssueListQuery, PrPayload};

// ─────────────────────────────────────────────────────────────────────────────
// GitHubIssueDiscovery
// ─────────────────────────────────────────────────────────────────────────────

/// Implements `IssueDiscoveryPort` by listing GitHub issues via `GitHubAdapter`.
///
/// Labels on each issue are forwarded as signal tokens so that downstream
/// `ProposalGeneratorPort` implementations can perform keyword matching.
pub struct GitHubIssueDiscovery {
    adapter: Arc<dyn GitHubAdapter>,
    query: IssueListQuery,
}

impl GitHubIssueDiscovery {
    /// Construct with a custom query.
    pub fn new(adapter: Arc<dyn GitHubAdapter>, query: IssueListQuery) -> Self {
        Self { adapter, query }
    }

    /// Convenience constructor that queries all open issues with defaults.
    pub fn open_issues(adapter: Arc<dyn GitHubAdapter>) -> Self {
        Self::new(adapter, IssueListQuery::open_only())
    }
}

impl IssueDiscoveryPort for GitHubIssueDiscovery {
    fn discover(&self) -> Vec<DiscoveredIssue> {
        let adapter = Arc::clone(&self.adapter);
        let query = self.query.clone();

        // Bridge async → sync via a dedicated thread to avoid nested-runtime panics.
        let result = match tokio::runtime::Handle::try_current() {
            Ok(handle) => std::thread::scope(|s| {
                s.spawn(|| handle.block_on(adapter.list_issues(&query)))
                    .join()
                    .unwrap_or_else(|_| {
                        Err(crate::github_adapter::GitHubAdapterError::new(
                            "discovery thread panicked",
                        ))
                    })
            }),
            Err(_) => tokio::runtime::Runtime::new()
                .map_err(|e| crate::github_adapter::GitHubAdapterError::new(e.to_string()))
                .and_then(|rt| rt.block_on(adapter.list_issues(&query))),
        };

        match result {
            Ok(issues) => issues
                .into_iter()
                .map(|ri| DiscoveredIssue {
                    issue_id: ri.url.clone(),
                    title: ri.title.clone(),
                    signals: ri.labels.clone(),
                })
                .collect(),
            Err(e) => {
                eprintln!("[GitHubIssueDiscovery] list_issues error: {e}");
                vec![]
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// GitHubPrDelivery
// ─────────────────────────────────────────────────────────────────────────────

/// Implements `PrDeliveryPort` by creating pull-requests via `GitHubAdapter`.
pub struct GitHubPrDelivery {
    adapter: Arc<dyn GitHubAdapter>,
}

impl GitHubPrDelivery {
    pub fn new(adapter: Arc<dyn GitHubAdapter>) -> Self {
        Self { adapter }
    }
}

impl PrDeliveryPort for GitHubPrDelivery {
    fn deliver(&self, payload: &PrPayload) -> Result<CreatedPullRequest, String> {
        let adapter = Arc::clone(&self.adapter);
        let payload = payload.clone();

        match tokio::runtime::Handle::try_current() {
            Ok(handle) => std::thread::scope(|s| {
                s.spawn(|| handle.block_on(adapter.create_pull_request(&payload)))
                    .join()
                    .unwrap_or_else(|_| {
                        Err(crate::github_adapter::GitHubAdapterError::new(
                            "pr delivery thread panicked",
                        ))
                    })
            }),
            Err(_) => tokio::runtime::Runtime::new()
                .map_err(|e| crate::github_adapter::GitHubAdapterError::new(e.to_string()))
                .and_then(|rt| rt.block_on(adapter.create_pull_request(&payload))),
        }
        .map_err(|e| e.to_string())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// SignalBasedProposalGenerator
// ─────────────────────────────────────────────────────────────────────────────

/// Implements `ProposalGeneratorPort` using pure signal keyword matching.
///
/// No LLM dependency — the intent and description are derived heuristically:
/// * If any signal matches a known keyword category, a targeted `intent` string
///   is produced.
/// * If no signals match anything actionable, `generate` returns `None`
///   (fail-closed).
///
/// This is intentionally simple: a more sophisticated generator could delegate
/// to an LLM backend while still implementing the same trait.
pub struct SignalBasedProposalGenerator;

impl SignalBasedProposalGenerator {
    pub fn new() -> Self {
        Self
    }

    /// Map signal tokens to an intent string.  Returns `None` when nothing
    /// actionable is found.
    fn classify(signals: &[String]) -> Option<String> {
        let combined = signals.join(" ").to_lowercase();
        if combined.contains("compile")
            || combined.contains("build")
            || combined.contains("error[e")
        {
            Some("Fix compiler error identified by signals".to_string())
        } else if combined.contains("test")
            || combined.contains("failed")
            || combined.contains("assertion")
        {
            Some("Fix failing test identified by signals".to_string())
        } else if combined.contains("lint")
            || combined.contains("clippy")
            || combined.contains("warning")
        {
            Some("Resolve lint / clippy warning identified by signals".to_string())
        } else if combined.contains("perf")
            || combined.contains("slow")
            || combined.contains("timeout")
        {
            Some("Improve performance as identified by signals".to_string())
        } else if combined.contains("dep")
            || combined.contains("cargo")
            || combined.contains("toml")
        {
            Some("Update dependency as identified by signals".to_string())
        } else if combined.contains("bug")
            || combined.contains("panic")
            || combined.contains("crash")
        {
            Some("Fix runtime bug / panic identified by signals".to_string())
        } else {
            None
        }
    }
}

impl Default for SignalBasedProposalGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl ProposalGeneratorPort for SignalBasedProposalGenerator {
    fn generate(&self, issue: &DiscoveredIssue) -> Option<GeneratedProposal> {
        // Combine signals and issue title for classification.
        let combined_signals: Vec<String> = {
            let mut v = issue.signals.clone();
            v.push(issue.title.clone());
            v
        };

        let intent = Self::classify(&combined_signals)?;

        Some(GeneratedProposal {
            issue_id: issue.issue_id.clone(),
            intent: intent.clone(),
            files: vec![],
            expected_effect: format!("Addresses '{}' (issue: {})", intent, issue.title),
            diff_payload: String::new(),
        })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// IntakeIssueDiscovery
// ─────────────────────────────────────────────────────────────────────────────

/// Convert an `oris_intake::IntakeEvent` into an `AutonomousLoop` `DiscoveredIssue`.
///
/// This is the canonical bridge between the intake pipeline (raw CI/webhook
/// signals) and the autonomous loop's issue discovery abstraction.
pub fn intake_event_to_discovered_issue(event: &oris_intake::IntakeEvent) -> DiscoveredIssue {
    DiscoveredIssue {
        issue_id: event.event_id.clone(),
        title: event.title.clone(),
        signals: event.signals.clone(),
    }
}

/// Implements `IssueDiscoveryPort` by processing raw bytes through an `IntakeSource`.
///
/// Useful when CI/webhook payloads arrive as raw bytes (e.g. from an HTTP handler)
/// and need to be decoded by an `IntakeSource` before entering the autonomous loop.
pub struct IntakeIssueDiscovery {
    source: Arc<dyn oris_intake::IntakeSource>,
    payload: Vec<u8>,
}

impl IntakeIssueDiscovery {
    /// Construct with any `IntakeSource` and the raw payload bytes to process.
    pub fn new(source: Arc<dyn oris_intake::IntakeSource>, payload: Vec<u8>) -> Self {
        Self { source, payload }
    }
}

impl IssueDiscoveryPort for IntakeIssueDiscovery {
    fn discover(&self) -> Vec<DiscoveredIssue> {
        match self.source.process(&self.payload) {
            Ok(events) => events
                .iter()
                .map(intake_event_to_discovered_issue)
                .collect(),
            Err(e) => {
                eprintln!("[IntakeIssueDiscovery] process error: {e}");
                vec![]
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autonomous_loop::DiscoveredIssue;
    use crate::github_adapter::{InMemoryGitHubAdapter, RemoteIssue};

    fn make_issue(title: &str, labels: &[&str]) -> RemoteIssue {
        RemoteIssue {
            number: 1,
            title: title.to_string(),
            state: "open".to_string(),
            url: format!("https://github.com/org/repo/issues/1"),
            labels: labels.iter().map(|s| s.to_string()).collect(),
            milestone_number: None,
            milestone_title: None,
            created_at: None,
        }
    }

    // ── SignalBasedProposalGenerator ──────────────────────────────────────────

    #[test]
    fn generator_classifies_compile_signal() {
        let gen = SignalBasedProposalGenerator::new();
        let issue = DiscoveredIssue {
            issue_id: "url".to_string(),
            title: "build broken".to_string(),
            signals: vec!["compile".to_string()],
        };
        let proposal = gen.generate(&issue).expect("should produce a proposal");
        assert!(proposal.intent.to_lowercase().contains("compiler"));
        assert_eq!(proposal.issue_id, "url");
    }

    #[test]
    fn generator_classifies_test_signal() {
        let gen = SignalBasedProposalGenerator::new();
        let issue = DiscoveredIssue {
            issue_id: "url".to_string(),
            title: "CI failing".to_string(),
            signals: vec!["test".to_string(), "failed".to_string()],
        };
        let proposal = gen.generate(&issue).expect("should produce a proposal");
        assert!(proposal.intent.to_lowercase().contains("test"));
    }

    #[test]
    fn generator_returns_none_for_empty_signals() {
        let gen = SignalBasedProposalGenerator::new();
        let issue = DiscoveredIssue {
            issue_id: "url".to_string(),
            title: "some random title".to_string(),
            signals: vec![],
        };
        // No known keywords → fail-closed → None
        assert!(gen.generate(&issue).is_none());
    }

    #[test]
    fn generator_matches_title_when_signals_empty() {
        let gen = SignalBasedProposalGenerator::new();
        let issue = DiscoveredIssue {
            issue_id: "url".to_string(),
            title: "panic in main".to_string(),
            signals: vec![],
        };
        let proposal = gen.generate(&issue).expect("title contains 'panic'");
        assert!(proposal.intent.to_lowercase().contains("bug"));
    }

    // ── GitHubIssueDiscovery ──────────────────────────────────────────────────

    #[test]
    fn discovery_maps_remote_issues_to_discovered() {
        let mem = InMemoryGitHubAdapter::default();
        mem.set_remote_issues(vec![
            make_issue("Fix compiler error", &["bug", "compile"]),
            make_issue("Dependency upgrade", &["dep"]),
        ]);

        let discovery = GitHubIssueDiscovery::open_issues(Arc::new(mem));
        // Synchronous call — runs fine without a Tokio runtime because the
        // InMemoryGitHubAdapter is synchronous internally.
        let issues = discovery.discover();
        assert_eq!(issues.len(), 2);
        assert_eq!(issues[0].title, "Fix compiler error");
        assert_eq!(issues[0].signals, vec!["bug", "compile"]);
    }

    #[test]
    fn discovery_returns_empty_on_adapter_error() {
        // An InMemoryGitHubAdapter with no issues set returns empty vec, not an error.
        let mem = InMemoryGitHubAdapter::default();
        let discovery = GitHubIssueDiscovery::open_issues(Arc::new(mem));
        let issues = discovery.discover();
        assert!(issues.is_empty());
    }

    // ── GitHubPrDelivery ──────────────────────────────────────────────────────

    #[test]
    fn pr_delivery_records_payload() {
        let mem = Arc::new(InMemoryGitHubAdapter::default());
        let delivery = GitHubPrDelivery::new(Arc::clone(&mem) as Arc<dyn GitHubAdapter>);

        let payload = PrPayload::new(
            "issue-1",
            "fix/issue-1",
            "main",
            "evidence-bundle-001",
            "Automated fix",
        );
        let result = delivery.deliver(&payload);
        assert!(
            result.is_ok(),
            "delivery should succeed: {:?}",
            result.err()
        );
        let recorded = mem.recorded_payloads();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].head, "fix/issue-1");
    }

    // ── intake_event_to_discovered_issue ──────────────────────────────────────

    #[test]
    fn converts_intake_event_to_discovered_issue() {
        use oris_intake::{IntakeEvent, IntakeSourceType};

        let event = IntakeEvent {
            event_id: "evt-abc".to_string(),
            source_type: IntakeSourceType::Github,
            source_event_id: None,
            title: "Build failed on main".to_string(),
            description: "cargo build returned exit code 1".to_string(),
            severity: oris_intake::IssueSeverity::High,
            signals: vec!["compile".to_string(), "error[E0308]".to_string()],
            raw_payload: None,
            timestamp_ms: 0,
        };

        let discovered = intake_event_to_discovered_issue(&event);
        assert_eq!(discovered.issue_id, "evt-abc");
        assert_eq!(discovered.title, "Build failed on main");
        assert_eq!(discovered.signals, vec!["compile", "error[E0308]"]);
    }
}
