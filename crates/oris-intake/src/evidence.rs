//! Evidence bundle structure (#405).
//!
//! Defines the evidence bundle attached to each proposal. Bundles include
//! before/after results, validation outputs, environment and provenance context,
//! policy decision linkage, and confidence updates. Evidence completeness is
//! verified before PR preparation; incomplete bundles prevent proposal delivery.

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

// ─── Evidence bundle ─────────────────────────────────────────────────────────

/// An evidence bundle attached to a proposal or mutation result.
///
/// Bundles are assembled during the evolution pipeline and must be complete
/// before a proposal can be delivered as a PR.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EvidenceBundle {
    /// Unique bundle identifier.
    pub bundle_id: String,
    /// The proposal/capsule this bundle documents.
    pub proposal_id: String,
    /// When the bundle was assembled (Unix ms).
    pub assembled_at_ms: i64,
    /// Before/after results.
    pub before_after: Option<BeforeAfterResults>,
    /// Validation outputs.
    pub validation_outputs: Vec<ValidationOutput>,
    /// Environment and provenance context.
    pub environment: EnvironmentContext,
    /// Policy decision linkage.
    pub policy_links: Vec<PolicyDecisionLink>,
    /// Confidence score updates associated with this bundle.
    pub confidence_updates: Vec<ConfidenceUpdate>,
    /// Completeness status.
    pub completeness: EvidenceCompleteness,
}

/// Before/after results showing what changed.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BeforeAfterResults {
    /// Files changed (relative paths).
    pub files_changed: Vec<String>,
    /// Total lines added.
    pub lines_added: usize,
    /// Total lines removed.
    pub lines_removed: usize,
    /// Key change description.
    pub summary: String,
}

/// A single validation output from a command.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ValidationOutput {
    /// Command that was run.
    pub command: String,
    /// Exit code.
    pub exit_code: i32,
    /// Whether the command passed.
    pub passed: bool,
    /// stdout (truncated to max 64KB).
    pub stdout: String,
    /// stderr (truncated to max 64KB).
    pub stderr: String,
    /// Duration in milliseconds.
    pub duration_ms: u64,
}

/// Environment and provenance context.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EnvironmentContext {
    /// Rust toolchain version.
    pub rustc_version: String,
    /// cargo.lock hash.
    pub cargo_lock_hash: String,
    /// Target triple.
    pub target_triple: String,
    /// OS name.
    pub os: String,
    /// Git commit SHA of the repo at proposal time.
    pub git_sha: Option<String>,
    /// Files changed from baseline.
    pub changed_files: Vec<String>,
}

/// A linkage to a policy decision that influenced this proposal.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PolicyDecisionLink {
    /// Name of the policy rule or gate.
    pub policy_name: String,
    /// The decision that was made.
    pub decision: String,
    /// Reasoning for the decision.
    pub reasoning: String,
    /// Timestamp (Unix ms).
    pub decided_at_ms: i64,
}

/// A confidence score update recorded in the bundle.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConfidenceUpdate {
    /// The asset (gene/capsule) being updated.
    pub asset_id: String,
    /// Previous confidence score.
    pub previous_confidence: f32,
    /// New confidence score.
    pub new_confidence: f32,
    /// Reason for the update.
    pub reason: String,
    /// Whether the update was a promotion or demotion.
    pub promotion: bool,
}

/// Completeness status of the evidence bundle.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum EvidenceCompleteness {
    /// All mandatory evidence items are present.
    Complete,
    /// Some mandatory items are missing.
    Incomplete { missing_items: Vec<String> },
    /// Evidence has been validated and is ready for delivery.
    Validated,
}

// ─── Evidence bundle builder ───────────────────────────────────────────────────

/// Builder for assembling evidence bundles.
#[derive(Default)]
pub struct EvidenceBundleBuilder {
    bundle_id: String,
    proposal_id: Option<String>,
    before_after: Option<BeforeAfterResults>,
    validation_outputs: Vec<ValidationOutput>,
    environment: Option<EnvironmentContext>,
    policy_links: Vec<PolicyDecisionLink>,
    confidence_updates: Vec<ConfidenceUpdate>,
}

impl EvidenceBundleBuilder {
    /// Start a new bundle with a generated ID.
    pub fn new() -> Self {
        Self {
            bundle_id: uuid::Uuid::new_v4().to_string(),
            ..Default::default()
        }
    }

    /// Set the proposal ID.
    pub fn proposal_id(mut self, id: impl Into<String>) -> Self {
        self.proposal_id = Some(id.into());
        self
    }

    /// Add before/after results.
    pub fn with_before_after(mut self, results: BeforeAfterResults) -> Self {
        self.before_after = Some(results);
        self
    }

    /// Add a validation output.
    pub fn add_validation_output(mut self, output: ValidationOutput) -> Self {
        self.validation_outputs.push(output);
        self
    }

    /// Add multiple validation outputs.
    pub fn with_validation_outputs(mut self, outputs: Vec<ValidationOutput>) -> Self {
        self.validation_outputs.extend(outputs);
        self
    }

    /// Set the environment context.
    pub fn with_environment(mut self, env: EnvironmentContext) -> Self {
        self.environment = Some(env);
        self
    }

    /// Add a policy decision link.
    pub fn add_policy_link(mut self, link: PolicyDecisionLink) -> Self {
        self.policy_links.push(link);
        self
    }

    /// Add a confidence update.
    pub fn add_confidence_update(mut self, update: ConfidenceUpdate) -> Self {
        self.confidence_updates.push(update);
        self
    }

    /// Finalise the bundle and check completeness.
    pub fn build(mut self) -> EvidenceBundle {
        let proposal_id = self
            .proposal_id
            .take()
            .unwrap_or_else(|| "unknown-proposal".into());
        let completeness = self.check_completeness();
        EvidenceBundle {
            bundle_id: self.bundle_id,
            proposal_id,
            assembled_at_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0),
            before_after: self.before_after,
            validation_outputs: self.validation_outputs,
            environment: self
                .environment
                .unwrap_or_else(|| build_default_environment()),
            policy_links: self.policy_links,
            confidence_updates: self.confidence_updates,
            completeness,
        }
    }

    fn check_completeness(&self) -> EvidenceCompleteness {
        let mut missing = Vec::new();

        if self.environment.is_none() {
            missing.push("environment".into());
        }

        if self.validation_outputs.is_empty() {
            missing.push("validation_outputs".into());
        }

        if missing.is_empty() {
            EvidenceCompleteness::Complete
        } else {
            EvidenceCompleteness::Incomplete {
                missing_items: missing,
            }
        }
    }
}

/// Check whether a bundle is complete enough to proceed to PR delivery.
pub fn is_bundle_deliverable(bundle: &EvidenceBundle) -> bool {
    matches!(
        bundle.completeness,
        EvidenceCompleteness::Complete | EvidenceCompleteness::Validated
    )
}

/// Validate that all mandatory evidence items are present.
pub fn validate_bundle(bundle: &EvidenceBundle) -> BundleValidationResult {
    let mut issues = Vec::new();

    if bundle.environment.rustc_version.is_empty() {
        issues.push("Missing rustc_version in environment".into());
    }

    let validation_passed = bundle.validation_outputs.iter().any(|o| o.passed);

    if bundle.validation_outputs.is_empty() {
        issues.push("No validation outputs recorded".into());
    } else if !validation_passed {
        issues.push("No validation command passed".into());
    }

    if matches!(bundle.completeness, EvidenceCompleteness::Incomplete { .. }) {
        issues.push("Bundle is incomplete".into());
    }

    if issues.is_empty() {
        BundleValidationResult {
            valid: true,
            issues: vec![],
        }
    } else {
        BundleValidationResult {
            valid: false,
            issues,
        }
    }
}

/// Result of bundle validation.
#[derive(Clone, Debug)]
pub struct BundleValidationResult {
    pub valid: bool,
    pub issues: Vec<String>,
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn build_default_environment() -> EnvironmentContext {
    EnvironmentContext {
        rustc_version: std::process::Command::new("rustc")
            .arg("--version")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        cargo_lock_hash: "unknown".to_string(),
        target_triple: std::env::consts::ARCH.to_string(),
        os: std::env::consts::OS.to_string(),
        git_sha: None,
        changed_files: vec![],
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_bundle() -> EvidenceBundle {
        EvidenceBundleBuilder::new()
            .proposal_id("proposal-1")
            .with_environment(EnvironmentContext {
                rustc_version: "rustc 1.80.0".into(),
                cargo_lock_hash: "abc123".into(),
                target_triple: "x86_64-apple-darwin".into(),
                os: "macos".into(),
                git_sha: Some("def456".into()),
                changed_files: vec!["src/lib.rs".into()],
            })
            .add_validation_output(ValidationOutput {
                command: "cargo clippy".into(),
                exit_code: 0,
                passed: true,
                stdout: "clippy passed".into(),
                stderr: String::new(),
                duration_ms: 5000,
            })
            .build()
    }

    #[test]
    fn builds_complete_bundle() {
        let bundle = valid_bundle();
        assert_eq!(bundle.bundle_id.len(), 36); // UUID length
        assert_eq!(bundle.proposal_id, "proposal-1");
        assert!(matches!(
            bundle.completeness,
            EvidenceCompleteness::Complete
        ));
    }

    #[test]
    fn missing_environment_marks_bundle_incomplete() {
        let bundle = EvidenceBundleBuilder::new()
            .proposal_id("proposal-2")
            .build();
        assert!(matches!(
            bundle.completeness,
            EvidenceCompleteness::Incomplete { .. }
        ));
    }

    #[test]
    fn deliverable_bundle_passes_validation() {
        let bundle = valid_bundle();
        let result = validate_bundle(&bundle);
        assert!(result.valid);
    }

    #[test]
    fn bundle_without_validation_fails_validation() {
        let bundle = EvidenceBundleBuilder::new()
            .proposal_id("proposal-3")
            .with_environment(EnvironmentContext {
                rustc_version: "1.80.0".into(),
                cargo_lock_hash: "abc".into(),
                target_triple: "x86_64".into(),
                os: "linux".into(),
                git_sha: None,
                changed_files: vec![],
            })
            .build();
        let result = validate_bundle(&bundle);
        assert!(!result.valid);
        assert!(result.issues.iter().any(|i| i.contains("No validation")));
    }

    #[test]
    fn incomplete_bundle_not_deliverable() {
        let bundle = EvidenceBundleBuilder::new()
            .proposal_id("proposal-4")
            .build();
        assert!(!is_bundle_deliverable(&bundle));
    }

    #[test]
    fn validated_bundle_is_deliverable() {
        let mut bundle = valid_bundle();
        bundle.completeness = EvidenceCompleteness::Validated;
        assert!(is_bundle_deliverable(&bundle));
    }

    #[test]
    fn builder_adds_multiple_validation_outputs() {
        let bundle = EvidenceBundleBuilder::new()
            .proposal_id("proposal-5")
            .with_environment(EnvironmentContext {
                rustc_version: "1.80".into(),
                cargo_lock_hash: "h".into(),
                target_triple: "x".into(),
                os: "x".into(),
                git_sha: None,
                changed_files: vec![],
            })
            .add_validation_output(ValidationOutput {
                command: "cargo fmt".into(),
                exit_code: 0,
                passed: true,
                stdout: "fmt ok".into(),
                stderr: String::new(),
                duration_ms: 100,
            })
            .add_validation_output(ValidationOutput {
                command: "cargo clippy".into(),
                exit_code: 0,
                passed: true,
                stdout: "clippy ok".into(),
                stderr: String::new(),
                duration_ms: 5000,
            })
            .build();
        assert_eq!(bundle.validation_outputs.len(), 2);
    }

    #[test]
    fn confidence_update_tracks_promotion() {
        let bundle = EvidenceBundleBuilder::new()
            .proposal_id("proposal-6")
            .with_environment(EnvironmentContext {
                rustc_version: "1.80".into(),
                cargo_lock_hash: "h".into(),
                target_triple: "x".into(),
                os: "x".into(),
                git_sha: None,
                changed_files: vec![],
            })
            .add_validation_output(ValidationOutput {
                command: "test".into(),
                exit_code: 0,
                passed: true,
                stdout: String::new(),
                stderr: String::new(),
                duration_ms: 100,
            })
            .add_confidence_update(ConfidenceUpdate {
                asset_id: "gene-1".into(),
                previous_confidence: 0.6,
                new_confidence: 0.85,
                reason: "Successful validation".into(),
                promotion: true,
            })
            .build();
        assert_eq!(bundle.confidence_updates.len(), 1);
        assert!(bundle.confidence_updates[0].promotion);
    }

    #[test]
    fn policy_decision_links_recorded() {
        let bundle = EvidenceBundleBuilder::new()
            .proposal_id("proposal-7")
            .with_environment(EnvironmentContext {
                rustc_version: "1.80".into(),
                cargo_lock_hash: "h".into(),
                target_triple: "x".into(),
                os: "x".into(),
                git_sha: None,
                changed_files: vec![],
            })
            .add_validation_output(ValidationOutput {
                command: "test".into(),
                exit_code: 0,
                passed: true,
                stdout: String::new(),
                stderr: String::new(),
                duration_ms: 100,
            })
            .add_policy_link(PolicyDecisionLink {
                policy_name: "GovernedEvolutionPolicy".into(),
                decision: "AutoApprove".into(),
                reasoning: "Risk tier Low, feasibility 0.8".into(),
                decided_at_ms: 1_700_000_000_000,
            })
            .build();
        assert_eq!(bundle.policy_links.len(), 1);
        assert_eq!(
            bundle.policy_links[0].policy_name,
            "GovernedEvolutionPolicy"
        );
    }
}
