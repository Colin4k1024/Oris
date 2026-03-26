//! Integration tests for the oris-agent-contract crate.

use oris_agent_contract::{
    accept_discovered_candidate, accept_self_evolution_selection_decision,
    approve_autonomous_mutation_proposal, approve_autonomous_pr_lane,
    approve_autonomous_release_gate, approve_autonomous_task_plan, approve_semantic_replay,
    demote_asset, deny_autonomous_mutation_proposal, deny_autonomous_pr_lane,
    deny_autonomous_release_gate, deny_autonomous_task_plan, deny_discovered_candidate,
    deny_semantic_replay, fail_confidence_revalidation, pass_confidence_revalidation,
    reject_self_evolution_selection_decision, AutonomousApprovalMode, AutonomousCandidateSource,
    AutonomousIntakeReasonCode, AutonomousMutationProposal, AutonomousPrLaneDecision,
    AutonomousProposalScope, AutonomousReleaseGateDecision, AutonomousRiskTier, BoundedTaskClass,
    ConfidenceDemotionReasonCode, ConfidenceRevalidationResult, ConfidenceState, DemotionDecision,
    DiscoveredCandidate, EquivalenceExplanation, MutationProposal, ReplayFallbackContract,
    RevalidationOutcome, SelfEvolutionMutationProposalContract, SemanticReplayDecision,
    TaskEquivalenceClass,
};

#[test]
fn approve_deny_autonomous_task_plan() {
    let approved = approve_autonomous_task_plan(
        "plan-123",
        "dedupe-key-456",
        BoundedTaskClass::DocsSingleFile,
        AutonomousRiskTier::Low,
        85,
        3,
        vec!["cargo test".to_string()],
        None,
    );

    assert!(approved.approved, "approved plan should have approved=true");
    assert!(
        !approved.fail_closed,
        "approved plan should have fail_closed=false"
    );
    assert_eq!(approved.plan_id, "plan-123");
    assert_eq!(approved.dedupe_key, "dedupe-key-456");
    assert_eq!(approved.task_class, Some(BoundedTaskClass::DocsSingleFile));
    assert_eq!(approved.risk_tier, AutonomousRiskTier::Low);
    assert_eq!(approved.feasibility_score, 85);
    assert_eq!(approved.validation_budget, 3);
    assert!(approved.denial_condition.is_none());

    // Test deny path
    let denied = deny_autonomous_task_plan(
        "plan-789",
        "dedupe-key-012",
        AutonomousRiskTier::High,
        oris_agent_contract::AutonomousPlanReasonCode::DeniedHighRisk,
    );

    assert!(!denied.approved, "denied plan should have approved=false");
    assert!(
        denied.fail_closed,
        "denied plan should have fail_closed=true"
    );
    assert_eq!(denied.plan_id, "plan-789");
    assert_eq!(denied.dedupe_key, "dedupe-key-012");
    assert_eq!(denied.task_class, None);
    assert_eq!(denied.risk_tier, AutonomousRiskTier::High);
    assert_eq!(denied.feasibility_score, 0);
    assert!(denied.denial_condition.is_some());

    let denial = denied.denial_condition.unwrap();
    assert_eq!(
        denial.reason_code,
        oris_agent_contract::AutonomousPlanReasonCode::DeniedHighRisk
    );
}

#[test]
fn approve_deny_autonomous_mutation_proposal() {
    let scope = AutonomousProposalScope {
        target_paths: vec!["src/lib.rs".to_string()],
        scope_rationale: "Single file edit".to_string(),
        max_files: 1,
    };

    // Test approve path
    let approved = approve_autonomous_mutation_proposal(
        "proposal-001",
        "plan-123",
        "dedupe-key-456",
        scope.clone(),
        vec!["cargo test".to_string()],
        vec!["test fails".to_string()],
        AutonomousApprovalMode::AutoApproved,
        None,
    );

    assert!(
        approved.proposed,
        "approved proposal should have proposed=true"
    );
    assert!(
        !approved.fail_closed,
        "approved proposal should have fail_closed=false"
    );
    assert_eq!(approved.proposal_id, "proposal-001");
    assert_eq!(approved.plan_id, "plan-123");
    assert_eq!(approved.dedupe_key, "dedupe-key-456");
    assert!(approved.scope.is_some());
    assert!(approved.denial_condition.is_none());

    // Test deny path
    let denied = deny_autonomous_mutation_proposal(
        "proposal-002",
        "plan-789",
        "dedupe-key-012",
        oris_agent_contract::AutonomousProposalReasonCode::DeniedPlanNotApproved,
    );

    assert!(
        !denied.proposed,
        "denied proposal should have proposed=false"
    );
    assert!(
        denied.fail_closed,
        "denied proposal should have fail_closed=true"
    );
    assert_eq!(denied.proposal_id, "proposal-002");
    assert!(denied.scope.is_none());
    assert!(denied.denial_condition.is_some());
}

#[test]
fn approve_deny_semantic_replay() {
    let explanation = EquivalenceExplanation {
        task_equivalence_class: TaskEquivalenceClass::DocumentationEdit,
        rationale: "Single file doc edit".to_string(),
        matching_features: vec!["*.md".to_string()],
        replay_match_confidence: 92,
    };

    // Test approve path
    let approved = approve_semantic_replay("eval-001", "task-123", explanation.clone());

    assert!(
        approved.replay_decision,
        "approved semantic replay should have replay_decision=true"
    );
    assert!(
        !approved.fail_closed,
        "approved semantic replay should have fail_closed=false"
    );
    assert_eq!(approved.evaluation_id, "eval-001");
    assert_eq!(approved.task_id, "task-123");
    assert!(approved.equivalence_explanation.is_some());

    // Test deny path
    let denied = deny_semantic_replay(
        "eval-002",
        "task-456",
        oris_agent_contract::SemanticReplayReasonCode::LowConfidenceDenied,
        "confidence below threshold",
    );

    assert!(
        !denied.replay_decision,
        "denied semantic replay should have replay_decision=false"
    );
    assert!(
        denied.fail_closed,
        "denied semantic replay should have fail_closed=true"
    );
    assert_eq!(denied.evaluation_id, "eval-002");
    assert!(denied.equivalence_explanation.is_none());
}

#[test]
fn approve_deny_autonomous_release_gate() {
    // Test approve path
    let approved = approve_autonomous_release_gate("gate-001", "task-123");

    assert!(
        !approved.fail_closed,
        "approved gate should have fail_closed=false"
    );
    assert_eq!(approved.gate_id, "gate-001");
    assert_eq!(
        approved.merge_gate_result,
        oris_agent_contract::AutonomousMergeGateStatus::MergeApproved
    );
    assert_eq!(
        approved.release_gate_result,
        oris_agent_contract::AutonomousReleaseGateStatus::ReleaseApproved
    );
    assert_eq!(
        approved.publish_gate_result,
        oris_agent_contract::AutonomousPublishGateStatus::PublishApproved
    );
    assert_eq!(
        approved.kill_switch_state,
        oris_agent_contract::KillSwitchState::Inactive
    );
    assert!(approved.rollback_plan.is_none());

    // Test deny path
    let denied = deny_autonomous_release_gate(
        "gate-002",
        "task-456",
        oris_agent_contract::AutonomousReleaseReasonCode::KillSwitchActive,
        oris_agent_contract::KillSwitchState::Active,
        "kill switch triggered",
        None,
    );

    assert!(
        denied.fail_closed,
        "denied gate should have fail_closed=true"
    );
    assert_eq!(denied.gate_id, "gate-002");
    assert_eq!(
        denied.merge_gate_result,
        oris_agent_contract::AutonomousMergeGateStatus::MergeBlocked
    );
    assert_eq!(
        denied.release_gate_result,
        oris_agent_contract::AutonomousReleaseGateStatus::ReleaseBlocked
    );
    assert_eq!(
        denied.publish_gate_result,
        oris_agent_contract::AutonomousPublishGateStatus::PublishBlocked
    );
    assert_eq!(
        denied.kill_switch_state,
        oris_agent_contract::KillSwitchState::Active
    );
}

#[test]
fn approve_deny_autonomous_pr_lane() {
    let evidence_bundle = oris_agent_contract::PrEvidenceBundle {
        patch_summary: "Fix typo in README".to_string(),
        validation_passed: true,
        audit_trail: vec!["cargo fmt --check passed".to_string()],
    };

    // Test approve path
    let approved = approve_autonomous_pr_lane(
        "pr-lane-001",
        "task-123",
        "feature/fix-readme",
        evidence_bundle.clone(),
    );

    assert!(
        approved.pr_ready,
        "approved PR lane should have pr_ready=true"
    );
    assert!(
        !approved.fail_closed,
        "approved PR lane should have fail_closed=false"
    );
    assert_eq!(approved.pr_lane_id, "pr-lane-001");
    assert!(approved.branch_name.is_some());
    assert!(approved.pr_payload.is_some());
    assert!(approved.evidence_bundle.is_some());
    assert_eq!(
        approved.delivery_status,
        oris_agent_contract::AutonomousPrLaneStatus::PrReady
    );
    assert_eq!(
        approved.approval_state,
        oris_agent_contract::PrLaneApprovalState::ClassApproved
    );

    // Test deny path
    let denied = deny_autonomous_pr_lane(
        "pr-lane-002",
        "task-456",
        oris_agent_contract::AutonomousPrLaneReasonCode::TaskClassNotApproved,
        "task class not in approved set",
    );

    assert!(
        !denied.pr_ready,
        "denied PR lane should have pr_ready=false"
    );
    assert!(
        denied.fail_closed,
        "denied PR lane should have fail_closed=true"
    );
    assert_eq!(denied.pr_lane_id, "pr-lane-002");
    assert!(denied.branch_name.is_none());
    assert!(denied.pr_payload.is_none());
    assert!(denied.evidence_bundle.is_none());
    assert_eq!(
        denied.delivery_status,
        oris_agent_contract::AutonomousPrLaneStatus::Denied
    );
    assert_eq!(
        denied.approval_state,
        oris_agent_contract::PrLaneApprovalState::ClassNotApproved
    );
}

#[test]
fn pass_fail_confidence_revalidation() {
    // Test pass path
    let passed = pass_confidence_revalidation("rev-001", "asset-123", ConfidenceState::Decaying);

    assert!(
        !passed.fail_closed,
        "passed revalidation should have fail_closed=false"
    );
    assert_eq!(passed.revalidation_id, "rev-001");
    assert_eq!(passed.asset_id, "asset-123");
    assert_eq!(passed.confidence_state, ConfidenceState::Active);
    assert_eq!(passed.revalidation_result, RevalidationOutcome::Passed);
    assert_eq!(
        passed.replay_eligibility,
        oris_agent_contract::ReplayEligibility::Eligible
    );

    // Test fail path
    let failed = fail_confidence_revalidation(
        "rev-002",
        "asset-456",
        ConfidenceState::Active,
        RevalidationOutcome::Failed,
    );

    assert!(
        failed.fail_closed,
        "failed revalidation should have fail_closed=true"
    );
    assert_eq!(failed.revalidation_id, "rev-002");
    assert_eq!(failed.asset_id, "asset-456");
    assert_eq!(failed.confidence_state, ConfidenceState::Active);
    assert_eq!(failed.revalidation_result, RevalidationOutcome::Failed);
    assert_eq!(
        failed.replay_eligibility,
        oris_agent_contract::ReplayEligibility::Ineligible
    );
}

#[test]
fn demote_asset_creates_record() {
    let demotion = demote_asset(
        "demotion-001",
        "asset-123",
        ConfidenceState::Active,
        ConfidenceState::Demoted,
        ConfidenceDemotionReasonCode::ConfidenceDecayThreshold,
    );

    assert!(
        demotion.fail_closed,
        "demotion should have fail_closed=true"
    );
    assert_eq!(demotion.demotion_id, "demotion-001");
    assert_eq!(demotion.asset_id, "asset-123");
    assert_eq!(demotion.prior_state, ConfidenceState::Active);
    assert_eq!(demotion.new_state, ConfidenceState::Demoted);
    assert_eq!(
        demotion.reason_code,
        ConfidenceDemotionReasonCode::ConfidenceDecayThreshold
    );
    assert_eq!(
        demotion.replay_eligibility,
        oris_agent_contract::ReplayEligibility::Ineligible
    );
    assert!(!demotion.quarantine_transition);

    // Test quarantine transition
    let quarantine = demote_asset(
        "demotion-002",
        "asset-456",
        ConfidenceState::Demoted,
        ConfidenceState::Quarantined,
        ConfidenceDemotionReasonCode::MaxFailureCountExceeded,
    );

    assert!(quarantine.quarantine_transition);
    assert_eq!(quarantine.new_state, ConfidenceState::Quarantined);
}

#[test]
fn accept_reject_discovered_candidate() {
    // Test accept path
    let accepted = accept_discovered_candidate(
        "dedupe-001",
        AutonomousCandidateSource::CiFailure,
        BoundedTaskClass::LintFix,
        vec!["cargo clippy warning".to_string()],
        None,
    );

    assert!(
        accepted.accepted,
        "accepted candidate should have accepted=true"
    );
    assert!(
        !accepted.fail_closed,
        "accepted candidate should have fail_closed=false"
    );
    assert_eq!(accepted.dedupe_key, "dedupe-001");
    assert_eq!(
        accepted.candidate_source,
        AutonomousCandidateSource::CiFailure
    );
    assert_eq!(accepted.candidate_class, Some(BoundedTaskClass::LintFix));
    assert_eq!(accepted.reason_code, AutonomousIntakeReasonCode::Accepted);

    // Test reject path
    let denied = deny_discovered_candidate(
        "dedupe-002",
        AutonomousCandidateSource::TestRegression,
        vec!["test failed".to_string()],
        AutonomousIntakeReasonCode::UnsupportedSignalClass,
    );

    assert!(
        !denied.accepted,
        "denied candidate should have accepted=false"
    );
    assert!(
        denied.fail_closed,
        "denied candidate should have fail_closed=true"
    );
    assert_eq!(denied.dedupe_key, "dedupe-002");
    assert!(denied.candidate_class.is_none());
    assert_eq!(
        denied.reason_code,
        AutonomousIntakeReasonCode::UnsupportedSignalClass
    );
    assert!(denied.failure_reason.is_some());
    assert!(denied.recovery_hint.is_some());
}

#[test]
fn accept_reject_self_evolution_selection() {
    // Test accept path
    let accepted =
        accept_self_evolution_selection_decision(123, BoundedTaskClass::DocsSingleFile, None);

    assert!(
        accepted.selected,
        "accepted selection should have selected=true"
    );
    assert!(
        !accepted.fail_closed,
        "accepted selection should have fail_closed=false"
    );
    assert_eq!(accepted.issue_number, 123);
    assert_eq!(
        accepted.candidate_class,
        Some(BoundedTaskClass::DocsSingleFile)
    );
    assert_eq!(
        accepted.reason_code,
        Some(oris_agent_contract::SelfEvolutionSelectionReasonCode::Accepted)
    );
    assert!(accepted.failure_reason.is_none());
    assert!(accepted.recovery_hint.is_none());

    // Test reject path
    let rejected = reject_self_evolution_selection_decision(
        456,
        oris_agent_contract::SelfEvolutionSelectionReasonCode::IssueClosed,
        None,
        None,
    );

    assert!(
        !rejected.selected,
        "rejected selection should have selected=false"
    );
    assert!(
        rejected.fail_closed,
        "rejected selection should have fail_closed=true"
    );
    assert_eq!(rejected.issue_number, 456);
    assert!(rejected.candidate_class.is_none());
    assert_eq!(
        rejected.reason_code,
        Some(oris_agent_contract::SelfEvolutionSelectionReasonCode::IssueClosed)
    );
    assert!(rejected.failure_reason.is_some());
    assert!(rejected.recovery_hint.is_some());
}
