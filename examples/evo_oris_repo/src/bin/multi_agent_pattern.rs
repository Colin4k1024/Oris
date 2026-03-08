//! Multi-Agent Collaboration Pattern Reuse Example
//!
//! This example demonstrates extracting and reusing collaboration patterns
//! from multi-agent workflows:
//! 1. Multiple agents collaborate to solve a task (Planner -> Coder -> Reviewer)
//! 2. The collaboration pattern is captured as a Gene
//! 3. Similar tasks can reuse the same collaboration pattern
//!
//! Run with:
//! ```bash
//! cargo run -p evo_oris_repo --bin multi_agent_pattern --features "full-evolution-experimental"
//! ```

use evo_oris_repo::{
    build_demo_evo, current_env_fingerprint, current_git_head, proposal_diff, proposal_for,
    single_path, ExampleResult,
};
use oris_runtime::agent_contract::{
    AgentRole, AgentTask, CoordinationPlan, CoordinationPrimitive, CoordinationTask, ProposalTarget,
};
use oris_runtime::evolution::EvoSelectorInput as SelectorInput;

#[tokio::main]
async fn main() -> ExampleResult<()> {
    println!("=== Multi-Agent Collaboration Pattern Reuse Example ===\n");

    let workspace_root = std::env::current_dir()?;
    let base_revision = current_git_head(&workspace_root);
    let evo = build_demo_evo("multi-agent-pattern", 1)?;

    // ============================================================
    // Round 1: Planner -> Coder -> Reviewer collaboration
    // ============================================================
    println!("[Round 1] Multi-agent collaboration: Planner -> Coder -> Reviewer");
    println!("  Task: Write and review a documentation update\n");

    // Define the collaboration pattern
    let collab_plan = CoordinationPlan {
        root_goal: "Documentation update with review".into(),
        primitive: CoordinationPrimitive::Sequential,
        tasks: vec![
            CoordinationTask {
                id: "planner-doc".into(),
                role: AgentRole::Planner,
                description: "Plan documentation structure".into(),
                depends_on: vec![],
            },
            CoordinationTask {
                id: "coder-doc".into(),
                role: AgentRole::Coder,
                description: "Write documentation content".into(),
                depends_on: vec!["planner-doc".into()],
            },
            CoordinationTask {
                id: "reviewer-doc".into(),
                role: AgentRole::Optimizer,
                description: "Optimize and finalize documentation".into(),
                depends_on: vec!["coder-doc".into()],
            },
        ],
        timeout_ms: 60_000,
        max_retries: 1,
    };

    println!("  Collaboration pattern:");
    for task in &collab_plan.tasks {
        println!("    {} -> {:?} ({})", task.id, task.role, task.description);
    }

    // Execute coordination
    let collab_result = evo.coordinate(collab_plan.clone());
    println!("  Result: {} completed, {} failed",
        collab_result.completed_tasks.len(),
        collab_result.failed_tasks.len()
    );

    // Capture the collaboration as a Gene
    let task = AgentTask {
        id: "doc-update-collab".into(),
        description: "Documentation update with Planner -> Coder -> Reviewer".into(),
    };
    let target = ProposalTarget::Paths(vec!["docs/evo-collab-pattern.md".into()]);
    let proposal = proposal_for(
        &task,
        &target,
        "collab-agent",
        "capture Planner->Coder->Reviewer pattern",
    );

    let capture = evo
        .capture_from_proposal(
            &"round-1-collab".into(),
            &proposal,
            proposal_diff(
                single_path(&target),
                "Collaboration Pattern Capture",
                "collab-agent",
            ),
            base_revision.clone(),
        )
        .await?;

    println!("  ✓ Gene captured: {}", capture.gene.id);
    println!("  ✓ Strategy: {:?}", capture.gene.strategy);

    let collab_signals = capture.gene.signals.clone();

    // ============================================================
    // Round 2: Try reuse for similar task
    // ============================================================
    println!("\n[Round 2] Similar task: Code review collaboration");
    println!("  Task: Code review with same pattern\n");

    let similar_plan = CoordinationPlan {
        root_goal: "Code review with approval".into(),
        primitive: CoordinationPrimitive::Sequential,
        tasks: vec![
            CoordinationTask {
                id: "planner-code".into(),
                role: AgentRole::Planner,
                description: "Plan code review scope".into(),
                depends_on: vec![],
            },
            CoordinationTask {
                id: "coder-code".into(),
                role: AgentRole::Coder,
                description: "Implement code changes".into(),
                depends_on: vec!["planner-code".into()],
            },
            CoordinationTask {
                id: "reviewer-code".into(),
                role: AgentRole::Optimizer,
                description: "Optimize code changes".into(),
                depends_on: vec!["coder-code".into()],
            },
        ],
        timeout_ms: 60_000,
        max_retries: 1,
    };

    // Extract signals for the similar task
    let new_signals = vec![
        "Planner".to_string(),
        "Coder".to_string(),
        "Reviewer".to_string(),
        "sequential".to_string(),
        "documentation".to_string(),
        "code".to_string(),
        "review".to_string(),
    ];

    let selector_input = SelectorInput {
        signals: new_signals,
        env: current_env_fingerprint(),
        spec_id: None,
        limit: 3,
    };

    let decision = evo
        .replay_or_fallback_for_run(&"round-2-reuse".into(), selector_input)
        .await?;

    println!("  Collaboration pattern:");
    for task in &similar_plan.tasks {
        println!("    {} -> {:?} ({})", task.id, task.role, task.description);
    }

    println!("\n=== Replay Decision ===");
    println!("  Used capsule: {}", decision.used_capsule);
    println!("  Fallback: {}", decision.fallback_to_planner);
    println!("  Reason: {}", decision.reason);

    // ============================================================
    // Round 3: Try exact reuse using captured signals
    // ============================================================
    println!("\n[Round 3] Try exact pattern match");
    println!("  Task: Another documentation update\n");

    let selector_input_exact = SelectorInput {
        signals: collab_signals.clone(),
        env: current_env_fingerprint(),
        spec_id: None,
        limit: 3,
    };

    let decision_exact = evo
        .replay_or_fallback_for_run(&"round-3-exact".into(), selector_input_exact)
        .await?;

    println!("\n=== Exact Match Decision ===");
    println!("  Used capsule: {}", decision_exact.used_capsule);
    println!("  Fallback: {}", decision_exact.fallback_to_planner);
    println!("  Reason: {}", decision_exact.reason);

    // ============================================================
    // Summary
    // ============================================================
    println!("\n=== Summary ===");
    println!("✓ Collaboration patterns can be captured as Genes");
    println!("✓ Roles: Planner, Coder, Reviewer, Repair, Optimizer");
    println!("✓ Primitives: Sequential, Parallel, Conditional");
    println!("✓ Similar workflows can reuse established patterns");

    // Show available roles
    println!("\nAvailable Agent Roles:");
    println!("  - Planner: Task planning and breakdown");
    println!("  - Coder: Implementation");
    println!("  - Reviewer: Code/doc review");
    println!("  - Repair: Error recovery");
    println!("  - Optimizer: Performance optimization");

    println!("\n=== Multi-Agent Pattern Demo Complete ===");
    Ok(())
}
