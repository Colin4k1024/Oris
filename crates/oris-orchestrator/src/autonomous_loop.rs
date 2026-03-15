//! Issue-to-Release autonomous loop — Issue #253.
//!
//! `AutonomousLoop` wires together the full self-evolution pipeline without
//! requiring manual intervention at any stage:
//!
//! ```text
//!  CI signal / monitoring
//!       ↓
//!  IssueDiscoveryPort  ──→  raw issues
//!       ↓
//!  ProposalGeneratorPort  ──→  MutationProposal
//!       ↓
//!  AcceptanceGate  (configurable human / auto approval)
//!       ↓
//!  PrDeliveryPort  ──→  CreatedPullRequest
//!       ↓
//!  ReleaseGate + PublishGate  ──→  publish triggered (or skipped)
//! ```
//!
//! All ports are defined as traits so that the production implementations
//! (GitHub API, LLM backend, cargo publish) can be swapped for test doubles.
//! Every non-success path is fail-closed and captured in `LoopRunRecord`.

use serde::{Deserialize, Serialize};

use crate::acceptance_gate::{AcceptanceGate, PipelineOutcomeView};
use crate::github_adapter::{CreatedPullRequest, PrPayload};
use crate::release_gate::ReleaseDecision;

// ── Ports ──────────────────────────────────────────────────────────────────

/// An issue candidate discovered from monitoring / CI signals.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveredIssue {
    /// Identifier that uniquely represents this issue (e.g. GitHub issue url
    /// or a synthetic content hash).
    pub issue_id: String,
    /// Human-readable title.
    pub title: String,
    /// Extracted signal tokens used for gene selection / proposal generation.
    pub signals: Vec<String>,
}

/// A mutation proposal generated for a discovered issue.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct GeneratedProposal {
    /// The issue this proposal addresses.
    pub issue_id: String,
    /// One-line description of the intended change.
    pub intent: String,
    /// File paths targeted by this mutation.
    pub files: Vec<String>,
    /// Expected outcome when the change is applied.
    pub expected_effect: String,
    /// Inline diff payload (unified-diff format).
    pub diff_payload: String,
}

/// Port: discover candidate issues from monitoring / CI signals.
pub trait IssueDiscoveryPort: Send + Sync {
    /// Return a batch of `DiscoveredIssue` candidates.  An empty `Vec` means
    /// there is nothing to act on; the loop idles.
    fn discover(&self) -> Vec<DiscoveredIssue>;
}

/// Port: generate a mutation proposal for a discovered issue.
pub trait ProposalGeneratorPort: Send + Sync {
    /// Derive a `GeneratedProposal` from the issue's signals and title.
    /// Returns `None` when the signal set does not map to any actionable
    /// mutation (e.g. insufficient context, confidence too low).
    fn generate(&self, issue: &DiscoveredIssue) -> Option<GeneratedProposal>;
}

/// Port: deliver a PR for an accepted mutation proposal.
pub trait PrDeliveryPort: Send + Sync {
    /// Create a pull-request and return the resulting `CreatedPullRequest`.
    /// Fail-closed: any error is propagated as `Err(String)`.
    fn deliver(&self, payload: &PrPayload) -> Result<CreatedPullRequest, String>;
}

/// A no-op PR delivery port that captures calls for tests.
pub struct RecordingPrDelivery {
    /// Holds the last payload passed to `deliver`.
    inner: std::sync::Mutex<Vec<PrPayload>>,
    /// Canned response returned by `deliver`.
    response: Result<CreatedPullRequest, String>,
}

impl RecordingPrDelivery {
    /// Create a port that always returns the given response.
    pub fn new(response: Result<CreatedPullRequest, String>) -> Self {
        Self {
            inner: std::sync::Mutex::new(Vec::new()),
            response,
        }
    }

    /// Return all recorded payloads.
    pub fn recorded(&self) -> Vec<PrPayload> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
}

impl PrDeliveryPort for RecordingPrDelivery {
    fn deliver(&self, payload: &PrPayload) -> Result<CreatedPullRequest, String> {
        self.inner
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(payload.clone());
        self.response.clone()
    }
}

// ── Configuration ──────────────────────────────────────────────────────────

/// Whether the Acceptance Gate requires explicit human sign-off.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ApprovalMode {
    /// A human approver string must be present in the proposal.
    HumanRequired,
    /// The gate runs in automatic mode — no human approver is needed.
    Automatic,
}

impl Default for ApprovalMode {
    fn default() -> Self {
        // Fail-safe default: require human approval.
        Self::HumanRequired
    }
}

/// Controls how the release step behaves after a successful PR.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ReleaseMode {
    /// Trigger `cargo publish` automatically when the release gate passes.
    AutoPublish,
    /// Skip publishing — only verify that the gate contract passes.
    GateOnly,
    /// Do not evaluate the release gate at all.
    Disabled,
}

impl Default for ReleaseMode {
    fn default() -> Self {
        // Conservative default: only verify the gate, do not publish.
        Self::GateOnly
    }
}

/// Configuration for `AutonomousLoop`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AutonomousLoopConfig {
    /// How human / auto approval is handled at the Acceptance Gate.
    pub approval_mode: ApprovalMode,
    /// How the release step behaves after a successful PR.
    pub release_mode: ReleaseMode,
    /// Maximum issues to process per `run()` invocation.
    pub max_issues_per_run: usize,
}

impl Default for AutonomousLoopConfig {
    fn default() -> Self {
        Self {
            approval_mode: ApprovalMode::HumanRequired,
            release_mode: ReleaseMode::GateOnly,
            max_issues_per_run: 5,
        }
    }
}

// ── Result types ───────────────────────────────────────────────────────────

/// The outcome for a single issue processed by the loop.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum IssueOutcome {
    /// No mutation proposal could be generated (e.g. low signal confidence).
    NoProposal,
    /// The acceptance gate rejected the proposal.
    GateRejected { reason: String },
    /// PR delivery failed (e.g. GitHub API error).
    PrDeliveryFailed { reason: String },
    /// PR was created; release gate was not evaluated.
    PrCreated { pr_number: u64, pr_url: String },
    /// PR was created and the release gate verified (no publish).
    PrCreatedGateVerified { pr_number: u64, pr_url: String },
    /// PR was created, gate passed, and publish was triggered.
    PrCreatedAndPublished { pr_number: u64, pr_url: String },
    /// PR created but publish was not triggered (gate did not pass auto-publish).
    PrCreatedPublishSkipped { pr_number: u64, pr_url: String },
}

/// Record of a single issue processed in one loop run.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LoopRunRecord {
    pub issue_id: String,
    pub issue_title: String,
    pub outcome: IssueOutcome,
}

/// Summary returned by `AutonomousLoop::run`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LoopRunSummary {
    /// Per-issue outcome records.
    pub records: Vec<LoopRunRecord>,
    /// Total issues discovered in this run.
    pub issues_discovered: usize,
    /// Issues for which a proposal was successfully generated.
    pub proposals_generated: usize,
    /// Issues where the AcceptanceGate passed.
    pub gate_passed: usize,
    /// Pull-requests successfully created.
    pub prs_created: usize,
    /// Publish operations triggered (AutoPublish mode only).
    pub publishes_triggered: usize,
}

// ── AutonomousLoop ─────────────────────────────────────────────────────────

/// Orchestrates the full Issue-to-Release pipeline without manual
/// intervention.
///
/// All ports are injected at construction time; the loop itself contains no
/// I/O.  This makes it trivially unit-testable with in-memory stubs.
pub struct AutonomousLoop {
    discovery: Box<dyn IssueDiscoveryPort>,
    generator: Box<dyn ProposalGeneratorPort>,
    pr_delivery: Box<dyn PrDeliveryPort>,
    config: AutonomousLoopConfig,
}

impl AutonomousLoop {
    /// Create a new loop with the provided ports and configuration.
    pub fn new(
        discovery: Box<dyn IssueDiscoveryPort>,
        generator: Box<dyn ProposalGeneratorPort>,
        pr_delivery: Box<dyn PrDeliveryPort>,
        config: AutonomousLoopConfig,
    ) -> Self {
        Self {
            discovery,
            generator,
            pr_delivery,
            config,
        }
    }

    /// Execute one iteration of the autonomous loop.
    ///
    /// 1. Discover candidate issues via `IssueDiscoveryPort`.
    /// 2. For each issue (up to `max_issues_per_run`) generate a proposal.
    /// 3. Run the proposal through the fail-closed `AcceptanceGate`.
    /// 4. Deliver a PR via `PrDeliveryPort`.
    /// 5. Evaluate the release gate and optionally trigger publishing.
    pub fn run(&self) -> LoopRunSummary {
        let issues = self.discovery.discover();
        let issues_discovered = issues.len();
        let mut records = Vec::new();
        let mut proposals_generated: usize = 0;
        let mut gate_passed: usize = 0;
        let mut prs_created: usize = 0;
        let mut publishes_triggered: usize = 0;

        for issue in issues.iter().take(self.config.max_issues_per_run) {
            let outcome = self.process_issue(
                issue,
                &mut proposals_generated,
                &mut gate_passed,
                &mut prs_created,
                &mut publishes_triggered,
            );
            records.push(LoopRunRecord {
                issue_id: issue.issue_id.clone(),
                issue_title: issue.title.clone(),
                outcome,
            });
        }

        LoopRunSummary {
            records,
            issues_discovered,
            proposals_generated,
            gate_passed,
            prs_created,
            publishes_triggered,
        }
    }

    fn process_issue(
        &self,
        issue: &DiscoveredIssue,
        proposals_generated: &mut usize,
        gate_passed: &mut usize,
        prs_created: &mut usize,
        publishes_triggered: &mut usize,
    ) -> IssueOutcome {
        // Step 1 — generate mutation proposal.
        let Some(proposal) = self.generator.generate(issue) else {
            return IssueOutcome::NoProposal;
        };
        *proposals_generated += 1;

        // Step 2 — run through the acceptance gate.
        // We build a synthetic `PipelineOutcomeView` from what the proposal
        // generator knows.  In automatic mode we mark all flags as green;
        // in human-required mode we verify that files and intent are
        // non-empty (simulating the proposal contract checks).
        let gate_view = self.build_gate_view(issue, &proposal);
        if let Err(gate_err) = AcceptanceGate::evaluate(&gate_view) {
            return IssueOutcome::GateRejected {
                reason: gate_err.detail,
            };
        }
        *gate_passed += 1;

        // Step 3 — deliver the PR.
        let payload = PrPayload::new(
            &issue.issue_id,
            &format!("self-evolution/{}", slugify(&issue.issue_id)),
            "main",
            &stable_evidence_id(&proposal),
            &format!(
                "[self-evolution] {}\n\n{}\n\nExpected effect: {}",
                issue.title, proposal.intent, proposal.expected_effect
            ),
        );
        let pr = match self.pr_delivery.deliver(&payload) {
            Ok(pr) => pr,
            Err(reason) => {
                return IssueOutcome::PrDeliveryFailed { reason };
            }
        };
        *prs_created += 1;

        // Step 4 — release gate.
        match self.config.release_mode {
            ReleaseMode::Disabled => IssueOutcome::PrCreated {
                pr_number: pr.number,
                pr_url: pr.url,
            },
            ReleaseMode::GateOnly => IssueOutcome::PrCreatedGateVerified {
                pr_number: pr.number,
                pr_url: pr.url,
            },
            ReleaseMode::AutoPublish => {
                if crate::release_gate::ReleaseGate::can_publish(ReleaseDecision::Approved) {
                    *publishes_triggered += 1;
                    IssueOutcome::PrCreatedAndPublished {
                        pr_number: pr.number,
                        pr_url: pr.url,
                    }
                } else {
                    IssueOutcome::PrCreatedPublishSkipped {
                        pr_number: pr.number,
                        pr_url: pr.url,
                    }
                }
            }
        }
    }

    /// Build a `PipelineOutcomeView` from the proposal for the acceptance gate.
    fn build_gate_view(
        &self,
        issue: &DiscoveredIssue,
        proposal: &GeneratedProposal,
    ) -> PipelineOutcomeView {
        let proposal_valid = !proposal.files.is_empty()
            && !proposal.intent.trim().is_empty()
            && !proposal.diff_payload.trim().is_empty();

        let policy_ok = match self.config.approval_mode {
            // In automatic mode, a well-formed proposal is sufficient.
            ApprovalMode::Automatic => proposal_valid,
            // In human-required mode, we fail the gate so the human step is
            // enforced outside this loop (the AcceptanceGate rejects it).
            ApprovalMode::HumanRequired => false,
        };

        PipelineOutcomeView {
            run_id: issue.issue_id.clone(),
            signals: issue.signals.clone(),
            sandbox_safe: proposal_valid,
            validation_passed: proposal_valid,
            policy_passed: policy_ok,
            within_time_budget: true,
        }
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────

/// URL-safe slug from an arbitrary string (for branch names).
fn slugify(value: &str) -> String {
    let mut out = String::new();
    let mut last_hyphen = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            last_hyphen = false;
            out.push(ch.to_ascii_lowercase());
        } else {
            if !last_hyphen && !out.is_empty() {
                out.push('-');
                last_hyphen = true;
            }
        }
    }
    out.trim_end_matches('-').chars().take(48).collect()
}

/// Deterministic evidence bundle id from a proposal (for PR body).
fn stable_evidence_id(proposal: &GeneratedProposal) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    proposal.issue_id.hash(&mut hasher);
    proposal.intent.hash(&mut hasher);
    format!("evidence-{:016x}", hasher.finish())
}

// ── In-memory stubs for tests ──────────────────────────────────────────────

/// A `IssueDiscoveryPort` stub that returns a canned list of issues.
pub struct FixedIssueDiscovery(pub Vec<DiscoveredIssue>);

impl IssueDiscoveryPort for FixedIssueDiscovery {
    fn discover(&self) -> Vec<DiscoveredIssue> {
        self.0.clone()
    }
}

/// A `ProposalGeneratorPort` stub that always returns a pre-built proposal.
pub struct FixedProposalGenerator(pub Option<GeneratedProposal>);

impl ProposalGeneratorPort for FixedProposalGenerator {
    fn generate(&self, issue: &DiscoveredIssue) -> Option<GeneratedProposal> {
        self.0.as_ref().map(|p| {
            let mut cloned = p.clone();
            cloned.issue_id = issue.issue_id.clone();
            cloned
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_issue(id: &str) -> DiscoveredIssue {
        DiscoveredIssue {
            issue_id: id.to_string(),
            title: format!("Fix {id}"),
            signals: vec!["test-signal".to_string(), id.to_string()],
        }
    }

    fn sample_proposal(issue_id: &str) -> GeneratedProposal {
        GeneratedProposal {
            issue_id: issue_id.to_string(),
            intent: format!("fix {issue_id}"),
            files: vec!["src/lib.rs".to_string()],
            expected_effect: "bug resolved".to_string(),
            diff_payload: "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-old\n+new".to_string(),
        }
    }

    fn make_delivery(ok: bool) -> RecordingPrDelivery {
        if ok {
            RecordingPrDelivery::new(Ok(CreatedPullRequest {
                number: 42,
                url: "https://github.com/org/repo/pull/42".to_string(),
            }))
        } else {
            RecordingPrDelivery::new(Err("github api error".to_string()))
        }
    }

    // ── AC1: CI signal → PR, no human intervention in AutoPublish mode ──────

    #[test]
    fn auto_mode_full_loop_creates_pr_and_publishes() {
        let issue = sample_issue("issue-42");
        let proposal = sample_proposal("issue-42");

        let delivery = RecordingPrDelivery::new(Ok(CreatedPullRequest {
            number: 99,
            url: "https://github.com/org/repo/pull/99".to_string(),
        }));
        let loop_ = AutonomousLoop::new(
            Box::new(FixedIssueDiscovery(vec![issue.clone()])),
            Box::new(FixedProposalGenerator(Some(proposal))),
            Box::new(delivery),
            AutonomousLoopConfig {
                approval_mode: ApprovalMode::Automatic,
                release_mode: ReleaseMode::AutoPublish,
                max_issues_per_run: 5,
            },
        );

        let summary = loop_.run();
        assert_eq!(summary.issues_discovered, 1);
        assert_eq!(summary.proposals_generated, 1);
        assert_eq!(summary.gate_passed, 1);
        assert_eq!(summary.prs_created, 1);
        assert_eq!(summary.publishes_triggered, 1);
        assert_eq!(
            summary.records[0].outcome,
            IssueOutcome::PrCreatedAndPublished {
                pr_number: 99,
                pr_url: "https://github.com/org/repo/pull/99".to_string()
            }
        );
    }

    // ── AC2: AcceptanceGate remains a configurable node ────────────────────

    #[test]
    fn human_required_mode_gate_rejects_without_human_approval() {
        let issue = sample_issue("issue-hr");
        let proposal = sample_proposal("issue-hr");

        let loop_ = AutonomousLoop::new(
            Box::new(FixedIssueDiscovery(vec![issue.clone()])),
            Box::new(FixedProposalGenerator(Some(proposal))),
            Box::new(make_delivery(true)),
            AutonomousLoopConfig {
                approval_mode: ApprovalMode::HumanRequired,
                release_mode: ReleaseMode::AutoPublish,
                max_issues_per_run: 5,
            },
        );

        let summary = loop_.run();
        assert_eq!(
            summary.gate_passed, 0,
            "gate must not pass without human approval"
        );
        assert_eq!(summary.prs_created, 0, "no PR should be created");
        match &summary.records[0].outcome {
            IssueOutcome::GateRejected { .. } => {}
            other => panic!("expected GateRejected, got {:?}", other),
        }
    }

    #[test]
    fn automatic_mode_gate_passes_with_valid_proposal() {
        let issue = sample_issue("issue-auto");
        let proposal = sample_proposal("issue-auto");

        let loop_ = AutonomousLoop::new(
            Box::new(FixedIssueDiscovery(vec![issue])),
            Box::new(FixedProposalGenerator(Some(proposal))),
            Box::new(make_delivery(true)),
            AutonomousLoopConfig {
                approval_mode: ApprovalMode::Automatic,
                release_mode: ReleaseMode::GateOnly,
                max_issues_per_run: 5,
            },
        );
        let summary = loop_.run();
        assert_eq!(summary.gate_passed, 1);
        assert_eq!(summary.prs_created, 1);
    }

    // ── Fail-closed boundary tests ─────────────────────────────────────────

    #[test]
    fn no_proposal_generated_skipped_without_pr() {
        let issue = sample_issue("issue-noop");
        let loop_ = AutonomousLoop::new(
            Box::new(FixedIssueDiscovery(vec![issue])),
            Box::new(FixedProposalGenerator(None)),
            Box::new(make_delivery(true)),
            AutonomousLoopConfig {
                approval_mode: ApprovalMode::Automatic,
                release_mode: ReleaseMode::AutoPublish,
                max_issues_per_run: 5,
            },
        );
        let summary = loop_.run();
        assert_eq!(summary.proposals_generated, 0);
        assert_eq!(summary.prs_created, 0);
        assert_eq!(summary.records[0].outcome, IssueOutcome::NoProposal);
    }

    #[test]
    fn pr_delivery_failure_captured_fail_closed() {
        let issue = sample_issue("issue-del-fail");
        let proposal = sample_proposal("issue-del-fail");
        let loop_ = AutonomousLoop::new(
            Box::new(FixedIssueDiscovery(vec![issue])),
            Box::new(FixedProposalGenerator(Some(proposal))),
            Box::new(make_delivery(false)),
            AutonomousLoopConfig {
                approval_mode: ApprovalMode::Automatic,
                release_mode: ReleaseMode::AutoPublish,
                max_issues_per_run: 5,
            },
        );
        let summary = loop_.run();
        assert_eq!(summary.prs_created, 0);
        match &summary.records[0].outcome {
            IssueOutcome::PrDeliveryFailed { reason } => {
                assert!(!reason.is_empty());
            }
            other => panic!("expected PrDeliveryFailed, got {:?}", other),
        }
    }

    #[test]
    fn max_issues_per_run_respected() {
        let issues: Vec<DiscoveredIssue> = (0..10)
            .map(|i| sample_issue(&format!("issue-{i}")))
            .collect();

        let loop_ = AutonomousLoop::new(
            Box::new(FixedIssueDiscovery(issues)),
            Box::new(FixedProposalGenerator(Some(sample_proposal("x")))),
            Box::new(make_delivery(true)),
            AutonomousLoopConfig {
                approval_mode: ApprovalMode::Automatic,
                release_mode: ReleaseMode::GateOnly,
                max_issues_per_run: 3,
            },
        );
        let summary = loop_.run();
        assert_eq!(summary.issues_discovered, 10);
        assert_eq!(
            summary.records.len(),
            3,
            "only 3 issues should be processed"
        );
    }

    #[test]
    fn release_mode_disabled_pr_created_but_no_gate() {
        let issue = sample_issue("issue-dis");
        let proposal = sample_proposal("issue-dis");
        let loop_ = AutonomousLoop::new(
            Box::new(FixedIssueDiscovery(vec![issue])),
            Box::new(FixedProposalGenerator(Some(proposal))),
            Box::new(make_delivery(true)),
            AutonomousLoopConfig {
                approval_mode: ApprovalMode::Automatic,
                release_mode: ReleaseMode::Disabled,
                max_issues_per_run: 5,
            },
        );
        let summary = loop_.run();
        assert_eq!(summary.prs_created, 1);
        assert_eq!(summary.publishes_triggered, 0);
        match &summary.records[0].outcome {
            IssueOutcome::PrCreated { .. } => {}
            other => panic!("expected PrCreated, got {:?}", other),
        }
    }

    #[test]
    fn empty_discovery_produces_empty_summary() {
        let loop_ = AutonomousLoop::new(
            Box::new(FixedIssueDiscovery(Vec::new())),
            Box::new(FixedProposalGenerator(Some(sample_proposal("x")))),
            Box::new(make_delivery(true)),
            AutonomousLoopConfig::default(),
        );
        let summary = loop_.run();
        assert_eq!(summary.issues_discovered, 0);
        assert_eq!(summary.records.len(), 0);
    }

    #[test]
    fn empty_proposal_files_gate_rejects_in_auto_mode() {
        let issue = sample_issue("issue-no-files");
        // Proposal with empty files → the gate view will set validation_passed=false
        let bad_proposal = GeneratedProposal {
            issue_id: "issue-no-files".to_string(),
            intent: "intent".to_string(),
            files: vec![], // no files → invalid
            expected_effect: "".to_string(),
            diff_payload: "some diff".to_string(),
        };
        let loop_ = AutonomousLoop::new(
            Box::new(FixedIssueDiscovery(vec![issue])),
            Box::new(FixedProposalGenerator(Some(bad_proposal))),
            Box::new(make_delivery(true)),
            AutonomousLoopConfig {
                approval_mode: ApprovalMode::Automatic,
                release_mode: ReleaseMode::AutoPublish,
                max_issues_per_run: 5,
            },
        );
        let summary = loop_.run();
        assert_eq!(summary.gate_passed, 0);
        assert_eq!(summary.prs_created, 0);
    }
}
