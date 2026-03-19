//! Fail-Closed Autonomous Merge and Release Gate — Issue #327.
//!
//! Implements three sequential gate contracts that must all pass before any
//! merge or publish action is taken in the narrow approved task-class lane:
//!
//! ```text
//! ReleaseEvidenceSnapshot  (all stages: intake → planning → proposal →
//!                           execution → confidence → PR)
//!       ↓
//! MergeGate            — kill switch + class eligibility + risk tier + evidence
//!       ↓
//! ExtendedReleaseGate  — kill switch + merge approval + post-gate drift check
//!       ↓
//! GatedPublishGate     — kill switch + release approval + rollback plan
//! ```
//!
//! ## Fail-closed principle
//!
//! All three gates are fail-closed. An unresolved kill switch, missing
//! evidence, ineligible task class, elevated risk tier, post-gate drift, or
//! absent rollback plan aborts with a structured [`ReleaseReasonCode`].  No
//! gate ever silently succeeds: the `fail_closed` field is always `true` in
//! every returned result.
//!
//! ## Machine-readable outputs
//!
//! Every result type carries the fields required by issue #327:
//! `merge_gate_result`, `release_gate_result`, `publish_gate_result`,
//! `kill_switch_state`, `rollback_plan`, `reason_code`, `fail_closed`.

use serde::{Deserialize, Serialize};

use crate::evidence::EvidenceBundle;
use crate::task_planner::RiskTier;

// ── Kill Switch ──────────────────────────────────────────────────────────

/// Current state of the autonomous-release kill switch.
///
/// Only `Inactive` permits automation to proceed.  `Active` and
/// `IncidentTripped` are both blocking and are treated identically by the
/// gates (fail-closed with [`ReleaseReasonCode::KillSwitchActive`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KillSwitchState {
    /// Kill switch is not active; automation may proceed.
    Inactive,
    /// Kill switch manually activated; all autonomous actions halted.
    Active,
    /// An incident stop condition tripped the switch automatically.
    IncidentTripped {
        /// Identifier of the triggering incident (e.g. "INC-42").
        incident_id: String,
    },
}

impl KillSwitchState {
    /// Returns `true` when the kill switch is in any blocking state.
    pub fn is_blocking(&self) -> bool {
        !matches!(self, KillSwitchState::Inactive)
    }
}

// ── Reason codes ────────────────────────────────────────────────────────

/// Structured reason code returned by the autonomous release gates.
///
/// The `Approved` variant is the only non-blocking code; every other variant
/// indicates a denial condition.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseReasonCode {
    /// Kill switch is active; no merge or publish is permitted.
    KillSwitchActive,
    /// The task class is not in the narrow approved autonomous-merge set.
    IneligibleClass,
    /// Risk tier exceeds the ceiling permitted for autonomous merge.
    RiskTierExceeded,
    /// Evidence from a required pipeline stage is missing or incomplete.
    IncompleteEvidence,
    /// Post-gate drift detected: state diverged after the merge gate passed.
    PostGateDrift,
    /// No validated rollback plan is present (required before publish).
    MissingRollbackPlan,
    /// All gate conditions satisfied; action is approved.
    Approved,
}

// ── Rollback Plan ────────────────────────────────────────────────────────

/// A validated rollback plan required before any autonomous publish action.
///
/// Callers must populate and validate this plan before passing it to
/// [`GatedPublishGate`].  An unvalidated or empty plan is treated as absent
/// and the publish gate will reject it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RollbackPlan {
    /// Git ref, tag, or commit SHA that can be restored if the release fails.
    pub restore_ref: String,
    /// Ordered rollback steps to execute (e.g. `git revert`, `cargo yank`).
    pub steps: Vec<String>,
    /// Whether the rollback has been validated as executable in this context.
    pub validated: bool,
}

impl RollbackPlan {
    /// Construct a rollback plan.
    pub fn new(restore_ref: impl Into<String>, steps: Vec<String>, validated: bool) -> Self {
        Self {
            restore_ref: restore_ref.into(),
            steps,
            validated,
        }
    }

    /// Returns `true` when the plan is complete and validated.
    ///
    /// A plan is ready only if it has a non-empty restore ref, at least one
    /// rollback step, and `validated` is set to `true`.
    pub fn is_ready(&self) -> bool {
        !self.restore_ref.is_empty() && !self.steps.is_empty() && self.validated
    }
}

// ── Evidence snapshot ────────────────────────────────────────────────────

/// Complete evidence set required by the autonomous release gate.
///
/// All seven stage flags plus the underlying [`EvidenceBundle`] must be
/// `true` before the merge gate will approve.  A single `false` field causes
/// [`MergeGate::evaluate`] to return
/// [`ReleaseReasonCode::IncompleteEvidence`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReleaseEvidenceSnapshot {
    /// Intake stage: the issue was accepted and deduplicated.
    pub intake_ok: bool,
    /// Planning stage: task class is approved and risk is within bounds.
    pub planning_ok: bool,
    /// Proposal stage: generated proposal is non-empty and parseable.
    pub proposal_ok: bool,
    /// Execution stage: sandbox ran the mutation without errors.
    pub execution_ok: bool,
    /// Confidence stage: confidence score meets the required threshold.
    pub confidence_ok: bool,
    /// PR stage: pull-request was created and returned a valid number.
    pub pr_ok: bool,
    /// Underlying build / contract / e2e / backend evidence.
    pub evidence_bundle: EvidenceBundle,
}

impl ReleaseEvidenceSnapshot {
    /// Returns `true` when every evidence stage is complete.
    pub fn all_stages_complete(&self) -> bool {
        self.intake_ok
            && self.planning_ok
            && self.proposal_ok
            && self.execution_ok
            && self.confidence_ok
            && self.pr_ok
            && self.evidence_bundle.build_ok
            && self.evidence_bundle.contract_ok
            && self.evidence_bundle.e2e_ok
            && self.evidence_bundle.backend_parity_ok
            && self.evidence_bundle.policy_ok
    }
}

// ── Approved class registry ──────────────────────────────────────────────

/// Task-class IDs approved for autonomous merge in the narrow safe lane.
///
/// Only the three lowest-risk, best-understood mutation classes are included.
/// Any class absent from this list is denied by [`MergeGate`] with
/// [`ReleaseReasonCode::IneligibleClass`].
pub fn approved_autonomous_merge_classes() -> &'static [&'static str] {
    &["missing-import", "type-mismatch", "test-failure"]
}

fn class_is_eligible(task_class_id: &str) -> bool {
    approved_autonomous_merge_classes().contains(&task_class_id)
}

fn risk_within_bounds(risk: &RiskTier) -> bool {
    matches!(risk, RiskTier::Low | RiskTier::Medium)
}

// ── Merge Gate ──────────────────────────────────────────────────────────

/// Input contract for the merge gate.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MergeGateRequest {
    /// Unique run identifier.
    pub run_id: String,
    /// Task class that produced the mutation proposal.
    pub task_class_id: String,
    /// Risk tier assigned to this candidate during task planning.
    pub task_risk_tier: RiskTier,
    /// Current kill switch state at evaluation time.
    pub kill_switch: KillSwitchState,
    /// Complete evidence snapshot from all pipeline stages.
    pub evidence: ReleaseEvidenceSnapshot,
}

/// Result returned by [`MergeGate::evaluate`].
///
/// `fail_closed` is always `true`; it signals that the gate contract itself
/// is fail-closed regardless of the `approved` outcome.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MergeGateResult {
    /// Run identifier echoed from the request.
    pub run_id: String,
    /// `true` only when all merge conditions are satisfied.
    pub approved: bool,
    /// Machine-readable denial or approval reason.
    pub reason_code: ReleaseReasonCode,
    /// Kill switch state at evaluation time.
    pub kill_switch_state: KillSwitchState,
    /// Always `true`; documents the fail-closed contract.
    pub fail_closed: bool,
}

/// Fail-closed merge gate.
///
/// Evaluation order: kill switch → class eligibility → risk tier → evidence.
/// The first failing condition wins; remaining checks are skipped.
pub struct MergeGate;

impl MergeGate {
    /// Evaluate merge eligibility.
    ///
    /// Returns a [`MergeGateResult`] with `approved: true` only if all four
    /// conditions pass.  Every denial path sets `approved: false` and a
    /// structured `reason_code`.
    pub fn evaluate(req: MergeGateRequest) -> MergeGateResult {
        // 1. Kill switch — unconditional halt before any other check.
        if req.kill_switch.is_blocking() {
            return MergeGateResult {
                run_id: req.run_id,
                approved: false,
                reason_code: ReleaseReasonCode::KillSwitchActive,
                kill_switch_state: req.kill_switch,
                fail_closed: true,
            };
        }

        // 2. Class eligibility — only narrowly approved classes may merge.
        if !class_is_eligible(&req.task_class_id) {
            return MergeGateResult {
                run_id: req.run_id,
                approved: false,
                reason_code: ReleaseReasonCode::IneligibleClass,
                kill_switch_state: req.kill_switch,
                fail_closed: true,
            };
        }

        // 3. Risk tier boundary — High and Critical are denied.
        if !risk_within_bounds(&req.task_risk_tier) {
            return MergeGateResult {
                run_id: req.run_id,
                approved: false,
                reason_code: ReleaseReasonCode::RiskTierExceeded,
                kill_switch_state: req.kill_switch,
                fail_closed: true,
            };
        }

        // 4. Complete evidence — all pipeline stages must be present.
        if !req.evidence.all_stages_complete() {
            return MergeGateResult {
                run_id: req.run_id,
                approved: false,
                reason_code: ReleaseReasonCode::IncompleteEvidence,
                kill_switch_state: req.kill_switch,
                fail_closed: true,
            };
        }

        MergeGateResult {
            run_id: req.run_id,
            approved: true,
            reason_code: ReleaseReasonCode::Approved,
            kill_switch_state: req.kill_switch,
            fail_closed: true,
        }
    }
}

// ── Release Gate ─────────────────────────────────────────────────────────

/// Input contract for the release gate (post-merge).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReleaseGateRequest {
    /// Unique run identifier.
    pub run_id: String,
    /// Result from the preceding merge gate evaluation.
    pub merge_gate_result: MergeGateResult,
    /// Kill switch state re-evaluated at release time.
    pub kill_switch: KillSwitchState,
    /// `true` when the post-merge repository state matches the evidence
    /// snapshot used for the merge gate (no drift).
    pub no_post_gate_drift: bool,
}

/// Result returned by [`ExtendedReleaseGate::evaluate`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExtendedReleaseGateResult {
    /// Run identifier echoed from the request.
    pub run_id: String,
    /// `true` only when all release conditions are satisfied.
    pub approved: bool,
    /// Machine-readable denial or approval reason.
    pub reason_code: ReleaseReasonCode,
    /// Kill switch state at evaluation time.
    pub kill_switch_state: KillSwitchState,
    /// Always `true`; documents the fail-closed contract.
    pub fail_closed: bool,
}

/// Fail-closed release gate (post-merge, pre-publish).
///
/// Evaluation order: kill switch → merge gate approval → post-gate drift.
pub struct ExtendedReleaseGate;

impl ExtendedReleaseGate {
    /// Evaluate release eligibility after merge.
    pub fn evaluate(req: ReleaseGateRequest) -> ExtendedReleaseGateResult {
        // 1. Kill switch — re-check at release time.
        if req.kill_switch.is_blocking() {
            return ExtendedReleaseGateResult {
                run_id: req.run_id,
                approved: false,
                reason_code: ReleaseReasonCode::KillSwitchActive,
                kill_switch_state: req.kill_switch,
                fail_closed: true,
            };
        }

        // 2. Merge gate must have passed.
        if !req.merge_gate_result.approved {
            return ExtendedReleaseGateResult {
                run_id: req.run_id,
                approved: false,
                reason_code: req.merge_gate_result.reason_code,
                kill_switch_state: req.kill_switch,
                fail_closed: true,
            };
        }

        // 3. Post-gate drift — abort if state diverged after merge gate passed.
        if !req.no_post_gate_drift {
            return ExtendedReleaseGateResult {
                run_id: req.run_id,
                approved: false,
                reason_code: ReleaseReasonCode::PostGateDrift,
                kill_switch_state: req.kill_switch,
                fail_closed: true,
            };
        }

        ExtendedReleaseGateResult {
            run_id: req.run_id,
            approved: true,
            reason_code: ReleaseReasonCode::Approved,
            kill_switch_state: req.kill_switch,
            fail_closed: true,
        }
    }
}

// ── Publish Gate ─────────────────────────────────────────────────────────

/// Input contract for the publish gate.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PublishGateRequest {
    /// Unique run identifier.
    pub run_id: String,
    /// Result from the preceding release gate evaluation.
    pub release_gate_result: ExtendedReleaseGateResult,
    /// Kill switch state re-evaluated at publish time.
    pub kill_switch: KillSwitchState,
    /// Validated rollback plan; `None` or an unvalidated plan causes denial.
    pub rollback_plan: Option<RollbackPlan>,
}

/// Result returned by [`GatedPublishGate::evaluate`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PublishGateResult {
    /// Run identifier echoed from the request.
    pub run_id: String,
    /// `true` only when all publish conditions are satisfied.
    pub approved: bool,
    /// Machine-readable denial or approval reason.
    pub reason_code: ReleaseReasonCode,
    /// Kill switch state at evaluation time.
    pub kill_switch_state: KillSwitchState,
    /// Rollback plan echoed from the request (present when approved).
    pub rollback_plan: Option<RollbackPlan>,
    /// Always `true`; documents the fail-closed contract.
    pub fail_closed: bool,
}

/// Fail-closed publish gate.
///
/// Evaluation order: kill switch → release gate approval → rollback plan.
/// A validated [`RollbackPlan`] is mandatory before any publish is approved.
pub struct GatedPublishGate;

impl GatedPublishGate {
    /// Evaluate publish eligibility.
    pub fn evaluate(req: PublishGateRequest) -> PublishGateResult {
        // 1. Kill switch — unconditional halt.
        if req.kill_switch.is_blocking() {
            return PublishGateResult {
                run_id: req.run_id,
                approved: false,
                reason_code: ReleaseReasonCode::KillSwitchActive,
                kill_switch_state: req.kill_switch,
                rollback_plan: req.rollback_plan,
                fail_closed: true,
            };
        }

        // 2. Release gate must have passed.
        if !req.release_gate_result.approved {
            return PublishGateResult {
                run_id: req.run_id,
                approved: false,
                reason_code: req.release_gate_result.reason_code,
                kill_switch_state: req.kill_switch,
                rollback_plan: req.rollback_plan,
                fail_closed: true,
            };
        }

        // 3. Rollback plan — mandatory and must be validated before publish.
        let plan = match req.rollback_plan {
            None => {
                return PublishGateResult {
                    run_id: req.run_id,
                    approved: false,
                    reason_code: ReleaseReasonCode::MissingRollbackPlan,
                    kill_switch_state: req.kill_switch,
                    rollback_plan: None,
                    fail_closed: true,
                };
            }
            Some(ref p) if !p.is_ready() => {
                return PublishGateResult {
                    run_id: req.run_id,
                    approved: false,
                    reason_code: ReleaseReasonCode::MissingRollbackPlan,
                    kill_switch_state: req.kill_switch,
                    rollback_plan: req.rollback_plan,
                    fail_closed: true,
                };
            }
            Some(p) => p,
        };

        PublishGateResult {
            run_id: req.run_id,
            approved: true,
            reason_code: ReleaseReasonCode::Approved,
            kill_switch_state: req.kill_switch,
            rollback_plan: Some(plan),
            fail_closed: true,
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evidence::EvidenceBundle;

    // ── Test helpers ──────────────────────────────────────────────────────

    fn green_evidence() -> ReleaseEvidenceSnapshot {
        ReleaseEvidenceSnapshot {
            intake_ok: true,
            planning_ok: true,
            proposal_ok: true,
            execution_ok: true,
            confidence_ok: true,
            pr_ok: true,
            evidence_bundle: EvidenceBundle::new("run-ok", true, true, true, true, true),
        }
    }

    fn approved_merge_result(run_id: &str) -> MergeGateResult {
        MergeGateResult {
            run_id: run_id.to_string(),
            approved: true,
            reason_code: ReleaseReasonCode::Approved,
            kill_switch_state: KillSwitchState::Inactive,
            fail_closed: true,
        }
    }

    fn approved_release_result(run_id: &str) -> ExtendedReleaseGateResult {
        ExtendedReleaseGateResult {
            run_id: run_id.to_string(),
            approved: true,
            reason_code: ReleaseReasonCode::Approved,
            kill_switch_state: KillSwitchState::Inactive,
            fail_closed: true,
        }
    }

    fn valid_rollback() -> RollbackPlan {
        RollbackPlan::new(
            "v0.53.0",
            vec![
                "git revert HEAD".to_string(),
                "cargo yank --version 0.54.0 oris-runtime".to_string(),
            ],
            true,
        )
    }

    // ── KillSwitchState ───────────────────────────────────────────────────

    #[test]
    fn autonomous_release_kill_switch_inactive_does_not_block() {
        assert!(!KillSwitchState::Inactive.is_blocking());
    }

    #[test]
    fn autonomous_release_kill_switch_active_blocks() {
        assert!(KillSwitchState::Active.is_blocking());
    }

    #[test]
    fn autonomous_release_kill_switch_incident_tripped_blocks() {
        let ks = KillSwitchState::IncidentTripped {
            incident_id: "INC-99".to_string(),
        };
        assert!(ks.is_blocking());
    }

    // ── RollbackPlan ──────────────────────────────────────────────────────

    #[test]
    fn autonomous_release_rollback_plan_ready_when_validated() {
        let plan = valid_rollback();
        assert!(plan.is_ready());
    }

    #[test]
    fn autonomous_release_rollback_plan_not_ready_when_unvalidated() {
        let plan = RollbackPlan::new("v0.53.0", vec!["git revert".to_string()], false);
        assert!(!plan.is_ready());
    }

    #[test]
    fn autonomous_release_rollback_plan_not_ready_when_empty_steps() {
        let plan = RollbackPlan::new("v0.53.0", vec![], true);
        assert!(!plan.is_ready());
    }

    #[test]
    fn autonomous_release_rollback_plan_not_ready_when_empty_ref() {
        let plan = RollbackPlan::new("", vec!["git revert".to_string()], true);
        assert!(!plan.is_ready());
    }

    // ── ReleaseEvidenceSnapshot ───────────────────────────────────────────

    #[test]
    fn autonomous_release_evidence_all_stages_complete_when_green() {
        assert!(green_evidence().all_stages_complete());
    }

    #[test]
    fn autonomous_release_evidence_incomplete_when_confidence_missing() {
        let mut ev = green_evidence();
        ev.confidence_ok = false;
        assert!(!ev.all_stages_complete());
    }

    #[test]
    fn autonomous_release_evidence_incomplete_when_pr_missing() {
        let mut ev = green_evidence();
        ev.pr_ok = false;
        assert!(!ev.all_stages_complete());
    }

    #[test]
    fn autonomous_release_evidence_incomplete_when_bundle_build_fails() {
        let mut ev = green_evidence();
        ev.evidence_bundle.build_ok = false;
        assert!(!ev.all_stages_complete());
    }

    // ── MergeGate ─────────────────────────────────────────────────────────

    #[test]
    fn autonomous_release_merge_gate_passes_approved_class() {
        let req = MergeGateRequest {
            run_id: "mg-1".to_string(),
            task_class_id: "missing-import".to_string(),
            task_risk_tier: RiskTier::Low,
            kill_switch: KillSwitchState::Inactive,
            evidence: green_evidence(),
        };
        let result = MergeGate::evaluate(req);
        assert!(result.approved);
        assert_eq!(result.reason_code, ReleaseReasonCode::Approved);
        assert!(result.fail_closed, "merge gate must always be fail_closed");
    }

    #[test]
    fn autonomous_release_merge_gate_passes_type_mismatch_class() {
        let req = MergeGateRequest {
            run_id: "mg-2".to_string(),
            task_class_id: "type-mismatch".to_string(),
            task_risk_tier: RiskTier::Low,
            kill_switch: KillSwitchState::Inactive,
            evidence: green_evidence(),
        };
        let result = MergeGate::evaluate(req);
        assert!(result.approved);
    }

    #[test]
    fn autonomous_release_merge_gate_passes_test_failure_class() {
        let req = MergeGateRequest {
            run_id: "mg-3".to_string(),
            task_class_id: "test-failure".to_string(),
            task_risk_tier: RiskTier::Low,
            kill_switch: KillSwitchState::Inactive,
            evidence: green_evidence(),
        };
        let result = MergeGate::evaluate(req);
        assert!(result.approved);
    }

    #[test]
    fn autonomous_release_merge_gate_rejects_kill_switch_active() {
        let req = MergeGateRequest {
            run_id: "mg-4".to_string(),
            task_class_id: "missing-import".to_string(),
            task_risk_tier: RiskTier::Low,
            kill_switch: KillSwitchState::Active,
            evidence: green_evidence(),
        };
        let result = MergeGate::evaluate(req);
        assert!(!result.approved);
        assert_eq!(result.reason_code, ReleaseReasonCode::KillSwitchActive);
        assert!(result.fail_closed);
    }

    #[test]
    fn autonomous_release_merge_gate_rejects_incident_tripped_kill_switch() {
        let req = MergeGateRequest {
            run_id: "mg-5".to_string(),
            task_class_id: "missing-import".to_string(),
            task_risk_tier: RiskTier::Low,
            kill_switch: KillSwitchState::IncidentTripped {
                incident_id: "INC-42".to_string(),
            },
            evidence: green_evidence(),
        };
        let result = MergeGate::evaluate(req);
        assert!(!result.approved);
        assert_eq!(result.reason_code, ReleaseReasonCode::KillSwitchActive);
    }

    #[test]
    fn autonomous_release_merge_gate_rejects_ineligible_class() {
        let req = MergeGateRequest {
            run_id: "mg-6".to_string(),
            task_class_id: "performance".to_string(), // not in approved set
            task_risk_tier: RiskTier::Low,
            kill_switch: KillSwitchState::Inactive,
            evidence: green_evidence(),
        };
        let result = MergeGate::evaluate(req);
        assert!(!result.approved);
        assert_eq!(result.reason_code, ReleaseReasonCode::IneligibleClass);
    }

    #[test]
    fn autonomous_release_merge_gate_rejects_borrow_conflict_class() {
        let req = MergeGateRequest {
            run_id: "mg-7".to_string(),
            task_class_id: "borrow-conflict".to_string(), // not in approved set
            task_risk_tier: RiskTier::Medium,
            kill_switch: KillSwitchState::Inactive,
            evidence: green_evidence(),
        };
        let result = MergeGate::evaluate(req);
        assert!(!result.approved);
        assert_eq!(result.reason_code, ReleaseReasonCode::IneligibleClass);
    }

    #[test]
    fn autonomous_release_merge_gate_rejects_high_risk_tier() {
        let req = MergeGateRequest {
            run_id: "mg-8".to_string(),
            task_class_id: "missing-import".to_string(),
            task_risk_tier: RiskTier::High,
            kill_switch: KillSwitchState::Inactive,
            evidence: green_evidence(),
        };
        let result = MergeGate::evaluate(req);
        assert!(!result.approved);
        assert_eq!(result.reason_code, ReleaseReasonCode::RiskTierExceeded);
    }

    #[test]
    fn autonomous_release_merge_gate_rejects_critical_risk_tier() {
        let req = MergeGateRequest {
            run_id: "mg-9".to_string(),
            task_class_id: "test-failure".to_string(),
            task_risk_tier: RiskTier::Critical,
            kill_switch: KillSwitchState::Inactive,
            evidence: green_evidence(),
        };
        let result = MergeGate::evaluate(req);
        assert!(!result.approved);
        assert_eq!(result.reason_code, ReleaseReasonCode::RiskTierExceeded);
    }

    #[test]
    fn autonomous_release_merge_gate_rejects_incomplete_evidence_confidence() {
        let mut ev = green_evidence();
        ev.confidence_ok = false;
        let req = MergeGateRequest {
            run_id: "mg-10".to_string(),
            task_class_id: "test-failure".to_string(),
            task_risk_tier: RiskTier::Low,
            kill_switch: KillSwitchState::Inactive,
            evidence: ev,
        };
        let result = MergeGate::evaluate(req);
        assert!(!result.approved);
        assert_eq!(result.reason_code, ReleaseReasonCode::IncompleteEvidence);
    }

    #[test]
    fn autonomous_release_merge_gate_rejects_incomplete_evidence_intake() {
        let mut ev = green_evidence();
        ev.intake_ok = false;
        let req = MergeGateRequest {
            run_id: "mg-11".to_string(),
            task_class_id: "type-mismatch".to_string(),
            task_risk_tier: RiskTier::Low,
            kill_switch: KillSwitchState::Inactive,
            evidence: ev,
        };
        let result = MergeGate::evaluate(req);
        assert!(!result.approved);
        assert_eq!(result.reason_code, ReleaseReasonCode::IncompleteEvidence);
    }

    /// Kill switch takes priority over ineligible class.
    #[test]
    fn autonomous_release_merge_gate_kill_switch_wins_over_ineligible_class() {
        let req = MergeGateRequest {
            run_id: "mg-12".to_string(),
            task_class_id: "performance".to_string(),
            task_risk_tier: RiskTier::High,
            kill_switch: KillSwitchState::Active,
            evidence: green_evidence(),
        };
        let result = MergeGate::evaluate(req);
        assert!(!result.approved);
        assert_eq!(
            result.reason_code,
            ReleaseReasonCode::KillSwitchActive,
            "kill switch must win over ineligible class"
        );
    }

    // ── ExtendedReleaseGate ───────────────────────────────────────────────

    #[test]
    fn autonomous_release_release_gate_passes_when_merge_approved_no_drift() {
        let req = ReleaseGateRequest {
            run_id: "rg-1".to_string(),
            merge_gate_result: approved_merge_result("rg-1"),
            kill_switch: KillSwitchState::Inactive,
            no_post_gate_drift: true,
        };
        let result = ExtendedReleaseGate::evaluate(req);
        assert!(result.approved);
        assert_eq!(result.reason_code, ReleaseReasonCode::Approved);
        assert!(result.fail_closed);
    }

    #[test]
    fn autonomous_release_release_gate_rejects_kill_switch() {
        let req = ReleaseGateRequest {
            run_id: "rg-2".to_string(),
            merge_gate_result: approved_merge_result("rg-2"),
            kill_switch: KillSwitchState::Active,
            no_post_gate_drift: true,
        };
        let result = ExtendedReleaseGate::evaluate(req);
        assert!(!result.approved);
        assert_eq!(result.reason_code, ReleaseReasonCode::KillSwitchActive);
    }

    #[test]
    fn autonomous_release_release_gate_rejects_unapproved_merge() {
        let denied_merge = MergeGateResult {
            run_id: "rg-3".to_string(),
            approved: false,
            reason_code: ReleaseReasonCode::IneligibleClass,
            kill_switch_state: KillSwitchState::Inactive,
            fail_closed: true,
        };
        let req = ReleaseGateRequest {
            run_id: "rg-3".to_string(),
            merge_gate_result: denied_merge,
            kill_switch: KillSwitchState::Inactive,
            no_post_gate_drift: true,
        };
        let result = ExtendedReleaseGate::evaluate(req);
        assert!(!result.approved);
        assert_eq!(result.reason_code, ReleaseReasonCode::IneligibleClass);
    }

    #[test]
    fn autonomous_release_release_gate_rejects_post_gate_drift() {
        let req = ReleaseGateRequest {
            run_id: "rg-4".to_string(),
            merge_gate_result: approved_merge_result("rg-4"),
            kill_switch: KillSwitchState::Inactive,
            no_post_gate_drift: false,
        };
        let result = ExtendedReleaseGate::evaluate(req);
        assert!(!result.approved);
        assert_eq!(result.reason_code, ReleaseReasonCode::PostGateDrift);
    }

    // ── GatedPublishGate ──────────────────────────────────────────────────

    #[test]
    fn autonomous_release_publish_gate_passes_with_valid_rollback_plan() {
        let req = PublishGateRequest {
            run_id: "pg-1".to_string(),
            release_gate_result: approved_release_result("pg-1"),
            kill_switch: KillSwitchState::Inactive,
            rollback_plan: Some(valid_rollback()),
        };
        let result = GatedPublishGate::evaluate(req);
        assert!(result.approved);
        assert_eq!(result.reason_code, ReleaseReasonCode::Approved);
        assert!(result.rollback_plan.is_some());
        assert!(result.fail_closed);
    }

    #[test]
    fn autonomous_release_publish_gate_rejects_missing_rollback_plan() {
        let req = PublishGateRequest {
            run_id: "pg-2".to_string(),
            release_gate_result: approved_release_result("pg-2"),
            kill_switch: KillSwitchState::Inactive,
            rollback_plan: None,
        };
        let result = GatedPublishGate::evaluate(req);
        assert!(!result.approved);
        assert_eq!(result.reason_code, ReleaseReasonCode::MissingRollbackPlan);
    }

    #[test]
    fn autonomous_release_publish_gate_rejects_unvalidated_rollback_plan() {
        let plan = RollbackPlan::new(
            "v0.53.0",
            vec!["git revert HEAD".to_string()],
            false, // not validated
        );
        let req = PublishGateRequest {
            run_id: "pg-3".to_string(),
            release_gate_result: approved_release_result("pg-3"),
            kill_switch: KillSwitchState::Inactive,
            rollback_plan: Some(plan),
        };
        let result = GatedPublishGate::evaluate(req);
        assert!(!result.approved);
        assert_eq!(result.reason_code, ReleaseReasonCode::MissingRollbackPlan);
    }

    #[test]
    fn autonomous_release_publish_gate_rejects_kill_switch() {
        let req = PublishGateRequest {
            run_id: "pg-4".to_string(),
            release_gate_result: approved_release_result("pg-4"),
            kill_switch: KillSwitchState::Active,
            rollback_plan: Some(valid_rollback()),
        };
        let result = GatedPublishGate::evaluate(req);
        assert!(!result.approved);
        assert_eq!(result.reason_code, ReleaseReasonCode::KillSwitchActive);
    }

    #[test]
    fn autonomous_release_publish_gate_rejects_unapproved_release_gate() {
        let denied_release = ExtendedReleaseGateResult {
            run_id: "pg-5".to_string(),
            approved: false,
            reason_code: ReleaseReasonCode::PostGateDrift,
            kill_switch_state: KillSwitchState::Inactive,
            fail_closed: true,
        };
        let req = PublishGateRequest {
            run_id: "pg-5".to_string(),
            release_gate_result: denied_release,
            kill_switch: KillSwitchState::Inactive,
            rollback_plan: Some(valid_rollback()),
        };
        let result = GatedPublishGate::evaluate(req);
        assert!(!result.approved);
        assert_eq!(result.reason_code, ReleaseReasonCode::PostGateDrift);
    }

    /// Full pipeline: merge → release → publish, all green.
    #[test]
    fn autonomous_release_full_pipeline_all_gates_pass() {
        // Merge gate
        let merge_req = MergeGateRequest {
            run_id: "full-1".to_string(),
            task_class_id: "missing-import".to_string(),
            task_risk_tier: RiskTier::Low,
            kill_switch: KillSwitchState::Inactive,
            evidence: green_evidence(),
        };
        let merge_result = MergeGate::evaluate(merge_req);
        assert!(merge_result.approved, "merge gate must pass");

        // Release gate
        let release_req = ReleaseGateRequest {
            run_id: "full-1".to_string(),
            merge_gate_result: merge_result,
            kill_switch: KillSwitchState::Inactive,
            no_post_gate_drift: true,
        };
        let release_result = ExtendedReleaseGate::evaluate(release_req);
        assert!(release_result.approved, "release gate must pass");

        // Publish gate
        let publish_req = PublishGateRequest {
            run_id: "full-1".to_string(),
            release_gate_result: release_result,
            kill_switch: KillSwitchState::Inactive,
            rollback_plan: Some(valid_rollback()),
        };
        let publish_result = GatedPublishGate::evaluate(publish_req);
        assert!(publish_result.approved, "publish gate must pass");
        assert_eq!(publish_result.reason_code, ReleaseReasonCode::Approved);
        assert!(publish_result.fail_closed);
    }

    /// Full pipeline: kill switch trips at publish stage, merge and release already passed.
    #[test]
    fn autonomous_release_full_pipeline_kill_switch_trips_at_publish() {
        let merge_result = approved_merge_result("full-2");
        let release_result = approved_release_result("full-2");

        let publish_req = PublishGateRequest {
            run_id: "full-2".to_string(),
            release_gate_result: release_result,
            kill_switch: KillSwitchState::IncidentTripped {
                incident_id: "INC-77".to_string(),
            },
            rollback_plan: Some(valid_rollback()),
        };
        let publish_result = GatedPublishGate::evaluate(publish_req);
        assert!(!publish_result.approved);
        assert_eq!(
            publish_result.reason_code,
            ReleaseReasonCode::KillSwitchActive
        );

        // Verify merge result was previously approved (documents pipeline state)
        assert!(merge_result.approved);
    }
}
