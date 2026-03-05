use evo_oris_repo::{
    build_demo_evo, current_git_head, proposal_diff, proposal_for, single_path, ExampleResult,
};
use oris_runtime::agent_contract::{AgentTask, ProposalTarget};
use oris_runtime::evolution::EvoSelectorInput as SelectorInput;

#[tokio::main]
async fn main() -> ExampleResult<()> {
    let workspace_root = std::env::current_dir()?;
    let base_revision = current_git_head(&workspace_root);
    let evo = build_demo_evo("metrics-health", 1)?;

    let metric_paths = ["docs/evolution-metrics-a.md", "docs/evolution-metrics-b.md"];
    let mut replay_signals: Option<Vec<String>> = None;
    let mut replay_env = None;

    for (index, path) in metric_paths.iter().enumerate() {
        let task = AgentTask {
            id: format!("metrics-capture-{index}"),
            description: format!("Capture mutation for metrics stream ({path})"),
        };
        let target = ProposalTarget::Paths(vec![(*path).to_string()]);
        let proposal = proposal_for(
            &task,
            &target,
            "metrics-agent",
            "populate replay metrics example",
        );

        let capture = evo
            .capture_from_proposal(
                &format!("metrics-run-{index}"),
                &proposal,
                proposal_diff(
                    single_path(&target),
                    "Evolution Metrics Capture",
                    "metrics-agent",
                ),
                base_revision.clone(),
            )
            .await?;
        if replay_signals.is_none() {
            replay_signals = Some(capture.gene.signals.clone());
            replay_env = Some(capture.capsule.env.clone());
        }
    }
    let replay_signals = replay_signals.ok_or("missing replay signals from capture")?;
    let replay_env = replay_env.ok_or("missing replay env from capture")?;

    for replay_index in 0..3 {
        let decision = evo
            .replay_or_fallback_for_run(
                &format!("metrics-replay-{replay_index}"),
                SelectorInput {
                    signals: replay_signals.clone(),
                    env: replay_env.clone(),
                    spec_id: None,
                    limit: 1,
                },
            )
            .await?;
        println!(
            "replay attempt {}: used_capsule={}, fallback={}, reason={}",
            replay_index + 1,
            decision.used_capsule,
            decision.fallback_to_planner,
            decision.reason
        );
    }

    let snapshot = evo.metrics_snapshot()?;
    println!(
        "metrics snapshot: replay_attempts_total={}, replay_success_total={}, replay_success_rate={:.3}, promoted_genes={}, promoted_capsules={}, confidence_revalidations_total={}",
        snapshot.replay_attempts_total,
        snapshot.replay_success_total,
        snapshot.replay_success_rate,
        snapshot.promoted_genes,
        snapshot.promoted_capsules,
        snapshot.confidence_revalidations_total
    );

    let health = evo.health_snapshot()?;
    println!(
        "health snapshot: status={}, last_event_seq={}, promoted_genes={}, promoted_capsules={}",
        health.status, health.last_event_seq, health.promoted_genes, health.promoted_capsules
    );

    let rendered = evo.render_metrics_prometheus()?;
    println!("prometheus metrics (first 12 lines):");
    for line in rendered.lines().take(12) {
        println!("{line}");
    }

    Ok(())
}
