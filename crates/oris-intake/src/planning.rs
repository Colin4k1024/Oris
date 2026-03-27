//! Bounded planning contracts (#403).
//!
//! Defines narrow planning contracts for admissible task classes. Contracts
//! specify what the planner is allowed to do, what evidence it must produce,
//! and what scope boundaries it must respect. Avoids open-ended plan synthesis.

use oris_agent_contract::{AutonomousRiskTier, BoundedTaskClass};
use serde::{Deserialize, Serialize};

// ─── Planning contract ───────────────────────────────────────────────────────

/// A bounded planning contract constrains what the planner may do for a given
/// task class. Contracts are narrow by design — they prevent open-ended
/// plan synthesis and ensure alignment with governance policy.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlanningContract {
    /// Unique contract identifier.
    pub contract_id: String,
    /// The bounded task class this contract applies to.
    pub task_class: BoundedTaskClass,
    /// Human-readable description of what this contract permits.
    pub description: String,
    /// Maximum number of files the plan may touch.
    pub max_files: usize,
    /// Maximum total lines changed.
    pub max_lines: usize,
    /// Maximum planning time budget in milliseconds.
    pub planning_timeout_ms: u64,
    /// Whether the plan requires human approval before execution.
    pub requires_human_approval: bool,
    /// Allowed mutation targets (file path patterns).
    pub allowed_path_patterns: Vec<String>,
    /// Required evidence types the plan must produce.
    pub required_evidence: Vec<RequiredEvidence>,
    /// Risk tier ceiling — plans exceeding this are rejected.
    pub max_risk_tier: AutonomousRiskTier,
}

/// Evidence that a planning contract requires the planner to produce.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RequiredEvidence {
    /// Evidence type identifier.
    pub evidence_type: EvidenceType,
    /// Whether this evidence is mandatory (vs. best-effort).
    pub mandatory: bool,
}

/// Types of evidence a plan can produce.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum EvidenceType {
    /// Validation output (test results, lint results).
    ValidationOutput,
    /// Before/after diff.
    DiffArtifact,
    /// Environment fingerprint.
    EnvironmentFingerprint,
    /// Confidence score update.
    ConfidenceUpdate,
    /// Policy decision linkage.
    PolicyDecisionLink,
}

// ─── Contract registry ───────────────────────────────────────────────────────

/// Registry of planning contracts keyed by task class.
pub struct PlanningContractRegistry {
    contracts: Vec<PlanningContract>,
}

impl PlanningContractRegistry {
    pub fn new(contracts: Vec<PlanningContract>) -> Self {
        Self { contracts }
    }

    /// Create a registry with built-in contracts for all bounded task classes.
    pub fn with_builtins() -> Self {
        Self::new(builtin_planning_contracts())
    }

    /// Look up the contract for a given task class.
    pub fn lookup(&self, task_class: &BoundedTaskClass) -> Option<&PlanningContract> {
        self.contracts.iter().find(|c| &c.task_class == task_class)
    }

    /// Validate that a proposed plan respects its contract bounds.
    pub fn validate_plan(
        &self,
        task_class: &BoundedTaskClass,
        proposed_files: usize,
        proposed_lines: usize,
        proposed_risk: &AutonomousRiskTier,
    ) -> PlanValidationResult {
        let contract = match self.lookup(task_class) {
            Some(c) => c,
            None => {
                return PlanValidationResult {
                    valid: false,
                    violations: vec![PlanViolation::NoContract],
                }
            }
        };

        let mut violations = Vec::new();

        if proposed_files > contract.max_files {
            violations.push(PlanViolation::ExceedsFileLimit {
                proposed: proposed_files,
                limit: contract.max_files,
            });
        }

        if proposed_lines > contract.max_lines {
            violations.push(PlanViolation::ExceedsLineLimit {
                proposed: proposed_lines,
                limit: contract.max_lines,
            });
        }

        if risk_tier_ord(proposed_risk) > risk_tier_ord(&contract.max_risk_tier) {
            violations.push(PlanViolation::ExceedsRiskTier {
                proposed: proposed_risk.clone(),
                limit: contract.max_risk_tier.clone(),
            });
        }

        PlanValidationResult {
            valid: violations.is_empty(),
            violations,
        }
    }

    /// Return all registered contracts.
    pub fn contracts(&self) -> &[PlanningContract] {
        &self.contracts
    }
}

impl Default for PlanningContractRegistry {
    fn default() -> Self {
        Self::with_builtins()
    }
}

/// Result of validating a plan against its contract.
#[derive(Clone, Debug)]
pub struct PlanValidationResult {
    pub valid: bool,
    pub violations: Vec<PlanViolation>,
}

/// A specific contract violation.
#[derive(Clone, Debug)]
pub enum PlanViolation {
    NoContract,
    ExceedsFileLimit {
        proposed: usize,
        limit: usize,
    },
    ExceedsLineLimit {
        proposed: usize,
        limit: usize,
    },
    ExceedsRiskTier {
        proposed: AutonomousRiskTier,
        limit: AutonomousRiskTier,
    },
}

// ─── Built-in contracts ──────────────────────────────────────────────────────

/// Return built-in planning contracts for all bounded task classes.
pub fn builtin_planning_contracts() -> Vec<PlanningContract> {
    vec![
        PlanningContract {
            contract_id: "plan-docs-single".into(),
            task_class: BoundedTaskClass::DocsSingleFile,
            description: "Single-file documentation update".into(),
            max_files: 1,
            max_lines: 100,
            planning_timeout_ms: 30_000,
            requires_human_approval: false,
            allowed_path_patterns: vec!["**/*.md".into(), "**/README*".into()],
            required_evidence: vec![RequiredEvidence {
                evidence_type: EvidenceType::DiffArtifact,
                mandatory: true,
            }],
            max_risk_tier: AutonomousRiskTier::Low,
        },
        PlanningContract {
            contract_id: "plan-docs-multi".into(),
            task_class: BoundedTaskClass::DocsMultiFile,
            description: "Multi-file documentation update".into(),
            max_files: 5,
            max_lines: 300,
            planning_timeout_ms: 60_000,
            requires_human_approval: false,
            allowed_path_patterns: vec!["**/*.md".into(), "docs/**".into()],
            required_evidence: vec![
                RequiredEvidence {
                    evidence_type: EvidenceType::DiffArtifact,
                    mandatory: true,
                },
                RequiredEvidence {
                    evidence_type: EvidenceType::ValidationOutput,
                    mandatory: false,
                },
            ],
            max_risk_tier: AutonomousRiskTier::Low,
        },
        PlanningContract {
            contract_id: "plan-cargo-dep".into(),
            task_class: BoundedTaskClass::CargoDepUpgrade,
            description: "Cargo dependency upgrade".into(),
            max_files: 3,
            max_lines: 50,
            planning_timeout_ms: 120_000,
            requires_human_approval: true,
            allowed_path_patterns: vec!["**/Cargo.toml".into(), "**/Cargo.lock".into()],
            required_evidence: vec![
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
            max_risk_tier: AutonomousRiskTier::Medium,
        },
        PlanningContract {
            contract_id: "plan-lint-fix".into(),
            task_class: BoundedTaskClass::LintFix,
            description: "Lint/clippy fix".into(),
            max_files: 5,
            max_lines: 100,
            planning_timeout_ms: 60_000,
            requires_human_approval: false,
            allowed_path_patterns: vec!["**/*.rs".into()],
            required_evidence: vec![
                RequiredEvidence {
                    evidence_type: EvidenceType::ValidationOutput,
                    mandatory: true,
                },
                RequiredEvidence {
                    evidence_type: EvidenceType::DiffArtifact,
                    mandatory: true,
                },
            ],
            max_risk_tier: AutonomousRiskTier::Low,
        },
    ]
}

fn risk_tier_ord(tier: &AutonomousRiskTier) -> u8 {
    match tier {
        AutonomousRiskTier::Low => 0,
        AutonomousRiskTier::Medium => 1,
        AutonomousRiskTier::High => 2,
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_contracts_cover_all_bounded_task_classes() {
        let registry = PlanningContractRegistry::with_builtins();
        let classes = [
            BoundedTaskClass::DocsSingleFile,
            BoundedTaskClass::DocsMultiFile,
            BoundedTaskClass::CargoDepUpgrade,
            BoundedTaskClass::LintFix,
        ];
        for class in &classes {
            assert!(
                registry.lookup(class).is_some(),
                "missing contract for {:?}",
                class
            );
        }
    }

    #[test]
    fn valid_plan_passes_validation() {
        let registry = PlanningContractRegistry::with_builtins();
        let result =
            registry.validate_plan(&BoundedTaskClass::LintFix, 2, 50, &AutonomousRiskTier::Low);
        assert!(result.valid);
        assert!(result.violations.is_empty());
    }

    #[test]
    fn plan_exceeding_file_limit_rejected() {
        let registry = PlanningContractRegistry::with_builtins();
        let result = registry.validate_plan(
            &BoundedTaskClass::DocsSingleFile,
            3,
            10,
            &AutonomousRiskTier::Low,
        );
        assert!(!result.valid);
        assert!(result
            .violations
            .iter()
            .any(|v| matches!(v, PlanViolation::ExceedsFileLimit { .. })));
    }

    #[test]
    fn plan_exceeding_risk_tier_rejected() {
        let registry = PlanningContractRegistry::with_builtins();
        let result =
            registry.validate_plan(&BoundedTaskClass::LintFix, 1, 10, &AutonomousRiskTier::High);
        assert!(!result.valid);
        assert!(result
            .violations
            .iter()
            .any(|v| matches!(v, PlanViolation::ExceedsRiskTier { .. })));
    }

    #[test]
    fn unknown_task_class_returns_no_contract_violation() {
        let registry = PlanningContractRegistry::new(vec![]);
        let result =
            registry.validate_plan(&BoundedTaskClass::LintFix, 1, 10, &AutonomousRiskTier::Low);
        assert!(!result.valid);
        assert!(result
            .violations
            .iter()
            .any(|v| matches!(v, PlanViolation::NoContract)));
    }

    #[test]
    fn cargo_dep_upgrade_requires_human_approval() {
        let registry = PlanningContractRegistry::with_builtins();
        let contract = registry.lookup(&BoundedTaskClass::CargoDepUpgrade).unwrap();
        assert!(contract.requires_human_approval);
    }

    #[test]
    fn lint_fix_does_not_require_human_approval() {
        let registry = PlanningContractRegistry::with_builtins();
        let contract = registry.lookup(&BoundedTaskClass::LintFix).unwrap();
        assert!(!contract.requires_human_approval);
    }
}
