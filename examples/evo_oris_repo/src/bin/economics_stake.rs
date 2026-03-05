use std::sync::{Arc, Mutex};

use evo_oris_repo::{
    build_demo_evo, current_git_head, proposal_diff, proposal_for, single_path, ExampleResult,
};
use oris_runtime::agent_contract::{AgentTask, ProposalTarget};
use oris_runtime::economics::{EvuAccount, EvuLedger, StakePolicy};

async fn capture_one_promoted_mutation(
    label: &str,
    path: &str,
    evo: &oris_runtime::evolution::EvoKernel<evo_oris_repo::ExampleState>,
    base_revision: Option<String>,
) -> ExampleResult<()> {
    let task = AgentTask {
        id: format!("economics-{label}"),
        description: format!("Capture mutation for economics stake demo ({label})"),
    };
    let target = ProposalTarget::Paths(vec![path.to_string()]);
    let proposal = proposal_for(
        &task,
        &target,
        "economics-agent",
        "create one promoted mutation",
    );

    evo.capture_from_proposal(
        &format!("economics-capture-{label}"),
        &proposal,
        proposal_diff(
            single_path(&target),
            "Evolution Economics Stake",
            "economics-agent",
        ),
        base_revision,
    )
    .await?;

    Ok(())
}

#[tokio::main]
async fn main() -> ExampleResult<()> {
    let workspace_root = std::env::current_dir()?;
    let base_revision = current_git_head(&workspace_root);

    let no_balance = build_demo_evo("economics-no-balance", 1)?;
    capture_one_promoted_mutation(
        "no-balance",
        "docs/evolution-economics-no-balance.md",
        &no_balance,
        base_revision.clone(),
    )
    .await?;

    match no_balance.export_promoted_assets("node-zero") {
        Ok(envelope) => {
            println!(
                "unexpected success without EVU: exported assets={}",
                envelope.assets.len()
            );
        }
        Err(err) => {
            println!("expected publish rejection without EVU: {err}");
        }
    }

    let rich_ledger = Arc::new(Mutex::new(EvuLedger {
        accounts: vec![EvuAccount {
            node_id: "node-rich".into(),
            balance: 6,
        }],
        reputations: vec![],
    }));

    let rich = build_demo_evo("economics-rich", 1)?
        .with_economics(rich_ledger)
        .with_stake_policy(StakePolicy {
            publish_cost: 2,
            ..StakePolicy::default()
        });

    capture_one_promoted_mutation(
        "rich",
        "docs/evolution-economics-rich.md",
        &rich,
        base_revision,
    )
    .await?;

    let envelope = rich.export_promoted_assets("node-rich")?;
    let signal = rich
        .economics_signal("node-rich")
        .ok_or("missing economics signal for node-rich")?;

    println!(
        "publish with EVU succeeded: exported assets={}, available_evu={}, selector_weight={:.3}",
        envelope.assets.len(),
        signal.available_evu,
        signal.selector_weight
    );

    Ok(())
}
