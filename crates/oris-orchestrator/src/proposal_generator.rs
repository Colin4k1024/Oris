//! Autonomous bounded proposal generation — Issue #282 (Stream C).
//!
//! Replaces the `ProposalGeneratorPort` stub with a real, deterministic
//! implementation that:
//!
//! 1. Derives a bounded diff proposal from a task plan and signal set.
//! 2. Enforces `AutonomousProposalScope` constraints (`max_files`,
//!    `target_paths`).
//! 3. Attaches a `RollbackCondition` and an `EvidenceTemplate` to each
//!    approved proposal.
//! 4. Rejects out-of-scope proposals with
//!    `AutonomousProposalReasonCode::ScopeExceeded` — fail-closed.
//!
//! No LLM dependency is required; all generation is deterministic heuristics.

use serde::{Deserialize, Serialize};

use crate::autonomous_loop::{DiscoveredIssue, GeneratedProposal, ProposalGeneratorPort};

// ── Scope ──────────────────────────────────────────────────────────────────

/// Execution-safety scope constraints for a mutation proposal.
///
/// Applied during proposal generation; proposals that exceed these bounds are
/// rejected fail-closed with `AutonomousProposalReasonCode::ScopeExceeded`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AutonomousProposalScope {
    /// Maximum number of files the proposal is allowed to touch.
    pub max_files: usize,
    /// Allowed path prefixes (empty means any path is in scope).
    pub target_paths: Vec<String>,
}

impl AutonomousProposalScope {
    /// Create a scope that allows any path up to `max_files`.
    pub fn any(max_files: usize) -> Self {
        Self {
            max_files,
            target_paths: vec![],
        }
    }

    /// Create a scope restricted to the given path prefixes and file limit.
    pub fn restricted(max_files: usize, target_paths: Vec<String>) -> Self {
        Self {
            max_files,
            target_paths,
        }
    }

    /// Return `true` when `path` is within scope.
    ///
    /// A path is in scope if `target_paths` is empty (*any* path allowed) or
    /// if any of the target prefixes is a prefix of `path`.
    pub fn path_in_scope(&self, path: &str) -> bool {
        if self.target_paths.is_empty() {
            return true;
        }
        self.target_paths
            .iter()
            .any(|prefix| path.starts_with(prefix.as_str()))
    }

    /// Validate that the proposed file list respects scope constraints.
    ///
    /// Returns `Err(AutonomousProposalReasonCode::ScopeExceeded)` when the
    /// file count or any file path exceeds the declared bounds.
    pub fn validate_files(&self, files: &[String]) -> Result<(), AutonomousProposalReasonCode> {
        if files.len() > self.max_files {
            return Err(AutonomousProposalReasonCode::ScopeExceeded);
        }
        for file in files {
            if !self.path_in_scope(file) {
                return Err(AutonomousProposalReasonCode::ScopeExceeded);
            }
        }
        Ok(())
    }
}

// ── Reason codes ───────────────────────────────────────────────────────────

/// Outcome reason for `AutonomousMutationProposal`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum AutonomousProposalReasonCode {
    /// Proposal approved; all scope checks passed.
    Approved,
    /// Rejected because the proposal exceeds `AutonomousProposalScope` bounds.
    ScopeExceeded,
    /// Rejected because signals carry no actionable mutation hint.
    NoActionableSignal,
}

impl AutonomousProposalReasonCode {
    /// `true` when the proposal is approved.
    pub fn is_approved(&self) -> bool {
        matches!(self, Self::Approved)
    }
}

// ── Rollback ───────────────────────────────────────────────────────────────

/// Describes the rollback strategy if the mutation must be reverted.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum RollbackAction {
    /// A `git revert <commit>` rollback.
    GitRevert {
        /// Commit SHA hint (may be empty before the proposal is applied).
        commit_hint: Option<String>,
    },
    /// A `cargo yank` rollback.
    CargoYank { crate_name: String, version: String },
}

/// A structured rollback condition attached to every proposal.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RollbackCondition {
    /// Human-readable trigger description.
    pub description: String,
    /// Concrete action to take if rollback is triggered.
    pub action: RollbackAction,
    /// Whether the rollback is immediately actionable.
    pub actionable: bool,
}

impl RollbackCondition {
    /// Standard Git revert rollback (pre-apply; commit_hint pending).
    pub fn git_revert_pending() -> Self {
        Self {
            description: "Revert to HEAD if CI fails after merge".to_string(),
            action: RollbackAction::GitRevert { commit_hint: None },
            actionable: false,
        }
    }
}

// ── Evidence template ──────────────────────────────────────────────────────

/// Expected validation evidence that must pass before the proposal is
/// considered fully validated.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EvidenceTemplate {
    /// Compile check required: `cargo build` must succeed.
    pub compile_check: bool,
    /// Test patterns that must pass (forwarded to `cargo test -- <pattern>`).
    pub test_patterns: Vec<String>,
    /// Lint check required: `cargo fmt --check` and `cargo clippy` must pass.
    pub lint_check: bool,
}

impl EvidenceTemplate {
    /// Minimal evidence: compile + lint only.
    pub fn compile_and_lint() -> Self {
        Self {
            compile_check: true,
            test_patterns: vec![],
            lint_check: true,
        }
    }

    /// Full evidence: compile, lint, and the given test patterns.
    pub fn with_tests(patterns: Vec<String>) -> Self {
        Self {
            compile_check: true,
            test_patterns: patterns,
            lint_check: true,
        }
    }
}

// ── AutonomousMutationProposal ─────────────────────────────────────────────

/// A machine-readable mutation proposal contract produced by
/// `BoundedProposalGenerator`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AutonomousMutationProposal {
    /// Issue being addressed.
    pub issue_id: String,
    /// One-line description of the intended change.
    pub intent: String,
    /// Files targeted by this mutation (within scope).
    pub files: Vec<String>,
    /// Expected observable effect once the mutation is applied.
    pub expected_effect: String,
    /// Placeholder unified-diff payload (deterministic, for testing).
    pub diff_payload: String,
    /// Scope constraints this proposal was generated under.
    pub scope: AutonomousProposalScope,
    /// Rollback condition attached to this proposal.
    pub rollback: RollbackCondition,
    /// Evidence template describing required validation checks.
    pub evidence: EvidenceTemplate,
    /// Reason code for the generation outcome.
    pub reason_code: AutonomousProposalReasonCode,
}

impl AutonomousMutationProposal {
    /// Convert to the `GeneratedProposal` used by `AutonomousLoop`.
    ///
    /// Returns `None` when the proposal was not approved.
    pub fn into_generated_proposal(self) -> Option<GeneratedProposal> {
        if !self.reason_code.is_approved() {
            return None;
        }
        Some(GeneratedProposal {
            issue_id: self.issue_id,
            intent: self.intent,
            files: self.files,
            expected_effect: self.expected_effect,
            diff_payload: self.diff_payload,
        })
    }
}

// ── BoundedProposalGenerator ───────────────────────────────────────────────

/// A real `ProposalGeneratorPort` implementation backed by
/// `AutonomousProposalScope` constraints.
///
/// Generation is fully deterministic — no LLM or external I/O.  The intent
/// and target files are derived from signal keyword classification (the same
/// heuristic used by `SignalBasedProposalGenerator` but with scope gating).
pub struct BoundedProposalGenerator {
    scope: AutonomousProposalScope,
}

impl BoundedProposalGenerator {
    /// Create a generator with the given scope.
    pub fn new(scope: AutonomousProposalScope) -> Self {
        Self { scope }
    }

    /// Create a generator with an unrestricted scope of `max_files`.
    pub fn any(max_files: usize) -> Self {
        Self::new(AutonomousProposalScope::any(max_files))
    }

    /// Generate a full `AutonomousMutationProposal` (richer than
    /// `GeneratedProposal`).
    ///
    /// This is the primary entry point for code that wants access to the full
    /// proposal contract including rollback conditions and evidence templates.
    pub fn generate_proposal(&self, issue: &DiscoveredIssue) -> AutonomousMutationProposal {
        let combined: Vec<String> = {
            let mut v = issue.signals.clone();
            v.push(issue.title.clone());
            v
        };

        // Classify signals to intent + target class.
        let (intent, target_files, evidence) = match classify_signals(&combined) {
            Some(c) => c,
            None => {
                return AutonomousMutationProposal {
                    issue_id: issue.issue_id.clone(),
                    intent: String::new(),
                    files: vec![],
                    expected_effect: String::new(),
                    diff_payload: String::new(),
                    scope: self.scope.clone(),
                    rollback: RollbackCondition::git_revert_pending(),
                    evidence: EvidenceTemplate::compile_and_lint(),
                    reason_code: AutonomousProposalReasonCode::NoActionableSignal,
                };
            }
        };

        // Scope check — reject fail-closed if any proposed file is out of scope
        // or the count exceeds the limit.  Do NOT silently drop out-of-scope files;
        // that would obscure the scope violation.
        if let Err(reason_code) = self.scope.validate_files(&target_files) {
            return AutonomousMutationProposal {
                issue_id: issue.issue_id.clone(),
                intent,
                files: target_files,
                expected_effect: String::new(),
                diff_payload: String::new(),
                scope: self.scope.clone(),
                rollback: RollbackCondition::git_revert_pending(),
                evidence,
                reason_code,
            };
        }

        let expected_effect = format!("Resolves '{}' in issue '{}'", intent, issue.title);
        let diff_payload = make_diff_placeholder(&issue.issue_id, &target_files);

        AutonomousMutationProposal {
            issue_id: issue.issue_id.clone(),
            intent,
            files: target_files,
            expected_effect,
            diff_payload,
            scope: self.scope.clone(),
            rollback: RollbackCondition::git_revert_pending(),
            evidence,
            reason_code: AutonomousProposalReasonCode::Approved,
        }
    }
}

impl ProposalGeneratorPort for BoundedProposalGenerator {
    fn generate(&self, issue: &DiscoveredIssue) -> Option<GeneratedProposal> {
        self.generate_proposal(issue).into_generated_proposal()
    }
}

// ── Classification helpers ─────────────────────────────────────────────────

/// Classify a combined signal+title list into (intent, candidate_files,
/// evidence).  Returns `None` when no actionable class is detected.
fn classify_signals(combined: &[String]) -> Option<(String, Vec<String>, EvidenceTemplate)> {
    let text = combined.join(" ").to_lowercase();

    if text.contains("error[e0") || text.contains("compile") || text.contains("build") {
        Some((
            "Fix compiler error identified by signals".to_string(),
            vec!["src/lib.rs".to_string()],
            EvidenceTemplate::compile_and_lint(),
        ))
    } else if text.contains("test") || text.contains("failed") || text.contains("assertion") {
        Some((
            "Fix failing test identified by signals".to_string(),
            vec!["src/lib.rs".to_string(), "tests/integration.rs".to_string()],
            EvidenceTemplate::with_tests(vec!["test_".to_string()]),
        ))
    } else if text.contains("lint") || text.contains("clippy") || text.contains("warning") {
        Some((
            "Resolve lint/clippy warning identified by signals".to_string(),
            vec!["src/lib.rs".to_string()],
            EvidenceTemplate::compile_and_lint(),
        ))
    } else if text.contains("perf") || text.contains("slow") || text.contains("timeout") {
        Some((
            "Improve performance as identified by signals".to_string(),
            vec!["src/lib.rs".to_string()],
            EvidenceTemplate::with_tests(vec!["perf_".to_string()]),
        ))
    } else if text.contains("dep") || text.contains("cargo") || text.contains("toml") {
        Some((
            "Update dependency as identified by signals".to_string(),
            vec!["Cargo.toml".to_string()],
            EvidenceTemplate::compile_and_lint(),
        ))
    } else if text.contains("bug") || text.contains("panic") || text.contains("crash") {
        Some((
            "Fix runtime bug/panic identified by signals".to_string(),
            vec!["src/lib.rs".to_string()],
            EvidenceTemplate::with_tests(vec!["test_".to_string()]),
        ))
    } else {
        None
    }
}

/// Build a deterministic placeholder diff payload for testing.
fn make_diff_placeholder(issue_id: &str, files: &[String]) -> String {
    let file = files.first().map(|f| f.as_str()).unwrap_or("src/lib.rs");
    format!(
        "--- a/{file}\n+++ b/{file}\n@@ -1 +1 @@\n-// placeholder: {issue_id}\n+// fixed: {issue_id}\n",
        file = file,
        issue_id = issue_id,
    )
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autonomous_loop::DiscoveredIssue;

    fn issue(id: &str, signals: Vec<&str>) -> DiscoveredIssue {
        DiscoveredIssue {
            issue_id: id.to_string(),
            title: format!("Test {id}"),
            signals: signals.into_iter().map(String::from).collect(),
        }
    }

    // ── proposal_generation_approved_compile_signal ────────────────────────

    #[test]
    fn proposal_generation_approved_compile_signal() {
        let gen = BoundedProposalGenerator::any(4);
        let i = issue(
            "p1",
            vec!["error[E0425]: cannot find value", "compile error"],
        );
        let proposal = gen.generate_proposal(&i);
        assert_eq!(proposal.reason_code, AutonomousProposalReasonCode::Approved);
        assert!(!proposal.intent.is_empty());
        assert!(!proposal.files.is_empty());
        assert!(
            !proposal.diff_payload.is_empty(),
            "diff_payload must be non-empty"
        );
        assert!(!proposal.expected_effect.is_empty());
    }

    // ── proposal_generation_scope_exceeded_file_count ─────────────────────

    #[test]
    fn proposal_generation_scope_exceeded_file_count() {
        // test signals → 2 files; restrict scope to max_files=1
        let scope = AutonomousProposalScope::any(1);
        let gen = BoundedProposalGenerator::new(scope);
        let i = issue("p2", vec!["test failed", "assertion error"]);
        let proposal = gen.generate_proposal(&i);
        assert_eq!(
            proposal.reason_code,
            AutonomousProposalReasonCode::ScopeExceeded,
            "expected ScopeExceeded, got {:?}",
            proposal.reason_code
        );
        // must not produce a GeneratedProposal
        assert!(gen.generate(&i).is_none());
    }

    // ── proposal_generation_scope_exceeded_wrong_path ─────────────────────

    #[test]
    fn proposal_generation_scope_exceeded_wrong_path() {
        // Allow only "tests/" prefix, but classifier targets "src/lib.rs"
        let scope = AutonomousProposalScope::restricted(4, vec!["tests/".to_string()]);
        let gen = BoundedProposalGenerator::new(scope);
        let i = issue("p3", vec!["error[E0425]: cannot find"]);
        let proposal = gen.generate_proposal(&i);
        assert_eq!(
            proposal.reason_code,
            AutonomousProposalReasonCode::ScopeExceeded
        );
    }

    // ── proposal_generation_no_actionable_signal ──────────────────────────

    #[test]
    fn proposal_generation_no_actionable_signal() {
        let gen = BoundedProposalGenerator::any(4);
        // Title must not contain classifier keywords like "test", "error", etc.
        let i = DiscoveredIssue {
            issue_id: "p4".to_string(),
            title: "Unrelated issue".to_string(),
            signals: vec!["completely unrelated signal xyz999".to_string()],
        };
        let proposal = gen.generate_proposal(&i);
        assert_eq!(
            proposal.reason_code,
            AutonomousProposalReasonCode::NoActionableSignal
        );
        assert!(gen.generate(&i).is_none());
    }

    // ── proposal_generation_rollback_present ──────────────────────────────

    #[test]
    fn proposal_generation_rollback_present() {
        let gen = BoundedProposalGenerator::any(4);
        let i = issue("p5", vec!["test failed: assertion"]);
        let proposal = gen.generate_proposal(&i);
        // rollback condition must be attached regardless of outcome
        match proposal.rollback.action {
            RollbackAction::GitRevert { .. } => {}
            other => panic!("expected GitRevert, got {:?}", other),
        }
    }

    // ── proposal_generation_evidence_template_for_tests ───────────────────

    #[test]
    fn proposal_generation_evidence_template_for_tests() {
        let gen = BoundedProposalGenerator::any(4);
        let i = issue("p6", vec!["test failed: assertion `left == right`"]);
        let proposal = gen.generate_proposal(&i);
        assert!(proposal.evidence.compile_check);
        assert!(proposal.evidence.lint_check);
        assert!(
            !proposal.evidence.test_patterns.is_empty(),
            "test signals should include test patterns"
        );
    }

    // ── proposal_generation_into_generated_proposal ───────────────────────

    #[test]
    fn proposal_generation_into_generated_proposal() {
        let gen = BoundedProposalGenerator::any(4);
        let i = issue("p7", vec!["compile error", "error[E0425]"]);
        let proposal = gen.generate_proposal(&i);
        assert_eq!(proposal.reason_code, AutonomousProposalReasonCode::Approved);
        let gp = proposal.into_generated_proposal();
        assert!(gp.is_some());
        let gp = gp.unwrap();
        assert_eq!(gp.issue_id, "p7");
    }

    // ── proposal_generation_target_paths_in_scope ────────────────────────

    #[test]
    fn proposal_generation_target_paths_in_scope() {
        // Allow "src/" prefix — compile signals target src/lib.rs which is in scope.
        let scope = AutonomousProposalScope::restricted(4, vec!["src/".to_string()]);
        let gen = BoundedProposalGenerator::new(scope);
        let i = issue("p8", vec!["compile error", "error[E0425]"]);
        let proposal = gen.generate_proposal(&i);
        assert_eq!(
            proposal.reason_code,
            AutonomousProposalReasonCode::Approved,
            "src/ path should be in scope"
        );
    }

    // ── proposal_generation_proto_generator_port ─────────────────────────

    #[test]
    fn proposal_generation_proto_generator_port() {
        let gen = BoundedProposalGenerator::any(4);
        let i = issue("p9", vec!["panic in thread main", "crash detected"]);
        let result: Option<crate::autonomous_loop::GeneratedProposal> = gen.generate(&i);
        assert!(result.is_some());
        let gp = result.unwrap();
        assert_eq!(gp.issue_id, "p9");
        assert!(!gp.diff_payload.is_empty());
    }
}
