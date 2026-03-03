use oris_evolution::{AssetState, BlastRadius, CandidateSource};
use oris_governor::{DefaultGovernor, Governor, GovernorConfig, GovernorInput, RevocationReason};

fn input(
    success_count: u64,
    files_changed: usize,
    lines_changed: usize,
    replay_failures: u64,
) -> GovernorInput {
    GovernorInput {
        candidate_source: CandidateSource::Local,
        success_count,
        blast_radius: BlastRadius {
            files_changed,
            lines_changed,
        },
        replay_failures,
        recent_mutation_ages_secs: Vec::new(),
        current_confidence: 0.7,
        historical_peak_confidence: 0.7,
        confidence_last_updated_secs: Some(0),
    }
}

#[test]
fn replay_failures_revoke_before_promotion() {
    let governor = DefaultGovernor::new(GovernorConfig {
        retry_cooldown_secs: 45,
        ..Default::default()
    });

    let decision = governor.evaluate(input(99, 1, 1, 2));

    assert_eq!(decision.target_state, AssetState::Revoked);
    assert!(matches!(
        decision.revocation_reason,
        Some(RevocationReason::ReplayRegression)
    ));
    assert_eq!(decision.cooling_window.unwrap().cooldown_secs, 45);
}

#[test]
fn blast_radius_limit_blocks_promotion_at_success_threshold() {
    let governor = DefaultGovernor::new(GovernorConfig::default());

    let decision = governor.evaluate(input(3, 6, 100, 0));

    assert_eq!(decision.target_state, AssetState::Candidate);
    assert!(decision.reason.contains("blast radius"));
}

#[test]
fn promotion_threshold_emits_configured_cooling_window() {
    let governor = DefaultGovernor::new(GovernorConfig {
        promote_after_successes: 2,
        cooldown_secs: 42,
        ..Default::default()
    });

    let decision = governor.evaluate(input(2, 1, 10, 0));

    assert_eq!(decision.target_state, AssetState::Promoted);
    assert_eq!(decision.cooling_window.unwrap().cooldown_secs, 42);
}

#[test]
fn mutation_rate_limit_returns_window_cooldown() {
    let governor = DefaultGovernor::new(GovernorConfig {
        max_mutations_per_window: 2,
        mutation_window_secs: 60,
        cooldown_secs: 0,
        ..Default::default()
    });
    let mut candidate = input(5, 1, 10, 0);
    candidate.recent_mutation_ages_secs = vec![8, 25];

    let decision = governor.evaluate(candidate);

    assert_eq!(decision.target_state, AssetState::Candidate);
    assert!(decision.reason.contains("rate limit"));
    assert_eq!(decision.cooling_window.unwrap().cooldown_secs, 35);
}

#[test]
fn cooling_window_blocks_rapid_repromotion_attempts() {
    let governor = DefaultGovernor::new(GovernorConfig {
        retry_cooldown_secs: 90,
        ..Default::default()
    });
    let mut candidate = input(5, 1, 10, 0);
    candidate.recent_mutation_ages_secs = vec![30];

    let decision = governor.evaluate(candidate);

    assert_eq!(decision.target_state, AssetState::Candidate);
    assert!(decision.reason.contains("cooling"));
    assert_eq!(decision.cooling_window.unwrap().cooldown_secs, 60);
}

#[test]
fn confidence_decay_can_trigger_regression_revocation() {
    let governor = DefaultGovernor::new(GovernorConfig {
        confidence_decay_rate_per_hour: 1.0,
        max_confidence_drop: 0.2,
        retry_cooldown_secs: 30,
        ..Default::default()
    });
    let mut candidate = input(1, 1, 10, 0);
    candidate.current_confidence = 0.9;
    candidate.historical_peak_confidence = 0.9;
    candidate.confidence_last_updated_secs = Some(60 * 60);

    let decision = governor.evaluate(candidate);

    assert_eq!(decision.target_state, AssetState::Revoked);
    assert!(decision.reason.contains("confidence regression"));
    assert!(matches!(
        decision.revocation_reason,
        Some(RevocationReason::ReplayRegression)
    ));
    assert_eq!(decision.cooling_window.unwrap().cooldown_secs, 30);
}
