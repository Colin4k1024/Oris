#![cfg(feature = "full-evolution-experimental")]

#[derive(Clone)]
struct FeatureState;

impl oris_runtime::kernel::KernelState for FeatureState {
    fn version(&self) -> u32 {
        1
    }
}

fn assert_type<T>() {}

#[test]
fn full_evolution_experimental_paths_resolve() {
    let _ = oris_runtime::evolution::extract_deterministic_signals;
    let _ = oris_runtime::evolution::detect_from_intake_events;
    let _ = oris_runtime::evolution::intake_events_to_extractor_input;
    let _ = oris_runtime::evolution::prepare_mutation;
    let _ = oris_runtime::evolution::EvoKernel::<FeatureState>::capture_from_proposal;
    let _ = oris_runtime::evolution::EvoKernel::<FeatureState>::feedback_for_agent;
    let _ = oris_runtime::evolution::EvoKernel::<FeatureState>::replay_feedback_for_agent;
    let _ = oris_runtime::evolution::EvoKernel::<FeatureState>::run_supervised_devloop;
    let _ = oris_runtime::evolution::EvoKernel::<FeatureState>::prepare_supervised_delivery;
    let _ =
        oris_runtime::evolution::EvoKernel::<FeatureState>::evaluate_self_evolution_acceptance_gate;
    let _ = oris_runtime::evolution::EvoKernel::<FeatureState>::select_self_evolution_candidate;
    let _ =
        oris_runtime::evolution::EvoKernel::<FeatureState>::prepare_self_evolution_mutation_proposal;
    let _ = oris_runtime::evolution::EvoKernel::<FeatureState>::coordinate;
    let _ = oris_runtime::evolution::EvoKernel::<FeatureState>::bootstrap_if_empty;
    let _ = oris_runtime::evolution::EvoKernel::<FeatureState>::select_candidates;
    let _ = oris_runtime::evolution::EvoKernel::<FeatureState>::replay_roi_release_gate_contract;
    let _ =
        oris_runtime::evolution::EvoKernel::<FeatureState>::render_replay_roi_release_gate_contract_json;
    let _ = oris_runtime::governor::DefaultGovernor::default;
    let _ = oris_runtime::economics::EvuLedger::default;
    let _ = oris_runtime::spec_contract::SpecCompiler::compile;
    let _ = oris_runtime::evolution::evaluate_replay_roi_release_gate_contract_input;
    let envelope = oris_runtime::evolution_network::EvolutionEnvelope::publish(
        "node-a",
        Vec::<oris_runtime::evolution_network::NetworkAsset>::new(),
    );

    assert_type::<oris_runtime::agent_contract::AgentTask>();
    assert_type::<oris_runtime::agent_contract::AgentCapabilityLevel>();
    assert_type::<oris_runtime::agent_contract::A2aProtocol>();
    assert_type::<oris_runtime::agent_contract::A2aCapability>();
    assert_type::<oris_runtime::agent_contract::A2aHandshakeRequest>();
    assert_type::<oris_runtime::agent_contract::A2aHandshakeResponse>();
    assert_type::<oris_runtime::agent_contract::A2aTaskLifecycleState>();
    assert_type::<oris_runtime::agent_contract::A2aTaskLifecycleEvent>();
    assert_type::<oris_runtime::agent_contract::A2aTaskSessionState>();
    assert_type::<oris_runtime::agent_contract::A2aTaskSessionStartRequest>();
    assert_type::<oris_runtime::agent_contract::A2aTaskSessionDispatchRequest>();
    assert_type::<oris_runtime::agent_contract::A2aTaskSessionProgressRequest>();
    assert_type::<oris_runtime::agent_contract::A2aTaskSessionCompletionRequest>();
    assert_type::<oris_runtime::agent_contract::A2aTaskSessionProgressItem>();
    assert_type::<oris_runtime::agent_contract::A2aTaskSessionAck>();
    assert_type::<oris_runtime::agent_contract::A2aTaskSessionResult>();
    assert_type::<oris_runtime::agent_contract::A2aTaskSessionCompletionResponse>();
    assert_type::<oris_runtime::agent_contract::A2aTaskSessionSnapshot>();
    assert_type::<oris_runtime::agent_contract::A2aErrorCode>();
    assert_type::<oris_runtime::agent_contract::A2aErrorEnvelope>();
    assert_type::<oris_runtime::agent_contract::AgentRole>();
    assert_type::<oris_runtime::agent_contract::CoordinationPrimitive>();
    assert_type::<oris_runtime::agent_contract::CoordinationTask>();
    assert_type::<oris_runtime::agent_contract::CoordinationMessage>();
    assert_type::<oris_runtime::agent_contract::CoordinationPlan>();
    assert_type::<oris_runtime::agent_contract::CoordinationResult>();
    assert_type::<oris_runtime::agent_contract::MutationProposal>();
    assert_type::<oris_runtime::agent_contract::MutationProposalContractReasonCode>();
    assert_type::<oris_runtime::agent_contract::MutationProposalEvidence>();
    assert_type::<oris_runtime::agent_contract::MutationProposalValidationBudget>();
    assert_type::<oris_runtime::agent_contract::MutationProposalScope>();
    assert_type::<oris_runtime::agent_contract::SelfEvolutionMutationProposalContract>();
    assert_type::<oris_runtime::agent_contract::ProposalTarget>();
    assert_type::<oris_runtime::agent_contract::ExecutionFeedback>();
    assert_type::<oris_runtime::agent_contract::ReplayFeedback>();
    assert_type::<oris_runtime::agent_contract::ReplayPlannerDirective>();
    assert_type::<oris_runtime::agent_contract::ReplayFallbackReasonCode>();
    assert_type::<oris_runtime::agent_contract::ReplayFallbackNextAction>();
    assert_type::<oris_runtime::agent_contract::ReplayFallbackContract>();
    assert_type::<oris_runtime::agent_contract::BoundedTaskClass>();
    assert_type::<oris_runtime::agent_contract::SelfEvolutionCandidateIntakeRequest>();
    assert_type::<oris_runtime::agent_contract::SelfEvolutionSelectionReasonCode>();
    assert_type::<oris_runtime::agent_contract::SelfEvolutionSelectionDecision>();
    assert_type::<oris_runtime::agent_contract::HumanApproval>();
    assert_type::<oris_runtime::agent_contract::SupervisedDevloopRequest>();
    assert_type::<oris_runtime::agent_contract::SupervisedDevloopStatus>();
    assert_type::<oris_runtime::agent_contract::SupervisedDeliveryStatus>();
    assert_type::<oris_runtime::agent_contract::SupervisedDeliveryApprovalState>();
    assert_type::<oris_runtime::agent_contract::SupervisedDeliveryReasonCode>();
    assert_type::<oris_runtime::agent_contract::SupervisedDeliveryContract>();
    assert_type::<oris_runtime::agent_contract::SupervisedExecutionDecision>();
    assert_type::<oris_runtime::agent_contract::SupervisedValidationOutcome>();
    assert_type::<oris_runtime::agent_contract::SupervisedExecutionReasonCode>();
    assert_type::<oris_runtime::agent_contract::SupervisedDevloopOutcome>();
    assert_type::<oris_runtime::agent_contract::SelfEvolutionAuditConsistencyResult>();
    assert_type::<oris_runtime::agent_contract::SelfEvolutionAcceptanceGateReasonCode>();
    assert_type::<oris_runtime::agent_contract::SelfEvolutionApprovalEvidence>();
    assert_type::<oris_runtime::agent_contract::SelfEvolutionDeliveryOutcome>();
    assert_type::<oris_runtime::agent_contract::SelfEvolutionReasonCodeMatrix>();
    assert_type::<oris_runtime::agent_contract::SelfEvolutionAcceptanceGateInput>();
    assert_type::<oris_runtime::agent_contract::SelfEvolutionAcceptanceGateContract>();
    assert_type::<oris_runtime::economics::EconomicsSignal>();
    assert_type::<oris_runtime::economics::StakePolicy>();
    assert_type::<oris_runtime::evolution::SignalExtractionInput>();
    assert_type::<oris_runtime::evolution::SignalExtractionOutput>();
    assert_type::<oris_runtime::evolution::ReplayRoiReleaseGateThresholds>();
    assert_type::<oris_runtime::evolution::ReplayRoiReleaseGateFailClosedPolicy>();
    assert_type::<oris_runtime::evolution::ReplayRoiReleaseGateInputContract>();
    assert_type::<oris_runtime::evolution::ReplayRoiReleaseGateOutputContract>();
    assert_type::<oris_runtime::evolution::ReplayRoiReleaseGateContract>();
    assert_type::<oris_runtime::evolution::ReplayRoiReleaseGateStatus>();
    assert_type::<oris_runtime::evolution::ReplayDetectEvidence>();
    assert_type::<oris_runtime::evolution::ReplayCandidateEvidence>();
    assert_type::<oris_runtime::evolution::ReplaySelectEvidence>();
    assert_type::<oris_runtime::evolution::ReplayDecision>();
    assert_type::<oris_runtime::evolution::TransitionEvidence>();
    assert_type::<oris_runtime::evolution::TransitionReasonCode>();
    assert_type::<oris_runtime::evolution::SeedTemplate>();
    assert_type::<oris_runtime::evolution::BootstrapReport>();
    assert_type::<oris_runtime::evolution::ValidationPlan>();
    assert_type::<oris_runtime::evolution_network::FetchQuery>();
    assert_type::<oris_runtime::governor::GovernorConfig>();
    assert_type::<oris_runtime::spec_contract::SpecDocument>();
    assert_eq!(oris_runtime::agent_contract::A2A_PROTOCOL_NAME, "oris.a2a");
    assert_eq!(
        oris_runtime::agent_contract::A2A_PROTOCOL_VERSION,
        "0.1.0-experimental"
    );
    assert_eq!(
        oris_runtime::agent_contract::A2A_TASK_SESSION_PROTOCOL_VERSION,
        "0.1.0-experimental"
    );
    let handshake_req = oris_runtime::agent_contract::A2aHandshakeRequest {
        agent_id: "agent-a".to_string(),
        role: oris_runtime::agent_contract::AgentRole::Planner,
        capability_level: oris_runtime::agent_contract::AgentCapabilityLevel::A1,
        supported_protocols: vec![oris_runtime::agent_contract::A2aProtocol::current()],
        advertised_capabilities: vec![
            oris_runtime::agent_contract::A2aCapability::Coordination,
            oris_runtime::agent_contract::A2aCapability::ReplayFeedback,
        ],
    };
    assert!(handshake_req.supports_current_protocol());
    let accepted = oris_runtime::agent_contract::A2aHandshakeResponse::accept(vec![
        oris_runtime::agent_contract::A2aCapability::Coordination,
    ]);
    assert!(accepted.accepted);
    let docs_multi_file = oris_runtime::agent_contract::BoundedTaskClass::DocsMultiFile;
    assert!(matches!(
        docs_multi_file,
        oris_runtime::agent_contract::BoundedTaskClass::DocsMultiFile
    ));
    let _ = oris_runtime::agent_contract::infer_replay_fallback_reason_code;
    let _ = oris_runtime::agent_contract::normalize_replay_fallback_contract;
    let rejected = oris_runtime::agent_contract::A2aHandshakeResponse::reject(
        oris_runtime::agent_contract::A2aErrorCode::UnsupportedCapability,
        "none",
        None,
    );
    assert!(!rejected.accepted);
    assert!(envelope.verify_content_hash());
}

/// Issue #243 — GeneStore SQLite CRUD + Solidify/Reuse wiring
#[test]
fn genestore_persist_adapter_resolves() {
    // Verify SqliteGeneStorePersistAdapter is accessible via the evolution facade
    assert_type::<oris_runtime::evolution::adapters::SqliteGeneStorePersistAdapter>();

    // Verify in-memory store can be opened (no filesystem needed)
    let adapter =
        oris_runtime::evolution::adapters::SqliteGeneStorePersistAdapter::open(":memory:");
    assert!(
        adapter.is_ok(),
        "SqliteGeneStorePersistAdapter::open(':memory:') should succeed"
    );
}

/// Issue #244 — Task-Class Generalization: semantic equivalence layer
#[test]
fn task_class_resolver_paths_resolve() {
    // Type-resolution gate: TaskClass and TaskClassMatcher must be accessible via the facade
    assert_type::<oris_runtime::evolution::TaskClass>();
    assert_type::<oris_runtime::evolution::TaskClassMatcher>();

    // Verify builtin class registry is accessible and non-empty
    let classes = oris_runtime::evolution::builtin_task_classes();
    assert!(
        !classes.is_empty(),
        "builtin task classes must not be empty"
    );
    assert!(
        classes.iter().any(|c| c.id == "missing-import"),
        "missing-import class must be present"
    );

    // Verify classify() works end-to-end via the runtime facade
    let matcher = oris_runtime::evolution::TaskClassMatcher::with_builtins();
    let signals = vec!["error[E0425]: cannot find value in scope".to_string()];
    let cls = matcher
        .classify(&signals)
        .expect("E0425 signal must classify");
    assert_eq!(cls.id, "missing-import");

    // Cross-class: E0308 must NOT classify as borrow-conflict
    let signals2 = vec!["error[E0308]: mismatched types".to_string()];
    let cls2 = matcher
        .classify(&signals2)
        .expect("E0308 signal must classify");
    assert_ne!(cls2.id, "borrow-conflict");

    // signals_match_class helper is accessible and functional
    assert!(oris_runtime::evolution::signals_match_class(
        &signals,
        "missing-import",
        &classes
    ));
}

/// Issue #264 — EVO26-AUTO-01: Autonomous candidate intake types resolve via facade
#[test]
fn autonomous_candidate_intake_types_resolve() {
    assert_type::<oris_runtime::agent_contract::AutonomousCandidateSource>();
    assert_type::<oris_runtime::agent_contract::AutonomousIntakeReasonCode>();
    assert_type::<oris_runtime::agent_contract::DiscoveredCandidate>();
    assert_type::<oris_runtime::agent_contract::AutonomousIntakeInput>();
    assert_type::<oris_runtime::agent_contract::AutonomousIntakeOutput>();

    // Constructor helpers resolve and produce correct types
    let _c: oris_runtime::agent_contract::DiscoveredCandidate =
        oris_runtime::agent_contract::accept_discovered_candidate(
            "key1".to_string(),
            oris_runtime::agent_contract::AutonomousCandidateSource::CiFailure,
            oris_runtime::agent_contract::BoundedTaskClass::LintFix,
            vec![],
            None,
        );
    let _d: oris_runtime::agent_contract::DiscoveredCandidate =
        oris_runtime::agent_contract::deny_discovered_candidate(
            "key2".to_string(),
            oris_runtime::agent_contract::AutonomousCandidateSource::CiFailure,
            vec![],
            oris_runtime::agent_contract::AutonomousIntakeReasonCode::UnknownFailClosed,
        );

    // Variants accessible
    let _src = oris_runtime::agent_contract::AutonomousCandidateSource::CiFailure;
    let _code = oris_runtime::agent_contract::AutonomousIntakeReasonCode::Accepted;
}

/// Issue #265 — EVO26-AUTO-02: Bounded task planning types resolve via facade
#[test]
fn autonomous_task_planning_types_resolve() {
    assert_type::<oris_runtime::agent_contract::AutonomousRiskTier>();
    assert_type::<oris_runtime::agent_contract::AutonomousPlanReasonCode>();
    assert_type::<oris_runtime::agent_contract::AutonomousDenialCondition>();
    assert_type::<oris_runtime::agent_contract::AutonomousTaskPlan>();

    // Constructor helpers resolve and produce correct types
    let _approved: oris_runtime::agent_contract::AutonomousTaskPlan =
        oris_runtime::agent_contract::approve_autonomous_task_plan(
            "plan-id-1".to_string(),
            "dedupe-key-1".to_string(),
            oris_runtime::agent_contract::BoundedTaskClass::LintFix,
            oris_runtime::agent_contract::AutonomousRiskTier::Low,
            85u8,
            2u8,
            vec!["cargo fmt".to_string()],
            Some("approved"),
        );
    let _denied: oris_runtime::agent_contract::AutonomousTaskPlan =
        oris_runtime::agent_contract::deny_autonomous_task_plan(
            "plan-id-2".to_string(),
            "dedupe-key-2".to_string(),
            oris_runtime::agent_contract::AutonomousRiskTier::High,
            oris_runtime::agent_contract::AutonomousPlanReasonCode::DeniedHighRisk,
        );

    // Variants accessible
    let _tier = oris_runtime::agent_contract::AutonomousRiskTier::Low;
    let _code = oris_runtime::agent_contract::AutonomousPlanReasonCode::Approved;

    // Risk ordering is correct
    assert!(
        oris_runtime::agent_contract::AutonomousRiskTier::Low
            < oris_runtime::agent_contract::AutonomousRiskTier::Medium
    );
    assert!(
        oris_runtime::agent_contract::AutonomousRiskTier::Medium
            < oris_runtime::agent_contract::AutonomousRiskTier::High
    );
}

/// Issue #266 — EVO26-AUTO-03: Autonomous mutation proposal types resolve via facade
#[test]
fn autonomous_mutation_proposal_types_resolve() {
    assert_type::<oris_runtime::agent_contract::AutonomousApprovalMode>();
    assert_type::<oris_runtime::agent_contract::AutonomousProposalReasonCode>();
    assert_type::<oris_runtime::agent_contract::AutonomousProposalScope>();
    assert_type::<oris_runtime::agent_contract::AutonomousMutationProposal>();

    // Constructor helpers resolve
    let scope = oris_runtime::agent_contract::AutonomousProposalScope {
        target_paths: vec!["crates/**/*.rs".to_string()],
        scope_rationale: "test scope".to_string(),
        max_files: 3,
    };
    let _approved: oris_runtime::agent_contract::AutonomousMutationProposal =
        oris_runtime::agent_contract::approve_autonomous_mutation_proposal(
            "prop-id-1".to_string(),
            "plan-id-1".to_string(),
            "dedupe-key-1".to_string(),
            scope,
            vec!["cargo fmt".to_string()],
            vec!["revert on failure".to_string()],
            oris_runtime::agent_contract::AutonomousApprovalMode::AutoApproved,
            Some("test proposal"),
        );
    let _denied: oris_runtime::agent_contract::AutonomousMutationProposal =
        oris_runtime::agent_contract::deny_autonomous_mutation_proposal(
            "prop-id-2".to_string(),
            "plan-id-2".to_string(),
            "dedupe-key-2".to_string(),
            oris_runtime::agent_contract::AutonomousProposalReasonCode::DeniedPlanNotApproved,
        );

    // Variants accessible
    let _mode = oris_runtime::agent_contract::AutonomousApprovalMode::AutoApproved;
    let _code = oris_runtime::agent_contract::AutonomousProposalReasonCode::Proposed;
}

#[test]
fn semantic_replay_decision_types_resolve() {
    // AUTO-04 wiring gate: ensure TaskEquivalenceClass, EquivalenceExplanation,
    // SemanticReplayDecision, SemanticReplayReasonCode, approve_semantic_replay,
    // deny_semantic_replay are reachable via oris_runtime::agent_contract.

    let explanation = oris_runtime::agent_contract::EquivalenceExplanation {
        task_equivalence_class:
            oris_runtime::agent_contract::TaskEquivalenceClass::StaticAnalysisFix,
        rationale: "lint task semantic equivalence".to_string(),
        matching_features: vec!["compiler-diagnostic signal".to_string()],
        replay_match_confidence: 95,
    };
    let _approved: oris_runtime::agent_contract::SemanticReplayDecision =
        oris_runtime::agent_contract::approve_semantic_replay(
            "eval-id-1".to_string(),
            "task-id-1".to_string(),
            explanation,
        );
    let _denied: oris_runtime::agent_contract::SemanticReplayDecision =
        oris_runtime::agent_contract::deny_semantic_replay(
            "eval-id-2".to_string(),
            "task-id-2".to_string(),
            oris_runtime::agent_contract::SemanticReplayReasonCode::NoEquivalenceClassMatch,
            "no matching class",
        );

    // Variants accessible
    let _class = oris_runtime::agent_contract::TaskEquivalenceClass::Unclassified;
    let _code = oris_runtime::agent_contract::SemanticReplayReasonCode::UnknownFailClosed;
}

#[test]
fn confidence_revalidation_decision_types_resolve() {
    // AUTO-05 wiring gate: ensure ConfidenceState, RevalidationOutcome,
    // ConfidenceDemotionReasonCode, ReplayEligibility, ConfidenceRevalidationResult,
    // DemotionDecision, pass_confidence_revalidation, fail_confidence_revalidation,
    // and demote_asset are reachable via oris_runtime::agent_contract.

    let _passing: oris_runtime::agent_contract::ConfidenceRevalidationResult =
        oris_runtime::agent_contract::pass_confidence_revalidation(
            "crv-id-1".to_string(),
            "asset-id-1".to_string(),
            oris_runtime::agent_contract::ConfidenceState::Active,
        );

    let _failing: oris_runtime::agent_contract::ConfidenceRevalidationResult =
        oris_runtime::agent_contract::fail_confidence_revalidation(
            "crv-id-2".to_string(),
            "asset-id-2".to_string(),
            oris_runtime::agent_contract::ConfidenceState::Decaying,
            oris_runtime::agent_contract::RevalidationOutcome::Failed,
        );

    let _demotion: oris_runtime::agent_contract::DemotionDecision =
        oris_runtime::agent_contract::demote_asset(
            "dem-id-1".to_string(),
            "asset-id-3".to_string(),
            oris_runtime::agent_contract::ConfidenceState::Active,
            oris_runtime::agent_contract::ConfidenceState::Demoted,
            oris_runtime::agent_contract::ConfidenceDemotionReasonCode::ConfidenceDecayThreshold,
        );

    // Variants accessible
    let _state = oris_runtime::agent_contract::ConfidenceState::Quarantined;
    let _outcome = oris_runtime::agent_contract::RevalidationOutcome::ErrorFailClosed;
    let _reason = oris_runtime::agent_contract::ConfidenceDemotionReasonCode::UnknownFailClosed;
    let _eligibility = oris_runtime::agent_contract::ReplayEligibility::Ineligible;
}

#[test]
fn autonomous_pr_lane_decision_types_resolve() {
    // AUTO-06 wiring gate: ensure AutonomousPrLaneStatus, PrLaneApprovalState,
    // AutonomousPrLaneReasonCode, PrEvidenceBundle, AutonomousPrLaneDecision,
    // approve_autonomous_pr_lane, and deny_autonomous_pr_lane are reachable
    // via oris_runtime::agent_contract.

    let bundle = oris_runtime::agent_contract::PrEvidenceBundle {
        patch_summary: "fix lint warnings".to_string(),
        validation_passed: true,
        audit_trail: vec!["audit-key-1".to_string()],
    };

    let _approved: oris_runtime::agent_contract::AutonomousPrLaneDecision =
        oris_runtime::agent_contract::approve_autonomous_pr_lane(
            "prl-id-1".to_string(),
            "task-id-1".to_string(),
            "auto/task-id-1".to_string(),
            bundle,
        );

    let _denied: oris_runtime::agent_contract::AutonomousPrLaneDecision =
        oris_runtime::agent_contract::deny_autonomous_pr_lane(
            "prl-id-2".to_string(),
            "task-id-2".to_string(),
            oris_runtime::agent_contract::AutonomousPrLaneReasonCode::TaskClassNotApproved,
            "task class not in approved set",
        );

    // Variants accessible
    let _status = oris_runtime::agent_contract::AutonomousPrLaneStatus::Denied;
    let _approval = oris_runtime::agent_contract::PrLaneApprovalState::ClassNotApproved;
    let _reason = oris_runtime::agent_contract::AutonomousPrLaneReasonCode::UnknownFailClosed;
}

/// Wiring gate: AUTO-07 types and constructors resolve through oris-runtime.
#[test]
#[cfg(feature = "full-evolution-experimental")]
fn autonomous_release_gate_decision_types_resolve() {
    // approve_autonomous_release_gate and deny_autonomous_release_gate are
    // reachable via oris_runtime::agent_contract.

    let _approved: oris_runtime::agent_contract::AutonomousReleaseGateDecision =
        oris_runtime::agent_contract::approve_autonomous_release_gate(
            "gate-id-1".to_string(),
            "task-id-1".to_string(),
        );

    let rollback = oris_runtime::agent_contract::RollbackPlan {
        rollback_id: "rbk-1".to_string(),
        description: "halt release".to_string(),
        actionable: true,
    };

    let _denied: oris_runtime::agent_contract::AutonomousReleaseGateDecision =
        oris_runtime::agent_contract::deny_autonomous_release_gate(
            "gate-id-2".to_string(),
            "task-id-2".to_string(),
            oris_runtime::agent_contract::AutonomousReleaseReasonCode::KillSwitchActive,
            oris_runtime::agent_contract::KillSwitchState::Active,
            "kill switch is active",
            Some(rollback),
        );

    // Variants accessible
    let _merge = oris_runtime::agent_contract::AutonomousMergeGateStatus::MergeBlocked;
    let _release = oris_runtime::agent_contract::AutonomousReleaseGateStatus::ReleaseBlocked;
    let _publish = oris_runtime::agent_contract::AutonomousPublishGateStatus::PublishBlocked;
    let _kill = oris_runtime::agent_contract::KillSwitchState::Inactive;
    let _reason = oris_runtime::agent_contract::AutonomousReleaseReasonCode::UnknownFailClosed;
}
