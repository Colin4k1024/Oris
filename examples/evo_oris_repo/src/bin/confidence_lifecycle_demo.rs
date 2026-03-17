use evo_oris_repo::ExampleResult;
use oris_evolution::{
    builtin_priors, AssetState, BayesianConfidenceUpdater, Gene, StandardConfidenceScheduler,
    MIN_REPLAY_CONFIDENCE,
};

fn format_ts(step_minutes: i64) -> String {
    let hours = step_minutes / 60;
    let minutes = step_minutes % 60;
    format!("T+{:02}:{:02}", hours, minutes)
}

fn print_row(
    ts_minutes: i64,
    stage: &str,
    posterior: f32,
    effective: f32,
    state: &str,
    note: &str,
) {
    println!(
        "{:<8} | {:<12} | {:>8.3} | {:>8.3} | {:<12} | {}",
        format_ts(ts_minutes),
        stage,
        posterior,
        effective,
        state,
        note
    );
}

fn demo_gene(id: &str, state: AssetState) -> Gene {
    Gene {
        id: id.into(),
        signals: vec!["ci_failure".into(), "compiler:error[E0308]".into()],
        strategy: vec!["tighten type handling".into()],
        validation: vec!["cargo test --release --all-features".into()],
        state,
        task_class_id: None,
    }
}

fn main() -> ExampleResult<()> {
    println!("=== Confidence Lifecycle Demo ===\n");
    println!("timestamp | stage        | posterior | effective | state        | note");
    println!("----------|--------------|-----------|-----------|--------------|------------------------------");

    let prior = builtin_priors();
    let mut updater = BayesianConfidenceUpdater::with_builtin_prior();
    let mut step_minutes = 0_i64;
    let mut gene_v1 = demo_gene("gene-v1", AssetState::Promoted);

    let initial = updater.snapshot(&prior);
    print_row(
        step_minutes,
        "promoted",
        initial.mean,
        initial.mean,
        "promoted",
        "initial prior-backed confidence",
    );

    for success_idx in 1..=10 {
        step_minutes += 15;
        updater.update_success();
        let snapshot = updater.snapshot(&prior);
        print_row(
            step_minutes,
            "success",
            snapshot.mean,
            snapshot.mean,
            "promoted",
            &format!("successful evaluation #{success_idx}"),
        );
    }

    for failure_idx in 1..=5 {
        step_minutes += 15;
        updater.update_failure();
        let snapshot = updater.snapshot(&prior);
        print_row(
            step_minutes,
            "failure",
            snapshot.mean,
            snapshot.mean,
            "promoted",
            &format!("failed evaluation #{failure_idx}"),
        );
    }

    let post_failures = updater.snapshot(&prior);
    let mut decay_hours = 0.0_f32;
    let mut decayed = post_failures.mean;
    while decayed >= MIN_REPLAY_CONFIDENCE {
        step_minutes += 120;
        decay_hours += 2.0;
        decayed = StandardConfidenceScheduler::calculate_decay(post_failures.mean, decay_hours);
        let state = if decayed < MIN_REPLAY_CONFIDENCE {
            "re-evolve"
        } else {
            "promoted"
        };
        let note = if decayed < MIN_REPLAY_CONFIDENCE {
            "confidence below replay threshold"
        } else {
            "time-decay applied"
        };
        print_row(
            step_minutes,
            "decay",
            post_failures.mean,
            decayed,
            state,
            note,
        );
    }

    gene_v1.state = AssetState::Archived;
    print_row(
        step_minutes,
        "retire",
        post_failures.mean,
        decayed,
        "archived",
        "old gene retired after re-evolution trigger",
    );

    let mut gene_v2 = demo_gene("gene-v2", AssetState::Candidate);
    let mut replacement = BayesianConfidenceUpdater::with_builtin_prior();
    replacement.update(4, 0);
    let replacement_snapshot = replacement.snapshot(&prior);
    gene_v2.state = AssetState::Promoted;
    step_minutes += 30;
    print_row(
        step_minutes,
        "promote-v2",
        replacement_snapshot.mean,
        replacement_snapshot.mean,
        "promoted",
        "new gene promoted to replace archived v1",
    );

    println!("\nSummary:");
    println!("- v1 final state: {:?}", gene_v1.state);
    println!("- v2 final state: {:?}", gene_v2.state);
    println!("- replay threshold: {:.2}", MIN_REPLAY_CONFIDENCE);
    println!("\n=== Confidence Lifecycle Demo Complete ===");
    Ok(())
}
