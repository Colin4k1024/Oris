//! Agent Task Execution Path Reuse Example
//!
//! This example demonstrates capturing and reusing agent task execution paths:
//! 1. Agent solves a problem using a specific strategy/steps
//! 2. The strategy is captured as Gene with execution path
//! 3. Similar problem arrives - system tries to reuse the execution path
//! 4. If reuse succeeds, no LLM reasoning needed
//!
//! Run with:
//! ```bash
//! cargo run -p evo_oris_repo --bin task_path_reuse --features "full-evolution-experimental"
//! ```

use evo_oris_repo::{
    build_demo_evo, current_env_fingerprint, current_git_head, proposal_diff, proposal_for,
    single_path, ExampleResult,
};
use oris_runtime::agent_contract::{AgentTask, ProposalTarget};
use oris_runtime::evolution::EvoSelectorInput as SelectorInput;

#[tokio::main]
async fn main() -> ExampleResult<()> {
    println!("=== Agent Task Execution Path Reuse Example ===\n");

    let workspace_root = std::env::current_dir()?;
    let base_revision = current_git_head(&workspace_root);
    let evo = build_demo_evo("task-path-reuse", 1)?;

    // ============================================================
    // Round 1: Agent solves "Add logging to function"
    // ============================================================
    println!("[Round 1] Agent solves: Add logging to function");
    println!("  Task: Add debug logging to a Rust function\n");

    let task_1 = AgentTask {
        id: "add-logging".into(),
        description: "Add debug logging to function using tracing crate".into(),
    };
    let target_1 = ProposalTarget::Paths(vec!["docs/evo-logging-solution.md".into()]);
    let proposal_1 = proposal_for(
        &task_1,
        &target_1,
        "code-agent",
        "add tracing::info! macro to log function entry/exit",
    );

    let diff_1 = proposal_diff(single_path(&target_1), "Logging Solution", "code-agent");

    println!("  Capturing execution path...");
    let capture_1 = evo
        .capture_from_proposal(
            &"round-1-logging".into(),
            &proposal_1,
            diff_1,
            base_revision.clone(),
        )
        .await?;

    println!("  ✓ Gene created: {}", capture_1.gene.id);
    println!("  ✓ Strategy captured: {:?}", capture_1.gene.strategy);
    println!("  ✓ Signals: {:?}", capture_1.gene.signals);

    // Store for replay test
    let round1_signals = capture_1.gene.signals.clone();
    let round1_env = capture_1.capsule.env.clone();

    // ============================================================
    // Round 2: Agent solves "Add metrics to function"
    // ============================================================
    println!("\n[Round 2] Agent solves: Add metrics to function");
    println!("  Task: Add performance metrics to a Rust function\n");

    let task_2 = AgentTask {
        id: "add-metrics".into(),
        description: "Add performance metrics to function using metrics crate".into(),
    };
    let target_2 = ProposalTarget::Paths(vec!["docs/evo-metrics-solution.md".into()]);
    let proposal_2 = proposal_for(
        &task_2,
        &target_2,
        "code-agent",
        "add metrics::counter! macro to track function calls",
    );

    let diff_2 = proposal_diff(single_path(&target_2), "Metrics Solution", "code-agent");

    println!("  Capturing second execution path...");
    let capture_2 = evo
        .capture_from_proposal(
            &"round-2-metrics".into(),
            &proposal_2,
            diff_2,
            base_revision.clone(),
        )
        .await?;

    println!("  ✓ Gene created: {}", capture_2.gene.id);
    println!("  ✓ Strategy captured: {:?}", capture_2.gene.strategy);

    // ============================================================
    // Round 3: Try to reuse path for "Add caching"
    // ============================================================
    println!("\n[Round 3] Task: Add caching to function");
    println!("  Similar to logging/metrics - instrumenting functions\n");

    // Extract signals that match the "instrumentation" pattern
    let new_signals = vec![
        "instrument".to_string(),
        "function".to_string(),
        "add".to_string(),
        "tracing".to_string(),
        "metrics".to_string(),
    ];

    println!("  Attempting to reuse execution path...");
    let selector_input = SelectorInput {
        signals: new_signals.clone(),
        env: current_env_fingerprint(),
        spec_id: None,
        limit: 3,
    };

    let decision = evo
        .replay_or_fallback_for_run(&"round-3-reuse".into(), selector_input)
        .await?;

    println!("\n=== Replay Decision ===");
    println!("  Used capsule: {}", decision.used_capsule);
    println!("  Fallback: {}", decision.fallback_to_planner);
    println!("  Reason: {}", decision.reason);

    // ============================================================
    // Round 4: Try exact match from Round 1
    // ============================================================
    println!("\n[Round 4] Try to reuse Round 1 path (logging)");
    println!("  Task: Add debug logging to another function\n");

    // Use signals from Round 1
    let selector_input_2 = SelectorInput {
        signals: round1_signals.clone(),
        env: round1_env,
        spec_id: None,
        limit: 3,
    };

    let decision_2 = evo
        .replay_or_fallback_for_run(&"round-4-reuse".into(), selector_input_2)
        .await?;

    println!("\n=== Replay Decision (Round 1 reuse) ===");
    println!("  Used capsule: {}", decision_2.used_capsule);
    println!("  Fallback: {}", decision_2.fallback_to_planner);
    println!("  Reason: {}", decision_2.reason);

    // ============================================================
    // Summary
    // ============================================================
    println!("\n=== Summary ===");
    println!("✓ Execution paths can be captured as Genes");
    println!("✓ Strategy contains the steps taken to solve problems");
    println!("✓ Similar problems can reuse previous execution paths");
    println!("✓ This reduces LLM reasoning for repetitive tasks");

    println!("\n=== Task Path Reuse Demo Complete ===");
    Ok(())
}
