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
    let _ = oris_runtime::evolution::EvoKernel::<FeatureState>::coordinate;
    let _ = oris_runtime::evolution::EvoKernel::<FeatureState>::bootstrap_if_empty;
    let _ = oris_runtime::evolution::EvoKernel::<FeatureState>::select_candidates;
    let _ = oris_runtime::governor::DefaultGovernor::default;
    let _ = oris_runtime::economics::EvuLedger::default;
    let _ = oris_runtime::spec_contract::SpecCompiler::compile;
    let envelope = oris_runtime::evolution_network::EvolutionEnvelope::publish(
        "node-a",
        Vec::<oris_runtime::evolution_network::NetworkAsset>::new(),
    );

    assert_type::<oris_runtime::agent_contract::AgentTask>();
    assert_type::<oris_runtime::agent_contract::AgentCapabilityLevel>();
    assert_type::<oris_runtime::agent_contract::AgentRole>();
    assert_type::<oris_runtime::agent_contract::CoordinationPrimitive>();
    assert_type::<oris_runtime::agent_contract::CoordinationTask>();
    assert_type::<oris_runtime::agent_contract::CoordinationMessage>();
    assert_type::<oris_runtime::agent_contract::CoordinationPlan>();
    assert_type::<oris_runtime::agent_contract::CoordinationResult>();
    assert_type::<oris_runtime::agent_contract::MutationProposal>();
    assert_type::<oris_runtime::agent_contract::ProposalTarget>();
    assert_type::<oris_runtime::agent_contract::ExecutionFeedback>();
    assert_type::<oris_runtime::agent_contract::ReplayFeedback>();
    assert_type::<oris_runtime::agent_contract::ReplayPlannerDirective>();
    assert_type::<oris_runtime::agent_contract::BoundedTaskClass>();
    assert_type::<oris_runtime::agent_contract::HumanApproval>();
    assert_type::<oris_runtime::agent_contract::SupervisedDevloopRequest>();
    assert_type::<oris_runtime::agent_contract::SupervisedDevloopStatus>();
    assert_type::<oris_runtime::agent_contract::SupervisedDevloopOutcome>();
    assert_type::<oris_runtime::economics::EconomicsSignal>();
    assert_type::<oris_runtime::economics::StakePolicy>();
    assert_type::<oris_runtime::evolution::SignalExtractionInput>();
    assert_type::<oris_runtime::evolution::SignalExtractionOutput>();
    assert_type::<oris_runtime::evolution::SeedTemplate>();
    assert_type::<oris_runtime::evolution::BootstrapReport>();
    assert_type::<oris_runtime::evolution::ValidationPlan>();
    assert_type::<oris_runtime::evolution_network::FetchQuery>();
    assert_type::<oris_runtime::governor::GovernorConfig>();
    assert_type::<oris_runtime::spec_contract::SpecDocument>();
    assert!(envelope.verify_content_hash());
}
