use evo_oris_repo::{
    build_demo_evo, current_git_head, proposal_diff, proposal_for, single_path, ExampleResult,
};
use oris_runtime::agent_contract::{
    AgentCapabilityLevel, AgentTask, ExecutionFeedback, HumanApproval, ProposalTarget,
    ReplayFeedback, SupervisedDevloopOutcome,
};
use oris_runtime::evolution::{EvoKernel, EvoSelectorInput as SelectorInput};

#[tokio::main]
async fn main() -> ExampleResult<()> {
    let workspace_root = std::env::current_dir()?;
    let evo = build_demo_evo("canonical-flow", 1)?;
    let base_revision = current_git_head(&workspace_root);

    let planner_task = AgentTask {
        id: "devloop-doc-sync".into(),
        description: "Add an EvoKernel example note through the proposal pipeline".into(),
    };
    let planner_capability = AgentCapabilityLevel::A2;
    let planner_target = ProposalTarget::Paths(vec!["docs/evokernel-example-generated.md".into()]);
    let planner_proposal = proposal_for(
        &planner_task,
        &planner_target,
        "planner-agent",
        "add the primary DEVLOOP note",
    );
    let planner_devloop = evo
        .run_supervised_devloop(
            &"example-run".into(),
            &oris_runtime::agent_contract::SupervisedDevloopRequest {
                task: planner_task.clone(),
                proposal: planner_proposal.clone(),
                approval: HumanApproval {
                    approved: true,
                    approver: Some("maintainer".into()),
                    note: Some("example flow".into()),
                },
            },
            proposal_diff(
                single_path(&planner_target),
                "EvoKernel Example",
                "planner-agent",
            ),
            base_revision.clone(),
        )
        .await?;

    let reviewer_task = AgentTask {
        id: "devloop-release-note".into(),
        description: "Add a second note from another agent source".into(),
    };
    let reviewer_capability = AgentCapabilityLevel::A1;
    let reviewer_target =
        ProposalTarget::Paths(vec!["docs/evokernel-example-review-generated.md".into()]);
    let reviewer_proposal = proposal_for(
        &reviewer_task,
        &reviewer_target,
        "review-agent",
        "add the secondary validation note",
    );
    let reviewer_outcome = evo
        .capture_from_proposal(
            &"review-run".into(),
            &reviewer_proposal,
            proposal_diff(
                single_path(&reviewer_target),
                "EvoKernel Review Example",
                "review-agent",
            ),
            base_revision,
        )
        .await?;
    let reviewer_feedback =
        EvoKernel::<evo_oris_repo::ExampleState>::feedback_for_agent(&reviewer_outcome);
    let replay_run_id = "replay-run".to_string();
    let replay_signals = reviewer_outcome.gene.signals.clone();

    let decision = evo
        .replay_or_fallback_for_run(
            &replay_run_id,
            SelectorInput {
                signals: replay_signals.clone(),
                env: reviewer_outcome.capsule.env.clone(),
                spec_id: None,
                limit: 1,
            },
        )
        .await?;
    let replay_feedback = EvoKernel::<evo_oris_repo::ExampleState>::replay_feedback_for_agent(
        &replay_signals,
        &decision,
    );

    if let Some(feedback) = planner_devloop.execution_feedback.as_ref() {
        print_feedback("planner-agent", &planner_capability, feedback);
    }
    print_devloop_outcome(&planner_devloop);
    print_feedback("review-agent", &reviewer_capability, &reviewer_feedback);
    print_replay_feedback(&replay_run_id, &replay_feedback);
    Ok(())
}

fn print_feedback(source: &str, capability: &AgentCapabilityLevel, feedback: &ExecutionFeedback) {
    println!(
        "{source} ({capability:?}) feedback: accepted={}, asset_state={:?}, summary={}",
        feedback.accepted, feedback.asset_state, feedback.summary
    );
}

fn print_replay_feedback(run_id: &str, feedback: &ReplayFeedback) {
    println!(
        "replay feedback: run_id={}, planner_directive={:?}, used_capsule={}, reasoning_steps_avoided={}, task_label={}, summary={}",
        run_id,
        feedback.planner_directive,
        feedback.used_capsule,
        feedback.reasoning_steps_avoided,
        feedback.task_label,
        feedback.summary
    );
}

fn print_devloop_outcome(outcome: &SupervisedDevloopOutcome) {
    println!(
        "supervised devloop: task_id={}, status={:?}, task_class={:?}, summary={}",
        outcome.task_id, outcome.status, outcome.task_class, outcome.summary
    );
}
