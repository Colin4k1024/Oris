//! Standardized proposal contracts (#404).
//!
//! Proposals are first-class objects with bounded, inspectable structure.
//! Each proposal includes intent, scope, expected effect, validation
//! requirements, and rollback conditions.

use oris_agent_contract::{AutonomousRiskTier, BoundedTaskClass};
use serde::{Deserialize, Serialize};

use crate::planning::{EvidenceType, RequiredEvidence};

// ─── Proposal contract ───────────────────────────────────────────────────────

/// A standardized proposal contract. Proposals are the unit of work that
/// flows from planning through execution to delivery.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProposalContract {
    /// Unique proposal identifier.
    pub proposal_id: String,
    /// The plan this proposal was derived from.
    pub plan_id: String,
    /// Intent section: what the proposal aims to achieve.
    pub intent: ProposalIntent,
    /// Scope section: what files/paths are affected.
    pub scope: ProposalScope,
    /// Expected effect: what should change after execution.
    pub expected_effect: ProposalEffect,
    /// Validation section: how to verify the proposal succeeded.
    pub validation: ProposalValidation,
    /// Rollback section: how to undo the proposal if it fails.
    pub rollback: ProposalRollback,
    /// Evidence requirements for this proposal type.
    pub required_evidence: Vec<RequiredEvidence>,
    /// Risk assessment.
    pub risk_tier: AutonomousRiskTier,
    /// Whether the proposal has been approved for execution.
    pub approved: bool,
}

/// What the proposal aims to achieve.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProposalIntent {
    /// The bounded task class driving this proposal.
    pub task_class: BoundedTaskClass,
    /// Human-readable description of the intent.
    pub description: String,
    /// The dedupe key linking back to the original candidate.
    pub dedupe_key: String,
}

/// What files/paths the proposal affects.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProposalScope {
    /// Target file paths.
    pub target_paths: Vec<String>,
    /// Rationale for the scope selection.
    pub scope_rationale: String,
    /// Maximum files allowed (from planning contract).
    pub max_files: usize,
    /// Maximum lines allowed (from planning contract).
    pub max_lines: usize,
}

/// What should change after execution.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProposalEffect {
    /// Description of the expected outcome.
    pub description: String,
    /// Specific assertions that should hold after execution.
    pub assertions: Vec<String>,
}

/// How to verify the proposal succeeded.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProposalValidation {
    /// Validation commands to run (e.g. `cargo test`, `cargo clippy`).
    pub commands: Vec<String>,
    /// Timeout for validation in milliseconds.
    pub timeout_ms: u64,
    /// Whether all commands must pass (vs. best-effort).
    pub all_must_pass: bool,
}

/// How to undo the proposal if it fails.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProposalRollback {
    /// Whether rollback is supported.
    pub supported: bool,
    /// Strategy for rollback.
    pub strategy: RollbackStrategy,
    /// Description of rollback procedure.
    pub description: String,
}

/// Rollback strategy.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum RollbackStrategy {
    /// Revert via git (the default for code changes).
    GitRevert,
    /// Manual rollback required.
    Manual,
    /// No rollback needed (e.g. docs-only changes).
    NotRequired,
}

// ─── Proposal builder ────────────────────────────────────────────────────────

/// Builder for constructing standardized proposals from planning contracts.
pub struct ProposalBuilder;

impl ProposalBuilder {
    /// Build a proposal from a plan and its associated metadata.
    pub fn build(
        plan_id: impl Into<String>,
        task_class: BoundedTaskClass,
        dedupe_key: impl Into<String>,
        description: impl Into<String>,
        target_paths: Vec<String>,
        scope_rationale: impl Into<String>,
        max_files: usize,
        max_lines: usize,
        risk_tier: AutonomousRiskTier,
    ) -> ProposalContract {
        let task_class_clone = task_class.clone();
        let required_evidence = default_evidence_for_class(&task_class_clone);
        let validation = default_validation_for_class(&task_class_clone);
        let rollback = default_rollback_for_class(&task_class_clone);

        ProposalContract {
            proposal_id: uuid::Uuid::new_v4().to_string(),
            plan_id: plan_id.into(),
            intent: ProposalIntent {
                task_class,
                description: description.into(),
                dedupe_key: dedupe_key.into(),
            },
            scope: ProposalScope {
                target_paths,
                scope_rationale: scope_rationale.into(),
                max_files,
                max_lines,
            },
            expected_effect: ProposalEffect {
                description: "Resolve the identified issue".into(),
                assertions: vec![],
            },
            validation,
            rollback,
            required_evidence,
            risk_tier,
            approved: false,
        }
    }
}

fn default_evidence_for_class(task_class: &BoundedTaskClass) -> Vec<RequiredEvidence> {
    match task_class {
        BoundedTaskClass::DocsSingleFile | BoundedTaskClass::DocsMultiFile => {
            vec![RequiredEvidence {
                evidence_type: EvidenceType::DiffArtifact,
                mandatory: true,
            }]
        }
        BoundedTaskClass::CargoDepUpgrade => vec![
            RequiredEvidence {
                evidence_type: EvidenceType::ValidationOutput,
                mandatory: true,
            },
            RequiredEvidence {
                evidence_type: EvidenceType::DiffArtifact,
                mandatory: true,
            },
            RequiredEvidence {
                evidence_type: EvidenceType::EnvironmentFingerprint,
                mandatory: true,
            },
        ],
        BoundedTaskClass::LintFix => vec![
            RequiredEvidence {
                evidence_type: EvidenceType::ValidationOutput,
                mandatory: true,
            },
            RequiredEvidence {
                evidence_type: EvidenceType::DiffArtifact,
                mandatory: true,
            },
        ],
    }
}

fn default_validation_for_class(task_class: &BoundedTaskClass) -> ProposalValidation {
    match task_class {
        BoundedTaskClass::DocsSingleFile | BoundedTaskClass::DocsMultiFile => ProposalValidation {
            commands: vec![],
            timeout_ms: 10_000,
            all_must_pass: true,
        },
        BoundedTaskClass::CargoDepUpgrade => ProposalValidation {
            commands: vec![
                "cargo check --all".into(),
                "cargo test --release --all-features".into(),
            ],
            timeout_ms: 300_000,
            all_must_pass: true,
        },
        BoundedTaskClass::LintFix => ProposalValidation {
            commands: vec![
                "cargo fmt --all -- --check".into(),
                "cargo clippy -- -D warnings".into(),
            ],
            timeout_ms: 120_000,
            all_must_pass: true,
        },
    }
}

fn default_rollback_for_class(task_class: &BoundedTaskClass) -> ProposalRollback {
    match task_class {
        BoundedTaskClass::DocsSingleFile | BoundedTaskClass::DocsMultiFile => ProposalRollback {
            supported: true,
            strategy: RollbackStrategy::GitRevert,
            description: "Revert documentation changes via git".into(),
        },
        BoundedTaskClass::CargoDepUpgrade => ProposalRollback {
            supported: true,
            strategy: RollbackStrategy::GitRevert,
            description: "Revert Cargo.toml and Cargo.lock changes via git".into(),
        },
        BoundedTaskClass::LintFix => ProposalRollback {
            supported: true,
            strategy: RollbackStrategy::GitRevert,
            description: "Revert lint fix changes via git".into(),
        },
    }
}

// ─── Proposal validation ─────────────────────────────────────────────────────

/// Validate that a proposal contract is complete and internally consistent.
pub fn validate_proposal(proposal: &ProposalContract) -> ProposalValidationResult {
    let mut issues = Vec::new();

    if proposal.intent.description.is_empty() {
        issues.push("Intent description is empty".into());
    }

    if proposal.scope.target_paths.is_empty() {
        issues.push("No target paths specified".into());
    }

    if proposal.scope.target_paths.len() > proposal.scope.max_files {
        issues.push(format!(
            "Target paths ({}) exceed max_files ({})",
            proposal.scope.target_paths.len(),
            proposal.scope.max_files
        ));
    }

    let mandatory_evidence: Vec<_> = proposal
        .required_evidence
        .iter()
        .filter(|e| e.mandatory)
        .collect();
    if mandatory_evidence.is_empty()
        && !matches!(
            proposal.intent.task_class,
            BoundedTaskClass::DocsSingleFile | BoundedTaskClass::DocsMultiFile
        )
    {
        issues.push("Non-docs proposal has no mandatory evidence requirements".into());
    }

    ProposalValidationResult {
        valid: issues.is_empty(),
        issues,
    }
}

/// Result of proposal validation.
#[derive(Clone, Debug)]
pub struct ProposalValidationResult {
    pub valid: bool,
    pub issues: Vec<String>,
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_proposal(task_class: BoundedTaskClass) -> ProposalContract {
        ProposalBuilder::build(
            "plan-001",
            task_class,
            "dedup-key-1",
            "Fix unused import warning",
            vec!["src/lib.rs".into()],
            "Single file affected by lint warning",
            5,
            100,
            AutonomousRiskTier::Low,
        )
    }

    #[test]
    fn builds_lint_fix_proposal() {
        let p = sample_proposal(BoundedTaskClass::LintFix);
        assert!(!p.proposal_id.is_empty());
        assert_eq!(p.plan_id, "plan-001");
        assert!(!p.approved);
        assert!(matches!(p.rollback.strategy, RollbackStrategy::GitRevert));
    }

    #[test]
    fn builds_docs_proposal_with_minimal_evidence() {
        let p = sample_proposal(BoundedTaskClass::DocsSingleFile);
        assert_eq!(p.required_evidence.len(), 1);
        assert!(matches!(
            p.required_evidence[0].evidence_type,
            EvidenceType::DiffArtifact
        ));
    }

    #[test]
    fn cargo_dep_upgrade_has_env_fingerprint_evidence() {
        let p = sample_proposal(BoundedTaskClass::CargoDepUpgrade);
        assert!(p
            .required_evidence
            .iter()
            .any(|e| matches!(e.evidence_type, EvidenceType::EnvironmentFingerprint)));
    }

    #[test]
    fn validates_complete_proposal() {
        let p = sample_proposal(BoundedTaskClass::LintFix);
        let result = validate_proposal(&p);
        assert!(result.valid);
    }

    #[test]
    fn validates_empty_intent_description() {
        let mut p = sample_proposal(BoundedTaskClass::LintFix);
        p.intent.description = String::new();
        let result = validate_proposal(&p);
        assert!(!result.valid);
        assert!(result.issues.iter().any(|i| i.contains("Intent")));
    }

    #[test]
    fn validates_empty_target_paths() {
        let mut p = sample_proposal(BoundedTaskClass::LintFix);
        p.scope.target_paths.clear();
        let result = validate_proposal(&p);
        assert!(!result.valid);
        assert!(result.issues.iter().any(|i| i.contains("target paths")));
    }

    #[test]
    fn validates_target_paths_exceed_max() {
        let mut p = sample_proposal(BoundedTaskClass::DocsSingleFile);
        p.scope.max_files = 1;
        p.scope.target_paths = vec!["a.md".into(), "b.md".into()];
        let result = validate_proposal(&p);
        assert!(!result.valid);
        assert!(result.issues.iter().any(|i| i.contains("exceed")));
    }

    #[test]
    fn lint_fix_has_validation_commands() {
        let p = sample_proposal(BoundedTaskClass::LintFix);
        assert!(!p.validation.commands.is_empty());
        assert!(p.validation.commands.iter().any(|c| c.contains("clippy")));
    }

    #[test]
    fn docs_proposal_has_no_validation_commands() {
        let p = sample_proposal(BoundedTaskClass::DocsSingleFile);
        assert!(p.validation.commands.is_empty());
    }
}
