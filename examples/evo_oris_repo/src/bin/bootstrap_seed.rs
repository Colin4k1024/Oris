use evo_oris_repo::{
    build_demo_evo, current_env_fingerprint, current_git_head, proposal_diff, proposal_for,
    single_path, ExampleResult,
};
use oris_runtime::agent_contract::{AgentTask, ProposalTarget};
use oris_runtime::evolution::EvoSelectorInput as SelectorInput;

#[tokio::main]
async fn main() -> ExampleResult<()> {
    let workspace_root = std::env::current_dir()?;
    let base_revision = current_git_head(&workspace_root);
    let evo = build_demo_evo("bootstrap-seed", 1)?;

    let bootstrap_report = evo.bootstrap_if_empty(&"bootstrap-seed-run".into())?;
    println!(
        "bootstrap report: seeded={}, genes_added={}, capsules_added={}",
        bootstrap_report.seeded, bootstrap_report.genes_added, bootstrap_report.capsules_added
    );

    let replay_before = evo
        .replay_or_fallback_for_run(
            &"bootstrap-replay-before".into(),
            SelectorInput {
                signals: vec!["missing readme".into()],
                env: current_env_fingerprint(),
                spec_id: None,
                limit: 1,
            },
        )
        .await?;
    println!(
        "replay before local promotion: used_capsule={}, fallback_to_planner={}, reason={}",
        replay_before.used_capsule, replay_before.fallback_to_planner, replay_before.reason
    );

    let task = AgentTask {
        id: "bootstrap-local-promote".into(),
        description: "Promote a local README recovery mutation after bootstrap".into(),
    };
    let target = ProposalTarget::Paths(vec!["docs/evolution-bootstrap-local.md".into()]);
    let proposal = proposal_for(
        &task,
        &target,
        "bootstrap-local-agent",
        "promote local capsule for missing readme style tasks",
    );
    let capture = evo
        .capture_from_proposal(
            &"bootstrap-local-capture".into(),
            &proposal,
            proposal_diff(
                single_path(&target),
                "Evolution Bootstrap Local Promotion",
                "bootstrap-local-agent",
            ),
            base_revision,
        )
        .await?;
    println!(
        "local capture: gene_id={}, capsule_id={}, state={:?}",
        capture.gene.id, capture.capsule.id, capture.governor_decision.target_state
    );

    let replay_after = evo
        .replay_or_fallback_for_run(
            &"bootstrap-replay-after".into(),
            SelectorInput {
                signals: capture.gene.signals.clone(),
                env: capture.capsule.env.clone(),
                spec_id: None,
                limit: 1,
            },
        )
        .await?;
    println!(
        "replay after local promotion: used_capsule={}, capsule_id={:?}, reason={}",
        replay_after.used_capsule, replay_after.capsule_id, replay_after.reason
    );

    Ok(())
}
