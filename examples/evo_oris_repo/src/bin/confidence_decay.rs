//! Confidence Decay and Quarantine Example
//!
//! This example demonstrates the Evolution system's confidence lifecycle:
//! 1. Gene/Capsule created with initial confidence (0.7)
//! 2. Confidence decays over time based on age
//! 3. When confidence drops below threshold, gene is quarantined
//! 4. Successful reuse can boost confidence back up
//!
//! Run with:
//! ```bash
//! cargo run -p evo_oris_repo --bin confidence_decay --features "full-evolution-experimental"
//! ```

use evo_oris_repo::{
    build_demo_evo, current_git_head, proposal_diff, proposal_for, single_path, ExampleResult,
};
use oris_runtime::agent_contract::{AgentTask, ProposalTarget};

const MIN_REPLAY_CONFIDENCE: f32 = 0.5;
const REPLAY_CONFIDENCE_DECAY_RATE: f32 = 0.05;

fn calculate_confidence_decay(initial: f32, hours: f32) -> f32 {
    let decay = (-REPLAY_CONFIDENCE_DECAY_RATE * hours).exp();
    (initial * decay).clamp(0.0, 1.0)
}

#[tokio::main]
async fn main() -> ExampleResult<()> {
    println!("=== Confidence Decay and Quarantine Example ===\n");

    let workspace_root = std::env::current_dir()?;
    let base_revision = current_git_head(&workspace_root);
    let evo = build_demo_evo("confidence-demo", 1)?;

    // ============================================================
    // Step 1: Create initial Gene/Capsule
    // ============================================================
    println!("[Step 1] Creating Gene/Capsule with initial confidence");

    let task = AgentTask {
        id: "initial-fix".into(),
        description: "Initial code fix to create baseline Gene".into(),
    };
    let target = ProposalTarget::Paths(vec!["docs/evo-confidence-initial.md".into()]);
    let proposal = proposal_for(
        &task,
        &target,
        "agent",
        "initial fix for baseline",
    );

    let capture = evo
        .capture_from_proposal(
            &"initial-run".into(),
            &proposal,
            proposal_diff(
                single_path(&target),
                "Initial Confidence Baseline",
                "agent",
            ),
            base_revision,
        )
        .await?;

    let initial_confidence = capture.capsule.confidence;
    println!("  ✓ Gene created: {}", capture.gene.id);
    println!("  ✓ Capsule created: {}", capture.capsule.id);
    println!("  Initial confidence: {:.2}", initial_confidence);

    // ============================================================
    // Step 2: Demonstrate confidence decay over time
    // ============================================================
    println!("\n[Step 2] Confidence decay over time");

    let test_hours = vec![0.0, 1.0, 5.0, 10.0, 13.86, 20.0, 24.0, 48.0];

    println!("  Age (hours) | Confidence | Status");
    println!("  ------------|------------|--------");
    for hours in test_hours {
        let decayed = calculate_confidence_decay(initial_confidence, hours);
        let status = if decayed < MIN_REPLAY_CONFIDENCE {
            "QUARANTINE"
        } else {
            "active"
        };
        println!("  {:>10.2} | {:>10.2} | {}", hours, decayed, status);
    }

    // ============================================================
    // Step 3: Show time to quarantine
    // ============================================================
    println!("\n[Step 3] Time to quarantine");

    // Calculate time to quarantine: solve for hours when confidence = 0.5
    // confidence = 0.7 * e^(-0.05 * hours) = 0.5
    // hours = ln(0.7/0.5) / 0.05 = 0.3365 / 0.05 = 6.73 hours
    let hours_to_quarantine = (initial_confidence / MIN_REPLAY_CONFIDENCE).ln() / REPLAY_CONFIDENCE_DECAY_RATE;
    println!("  From {:.2} to {:.2}: {:.1} hours ({:.1} days)",
        initial_confidence, MIN_REPLAY_CONFIDENCE, hours_to_quarantine, hours_to_quarantine / 24.0);

    // ============================================================
    // Step 4: Demonstrate successful reuse boost
    // ============================================================
    println!("\n[Step 4] Confidence boost on successful reuse");

    // Simulate 20 hours passing
    let age_hours = 20.0;
    let current_confidence = calculate_confidence_decay(initial_confidence, age_hours);

    // Boost on successful reuse (+0.1, capped at 1.0)
    let boost_amount = 0.1;
    let boosted_confidence = (current_confidence + boost_amount).min(1.0);

    println!("  At {:.1} hours: {:.2}", age_hours, current_confidence);
    println!("  After successful reuse: +{:.2} -> {:.2}", boost_amount, boosted_confidence);

    // ============================================================
    // Step 5: Show complete lifecycle
    // ============================================================
    println!("\n[Step 5] Complete confidence lifecycle");

    println!("  DECAY_RATE: {} per hour", REPLAY_CONFIDENCE_DECAY_RATE);
    println!("  MIN_THRESHOLD: {:.2}", MIN_REPLAY_CONFIDENCE);
    println!("  BOOST_AMOUNT: +{} per successful reuse", boost_amount);

    // ============================================================
    // Summary
    // ============================================================
    println!("\n=== Summary ===");
    println!("✓ Confidence decays automatically over time");
    println!("✓ Genes/Capsules are quarantined when confidence drops below {:.2}", MIN_REPLAY_CONFIDENCE);
    println!("✓ Successful reuse can boost confidence back up");
    println!("✓ This enables automatic 'forgetting' of stale solutions");

    println!("\n=== Confidence Decay Demo Complete ===");
    Ok(())
}
