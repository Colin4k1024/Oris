use oris_agent_contract::{
    approve_autonomous_release_gate, deny_autonomous_release_gate, AutonomousMergeGateStatus,
    AutonomousPublishGateStatus, AutonomousReleaseGateStatus, AutonomousReleaseReasonCode,
    KillSwitchState, RollbackPlan,
};

fn make_rollback(id: &str) -> RollbackPlan {
    RollbackPlan {
        rollback_id: id.to_string(),
        description: format!("rollback for {id}"),
        actionable: true,
    }
}

#[test]
fn autonomous_release_gate_approved_for_docs_single_file() {
    let decision = approve_autonomous_release_gate("gate-1", "task-1");
    assert_eq!(
        decision.merge_gate_result,
        AutonomousMergeGateStatus::MergeApproved
    );
    assert_eq!(
        decision.release_gate_result,
        AutonomousReleaseGateStatus::ReleaseApproved
    );
    assert_eq!(
        decision.publish_gate_result,
        AutonomousPublishGateStatus::PublishApproved
    );
    assert_eq!(decision.kill_switch_state, KillSwitchState::Inactive);
    assert_eq!(
        decision.reason_code,
        AutonomousReleaseReasonCode::ApprovedForAutonomousRelease
    );
    assert!(!decision.fail_closed);
    assert!(decision.rollback_plan.is_none());
}

#[test]
fn autonomous_release_gate_denied_when_kill_switch_active() {
    let rollback = make_rollback("rbk-ks");
    let decision = deny_autonomous_release_gate(
        "gate-2",
        "task-2",
        AutonomousReleaseReasonCode::KillSwitchActive,
        KillSwitchState::Active,
        "kill switch is active",
        Some(rollback),
    );
    assert_eq!(
        decision.merge_gate_result,
        AutonomousMergeGateStatus::MergeBlocked
    );
    assert_eq!(decision.kill_switch_state, KillSwitchState::Active);
    assert_eq!(
        decision.reason_code,
        AutonomousReleaseReasonCode::KillSwitchActive
    );
    assert!(decision.fail_closed);
    assert!(decision.rollback_plan.is_some());
}

#[test]
fn autonomous_release_gate_denied_for_unapproved_task_class() {
    let decision = deny_autonomous_release_gate(
        "gate-3",
        "task-3",
        AutonomousReleaseReasonCode::TaskClassNotApproved,
        KillSwitchState::Inactive,
        "task class not approved",
        None,
    );
    assert_eq!(
        decision.reason_code,
        AutonomousReleaseReasonCode::TaskClassNotApproved
    );
    assert_eq!(
        decision.publish_gate_result,
        AutonomousPublishGateStatus::PublishBlocked
    );
    assert!(decision.fail_closed);
    assert!(decision.rollback_plan.is_none());
}

#[test]
fn autonomous_release_gate_denied_for_incomplete_evidence() {
    let rollback = make_rollback("rbk-ev");
    let decision = deny_autonomous_release_gate(
        "gate-4",
        "task-4",
        AutonomousReleaseReasonCode::IncompleteStageEvidence,
        KillSwitchState::Inactive,
        "evidence from a prior stage is incomplete",
        Some(rollback.clone()),
    );
    assert_eq!(
        decision.reason_code,
        AutonomousReleaseReasonCode::IncompleteStageEvidence
    );
    assert!(decision.fail_closed);
    assert_eq!(decision.rollback_plan, Some(rollback));
}

#[test]
fn autonomous_release_gate_reason_codes_and_statuses_are_stable() {
    let _ = AutonomousReleaseReasonCode::ApprovedForAutonomousRelease;
    let _ = AutonomousReleaseReasonCode::TaskClassNotApproved;
    let _ = AutonomousReleaseReasonCode::IncompleteStageEvidence;
    let _ = AutonomousReleaseReasonCode::KillSwitchActive;
    let _ = AutonomousReleaseReasonCode::RiskTierTooHigh;
    let _ = AutonomousReleaseReasonCode::PostGateDriftDetected;
    let _ = AutonomousReleaseReasonCode::UnknownFailClosed;
    let _ = AutonomousMergeGateStatus::MergeApproved;
    let _ = AutonomousMergeGateStatus::MergeBlocked;
    let _ = AutonomousReleaseGateStatus::ReleaseApproved;
    let _ = AutonomousReleaseGateStatus::ReleaseBlocked;
    let _ = AutonomousPublishGateStatus::PublishApproved;
    let _ = AutonomousPublishGateStatus::PublishBlocked;
    let _ = KillSwitchState::Active;
    let _ = KillSwitchState::Inactive;
}
