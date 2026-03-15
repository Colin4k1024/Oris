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
