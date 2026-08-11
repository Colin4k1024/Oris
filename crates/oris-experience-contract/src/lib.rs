//! Canonical, lossless contract shared by Oris and upstream agents.
//!
//! The checked-in JSON Schema in `schema/experience-bundle-v1.schema.json` is
//! the wire-level source of truth. These Rust types intentionally mirror it.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const EXPERIENCE_BUNDLE_V1: &str = "oris.experience.bundle/v1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperienceBundleV1 {
    pub schema_version: String,
    pub gene: GeneV1,
    #[serde(default)]
    pub capsules: Vec<CapsuleV1>,
    #[serde(default)]
    pub usage_receipts: Vec<UsageReceiptV1>,
}

impl ExperienceBundleV1 {
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.schema_version != EXPERIENCE_BUNDLE_V1 {
            return Err(ContractError::UnsupportedSchema(
                self.schema_version.clone(),
            ));
        }
        self.gene.validate()?;
        for capsule in &self.capsules {
            if capsule.gene_id != self.gene.id || capsule.gene_version != self.gene.version {
                return Err(ContractError::BrokenReference(capsule.id.clone()));
            }
            capsule.validate()?;
        }
        for receipt in &self.usage_receipts {
            if receipt.gene_id != self.gene.id || receipt.gene_version != self.gene.version {
                return Err(ContractError::BrokenReference(receipt.id.clone()));
            }
            receipt.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneV1 {
    pub id: String,
    pub version: u32,
    pub name: String,
    pub description: String,
    pub scope: ExperienceScope,
    pub task_category: String,
    pub applicability: Applicability,
    pub steps: Vec<ProcedureStep>,
    #[serde(default)]
    pub tool_requirements: Vec<ToolRequirement>,
    pub safety: SafetyConstraints,
    pub validation: ValidationContract,
    pub provenance: Provenance,
    pub lifecycle: LifecycleState,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub metadata: BTreeMap<String, Value>,
}

impl GeneV1 {
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.id.trim().is_empty()
            || self.name.trim().is_empty()
            || self.task_category.trim().is_empty()
        {
            return Err(ContractError::MissingRequiredField("gene identity"));
        }
        if self.version == 0 {
            return Err(ContractError::InvalidValue("version must be >= 1"));
        }
        if self.steps.is_empty() || self.validation.checks.is_empty() {
            return Err(ContractError::MissingRequiredField(
                "steps and validation.checks",
            ));
        }
        if !self.safety.suggestion_only {
            return Err(ContractError::InvalidValue(
                "v1 engineering experiences must remain suggestion_only",
            ));
        }
        if self.lifecycle == LifecycleState::Stable
            && (self.provenance.verified_successes < 3
                || self.provenance.distinct_task_contexts < 2)
        {
            return Err(ContractError::InvalidPromotionEvidence);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExperienceScope {
    Local,
    Project,
    Tenant,
    Team,
    Network,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Applicability {
    #[serde(default)]
    pub required_signals: Vec<String>,
    #[serde(default)]
    pub excluded_signals: Vec<String>,
    #[serde(default)]
    pub environments: Vec<EnvironmentConstraint>,
    #[serde(default)]
    pub project_ids: Vec<String>,
    #[serde(default)]
    pub tenant_ids: Vec<String>,
    #[serde(default)]
    pub do_not_use_when: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentConstraint {
    pub key: String,
    pub operator: ConstraintOperator,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintOperator {
    Equals,
    NotEquals,
    Contains,
    Semver,
    Exists,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcedureStep {
    pub id: String,
    pub instruction: String,
    #[serde(default)]
    pub tool: Option<String>,
    #[serde(default)]
    pub requires_approval: bool,
    #[serde(default)]
    pub expected_output: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolRequirement {
    pub name: String,
    #[serde(default)]
    pub minimum_version: Option<String>,
    #[serde(default)]
    pub required_permissions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SafetyConstraints {
    #[serde(default = "default_suggestion_only")]
    pub suggestion_only: bool,
    #[serde(default)]
    pub forbidden_operations: Vec<String>,
    #[serde(default)]
    pub required_approvals: Vec<String>,
    #[serde(default)]
    pub secret_handling: SecretHandling,
}

fn default_suggestion_only() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretHandling {
    #[default]
    Redact,
    Reject,
    AllowReferencesOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidationContract {
    pub checks: Vec<ValidationCheck>,
    #[serde(default = "default_all_checks")]
    pub success_condition: ValidationSuccessCondition,
}

fn default_all_checks() -> ValidationSuccessCondition {
    ValidationSuccessCondition::All
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidationCheck {
    pub id: String,
    pub command_or_assertion: String,
    pub evidence_kind: EvidenceKind,
    #[serde(default)]
    pub timeout_seconds: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    Test,
    Command,
    Diff,
    Artifact,
    HumanApproval,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationSuccessCondition {
    All,
    Any,
    HumanApproved,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub source_agent: String,
    pub source_run_id: String,
    #[serde(default)]
    pub trace_refs: Vec<String>,
    #[serde(default)]
    pub extractor_version: Option<String>,
    #[serde(default)]
    pub verified_successes: u64,
    #[serde(default)]
    pub verified_failures: u64,
    #[serde(default)]
    pub distinct_task_contexts: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleState {
    Candidate,
    Stable,
    Deprecated,
    Quarantined,
    Revoked,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapsuleV1 {
    pub id: String,
    pub gene_id: String,
    pub gene_version: u32,
    pub environment_fingerprint: String,
    pub task_context_hash: String,
    pub execution_evidence_hash: String,
    pub validation: ValidationResult,
    #[serde(default)]
    pub artifact_refs: Vec<String>,
    pub redaction: RedactionStatus,
    pub created_at: DateTime<Utc>,
}

impl CapsuleV1 {
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.execution_evidence_hash.trim().is_empty()
            || self.task_context_hash.trim().is_empty()
        {
            return Err(ContractError::MissingRequiredField(
                "capsule evidence hashes",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidationResult {
    pub status: OutcomeStatus,
    #[serde(default)]
    pub checks: Vec<CheckResult>,
    #[serde(default)]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckResult {
    pub check_id: String,
    pub passed: bool,
    #[serde(default)]
    pub evidence_ref: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeStatus {
    Succeeded,
    Failed,
    SafetyFailed,
    Inconclusive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RedactionStatus {
    Redacted,
    VerifiedClean,
    ContainsReferencesOnly,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsageReceiptV1 {
    pub id: String,
    pub gene_id: String,
    pub gene_version: u32,
    pub agent_id: String,
    pub run_id: String,
    pub task_context_hash: String,
    pub adoption: AdoptionStatus,
    #[serde(default)]
    pub applied_step_ids: Vec<String>,
    pub outcome: OutcomeStatus,
    #[serde(default)]
    pub failure_reason: Option<String>,
    #[serde(default)]
    pub test_evidence_refs: Vec<String>,
    #[serde(default)]
    pub cost: Option<CostMetrics>,
    pub created_at: DateTime<Utc>,
}

impl UsageReceiptV1 {
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.adoption == AdoptionStatus::Adopted
            && self.outcome == OutcomeStatus::Succeeded
            && self.test_evidence_refs.is_empty()
        {
            return Err(ContractError::MissingValidationEvidence);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdoptionStatus {
    Adopted,
    PartiallyAdopted,
    Rejected,
    NotApplicable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CostMetrics {
    #[serde(default)]
    pub input_tokens: Option<u64>,
    #[serde(default)]
    pub output_tokens: Option<u64>,
    #[serde(default)]
    pub latency_ms: Option<u64>,
    #[serde(default)]
    pub monetary_cost: Option<f64>,
    #[serde(default)]
    pub currency: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContractError {
    UnsupportedSchema(String),
    MissingRequiredField(&'static str),
    InvalidValue(&'static str),
    BrokenReference(String),
    InvalidPromotionEvidence,
    MissingValidationEvidence,
}

impl std::fmt::Display for ContractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for ContractError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_fixture_round_trips_losslessly() {
        let fixture = include_str!("../../../spec/experience/golden/experience-bundle-v1.json");
        let bundle: ExperienceBundleV1 = serde_json::from_str(fixture).unwrap();
        bundle.validate().unwrap();
        let value: Value = serde_json::from_str(fixture).unwrap();
        assert_eq!(serde_json::to_value(bundle).unwrap(), value);
    }
}
