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
    }
}

#[test]
fn replay_failures_revoke_before_promotion() {
    let governor = DefaultGovernor::new(GovernorConfig::default());

    let decision = governor.evaluate(input(99, 1, 1, 2));

    assert_eq!(decision.target_state, AssetState::Revoked);
    assert!(matches!(
        decision.revocation_reason,
        Some(RevocationReason::ReplayRegression)
    ));
    assert!(decision.cooling_window.is_none());
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
