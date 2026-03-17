use std::sync::{Arc, Mutex};

use evo_oris_repo::{
    build_demo_evo, current_env_fingerprint, current_git_head, proposal_diff, proposal_for,
    single_path, ExampleResult,
};
use oris_runtime::agent_contract::{AgentTask, ProposalTarget};
use oris_runtime::economics::{EvuAccount, EvuLedger};
use oris_runtime::evolution::{EvoSelectorInput as SelectorInput, FetchQuery};
use oris_runtime::evolution_network::NetworkAsset;

#[tokio::main]
async fn main() -> ExampleResult<()> {
    let workspace_root = std::env::current_dir()?;
    let base_revision = current_git_head(&workspace_root);

    let node_a_ledger = Arc::new(Mutex::new(EvuLedger {
        accounts: vec![EvuAccount {
            node_id: "node-a".into(),
            balance: 5,
        }],
        reputations: vec![],
    }));

    let node_a = build_demo_evo("network-node-a", 1)?.with_economics(node_a_ledger);
    let node_b = build_demo_evo("network-node-b", 1)?;

    let pre_import_health = node_b.health_snapshot()?;
    println!(
        "node-b before import: promoted_genes={}, promoted_capsules={}",
        pre_import_health.promoted_genes, pre_import_health.promoted_capsules
    );

    let task = AgentTask {
        id: "network-publish".into(),
        description: "Create a promoted mutation that can be exported to another node".into(),
    };
    let target = ProposalTarget::Paths(vec!["docs/evolution-network-exchange.md".into()]);
    let proposal = proposal_for(
        &task,
        &target,
        "node-a-agent",
        "publish one promoted mutation for remote reuse",
    );
    node_a
        .capture_from_proposal(
            &"network-node-a-capture".into(),
            &proposal,
            proposal_diff(
                single_path(&target),
                "Evolution Network Exchange",
                "node-a-agent",
            ),
            base_revision,
        )
        .await?;

    let envelope = node_a.export_promoted_assets("node-a")?;
    println!(
        "node-a export: protocol={}, assets={}, hash_ok={}",
        envelope.protocol,
        envelope.assets.len(),
        envelope.verify_content_hash()
    );

    let import = node_b.import_remote_envelope(&envelope)?;
    println!(
        "node-b import: accepted={}, imported_asset_ids={}",
        import.accepted,
        import.imported_asset_ids.len()
    );

    let post_import_health = node_b.health_snapshot()?;
    println!(
        "node-b after import: promoted_genes={}, promoted_capsules={}",
        post_import_health.promoted_genes, post_import_health.promoted_capsules
    );

    let imported_gene_signals = envelope
        .assets
        .iter()
        .find_map(|asset| match asset {
            NetworkAsset::Gene { gene } => Some(gene.signals.clone()),
            _ => None,
        })
        .ok_or("missing gene signals from exported envelope")?;
    let imported_env = envelope
        .assets
        .iter()
        .find_map(|asset| match asset {
            NetworkAsset::Capsule { capsule } => Some(capsule.env.clone()),
            _ => None,
        })
        .unwrap_or_else(current_env_fingerprint);

    let replay = node_b
        .replay_or_fallback_for_run(
            &"network-node-b-replay".into(),
            SelectorInput {
                signals: imported_gene_signals.clone(),
                env: imported_env,
                spec_id: None,
                limit: 1,
            },
        )
        .await?;
    println!(
        "node-b local replay validation: used_capsule={}, fallback_to_planner={}, reason={}",
        replay.used_capsule, replay.fallback_to_planner, replay.reason
    );
    println!(
        "node-b auto-promotion evidence: imported_assets={}, remote_capsule_reused={}",
        import.imported_asset_ids.len(),
        replay.used_capsule
    );

    let fetch = node_b.fetch_assets(
        "node-b",
        &FetchQuery {
            sender_id: "node-b".into(),
            signals: imported_gene_signals,
            since_cursor: None,
            resume_token: None,
        },
    )?;
    println!(
        "node-b fetch response: sender_id={}, assets={}",
        fetch.sender_id,
        fetch.assets.len()
    );

    let health = node_b.health_snapshot()?;
    println!(
        "node-b health: status={}, promoted_genes={}, promoted_capsules={}, last_event_seq={}",
        health.status, health.promoted_genes, health.promoted_capsules, health.last_event_seq
    );

    Ok(())
}
