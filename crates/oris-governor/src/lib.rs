//! Policy-only governor contracts for Oris EvoKernel.

use serde::{Deserialize, Serialize};

use oris_evolution::{AssetState, BlastRadius, CandidateSource, TransitionReasonCode};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GovernorConfig {
    pub promote_after_successes: u64,
    pub max_files_changed: usize,
    pub max_lines_changed: usize,
    pub cooldown_secs: u64,
    pub retry_cooldown_secs: u64,
    pub revoke_after_replay_failures: u64,
    pub max_mutations_per_window: u64,
    pub mutation_window_secs: u64,
    pub confidence_decay_rate_per_hour: f32,
    pub max_confidence_drop: f32,
}

impl Default for GovernorConfig {
    fn default() -> Self {
        Self {
            promote_after_successes: 3,
            max_files_changed: 5,
            max_lines_changed: 300,
            cooldown_secs: 30 * 60,
            retry_cooldown_secs: 0,
            revoke_after_replay_failures: 2,
            max_mutations_per_window: 100,
            mutation_window_secs: 60 * 60,
            confidence_decay_rate_per_hour: 0.05,
            max_confidence_drop: 0.35,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CoolingWindow {
    pub cooldown_secs: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum RevocationReason {
    ReplayRegression,
    ValidationFailure,
    Manual(String),
    EvidenceIncomplete,
    EnvironmentIncompatible,
    BoundedTaskClassDenied,
}

/// Evidence completeness status for promotion gating.
///
/// Promotion requires evidence completeness to prevent incomplete
/// mutations from being promoted to the gene pool.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum EvidenceCompletenessStatus {
    /// Evidence bundle is complete and validated.
    Complete,
    /// Evidence bundle is present but not yet validated.
    PendingValidation,
    /// Evidence bundle is incomplete - missing required items.
    Incomplete,
}

impl Default for EvidenceCompletenessStatus {
    fn default() -> Self {
        Self::Incomplete
    }
}

/// Environment compatibility status for promotion gating.
///
/// Tracks whether the mutation is compatible with the current
/// environment and policy constraints.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum EnvironmentCompatibilityStatus {
    /// Environment is compatible - safe to promote.
    Compatible,
    /// Environment has drifted - requires revalidation.
    DriftDetected,
    /// Environment is incompatible - promotion blocked.
    Incompatible,
}

impl Default for EnvironmentCompatibilityStatus {
    fn default() -> Self {
        Self::Incompatible
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GovernorInput {
    pub candidate_source: CandidateSource,
    pub success_count: u64,
    pub blast_radius: BlastRadius,
    pub replay_failures: u64,
    pub recent_mutation_ages_secs: Vec<u64>,
    pub current_confidence: f32,
    pub historical_peak_confidence: f32,
    pub confidence_last_updated_secs: Option<u64>,
    /// Evidence completeness status - gates promotion if not Complete.
    #[serde(default)]
    pub evidence_completeness: EvidenceCompletenessStatus,
    /// Environment compatibility status - gates promotion if not Compatible.
    #[serde(default)]
    pub environment_compatibility: EnvironmentCompatibilityStatus,
    /// Whether the task class is bounded and approved for autonomous promotion.
    /// None means task class is not applicable or unknown.
    #[serde(default)]
    pub bounded_task_class_approved: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GovernorDecision {
    pub target_state: AssetState,
    pub reason: String,
    #[serde(default)]
    pub reason_code: TransitionReasonCode,
    pub cooling_window: Option<CoolingWindow>,
    pub revocation_reason: Option<RevocationReason>,
}

pub trait Governor: Send + Sync {
    fn evaluate(&self, input: GovernorInput) -> GovernorDecision;
}

#[derive(Clone, Debug, Default)]
pub struct DefaultGovernor {
    config: GovernorConfig,
}

impl DefaultGovernor {
    pub fn new(config: GovernorConfig) -> Self {
        Self { config }
    }

    fn cooling_window_for(&self, cooldown_secs: u64) -> Option<CoolingWindow> {
        if cooldown_secs == 0 {
            None
        } else {
            Some(CoolingWindow { cooldown_secs })
        }
    }

    fn rate_limit_cooldown(&self, input: &GovernorInput) -> Option<u64> {
        if self.config.max_mutations_per_window == 0 || self.config.mutation_window_secs == 0 {
            return None;
        }

        let in_window = input
            .recent_mutation_ages_secs
            .iter()
            .copied()
            .filter(|age| *age < self.config.mutation_window_secs)
            .collect::<Vec<_>>();
        if in_window.len() as u64 >= self.config.max_mutations_per_window {
            let oldest_in_window = in_window.into_iter().max().unwrap_or(0);
            Some(
                self.config
                    .mutation_window_secs
                    .saturating_sub(oldest_in_window),
            )
        } else {
            None
        }
    }

    fn cooling_remaining(&self, input: &GovernorInput) -> Option<u64> {
        if self.config.retry_cooldown_secs == 0 {
            return None;
        }

        let most_recent = input.recent_mutation_ages_secs.iter().copied().min()?;
        if most_recent < self.config.retry_cooldown_secs {
            Some(self.config.retry_cooldown_secs.saturating_sub(most_recent))
        } else {
            None
        }
    }

    fn decayed_confidence(&self, input: &GovernorInput) -> f32 {
        if self.config.confidence_decay_rate_per_hour <= 0.0 {
            return input.current_confidence;
        }

        let age_hours = input.confidence_last_updated_secs.unwrap_or(0) as f32 / 3600.0;
        let decay = (-self.config.confidence_decay_rate_per_hour * age_hours).exp();
        input.current_confidence * decay
    }
}

impl Governor for DefaultGovernor {
    fn evaluate(&self, input: GovernorInput) -> GovernorDecision {
        if input.replay_failures >= self.config.revoke_after_replay_failures {
            return GovernorDecision {
                target_state: AssetState::Revoked,
                reason: "replay validation failures exceeded threshold".into(),
                reason_code: TransitionReasonCode::DowngradeReplayRegression,
                cooling_window: self.cooling_window_for(self.config.retry_cooldown_secs),
                revocation_reason: Some(RevocationReason::ReplayRegression),
            };
        }

        let decayed_confidence = self.decayed_confidence(&input);
        if self.config.max_confidence_drop > 0.0
            && input.historical_peak_confidence > 0.0
            && (input.historical_peak_confidence - decayed_confidence)
                >= self.config.max_confidence_drop
        {
            return GovernorDecision {
                target_state: AssetState::Revoked,
                reason: "confidence regression exceeded threshold".into(),
                reason_code: TransitionReasonCode::DowngradeConfidenceRegression,
                cooling_window: self.cooling_window_for(self.config.retry_cooldown_secs),
                revocation_reason: Some(RevocationReason::ReplayRegression),
            };
        }

        if let Some(cooldown_secs) = self.rate_limit_cooldown(&input) {
            return GovernorDecision {
                target_state: AssetState::Candidate,
                reason: "mutation rate limit exceeded".into(),
                reason_code: TransitionReasonCode::CandidateRateLimited,
                cooling_window: self.cooling_window_for(cooldown_secs),
                revocation_reason: None,
            };
        }

        if let Some(cooldown_secs) = self.cooling_remaining(&input) {
            return GovernorDecision {
                target_state: AssetState::Candidate,
                reason: "cooling window active after recent mutation".into(),
                reason_code: TransitionReasonCode::CandidateCoolingWindow,
                cooling_window: self.cooling_window_for(cooldown_secs),
                revocation_reason: None,
            };
        }

        if input.blast_radius.files_changed > self.config.max_files_changed
            || input.blast_radius.lines_changed > self.config.max_lines_changed
        {
            return GovernorDecision {
                target_state: AssetState::Candidate,
                reason: "blast radius exceeds promotion threshold".into(),
                reason_code: TransitionReasonCode::CandidateBlastRadiusExceeded,
                cooling_window: self.cooling_window_for(self.config.retry_cooldown_secs),
                revocation_reason: None,
            };
        }

        // Evidence completeness gate: require complete evidence before promotion.
        if input.evidence_completeness != EvidenceCompletenessStatus::Complete {
            let reason = match &input.evidence_completeness {
                EvidenceCompletenessStatus::Incomplete => {
                    "evidence bundle incomplete - missing required items".into()
                }
                EvidenceCompletenessStatus::PendingValidation => {
                    "evidence bundle pending validation".into()
                }
                EvidenceCompletenessStatus::Complete => unreachable!(),
            };
            return GovernorDecision {
                target_state: AssetState::Candidate,
                reason,
                reason_code: TransitionReasonCode::CandidateCollectingEvidence,
                cooling_window: self.cooling_window_for(self.config.retry_cooldown_secs),
                revocation_reason: Some(RevocationReason::EvidenceIncomplete),
            };
        }

        // Environment compatibility gate: require compatible environment before promotion.
        if input.environment_compatibility != EnvironmentCompatibilityStatus::Compatible {
            let reason = match &input.environment_compatibility {
                EnvironmentCompatibilityStatus::Incompatible => {
                    "environment incompatible with current policy".into()
                }
                EnvironmentCompatibilityStatus::DriftDetected => {
                    "environment drift detected - revalidation required".into()
                }
                EnvironmentCompatibilityStatus::Compatible => unreachable!(),
            };
            return GovernorDecision {
                target_state: AssetState::Candidate,
                reason,
                reason_code: TransitionReasonCode::CandidateCollectingEvidence,
                cooling_window: self.cooling_window_for(self.config.retry_cooldown_secs),
                revocation_reason: Some(RevocationReason::EnvironmentIncompatible),
            };
        }

        // Bounded task class gate: require approved task class for autonomous promotion.
        if let Some(bounded_approved) = input.bounded_task_class_approved {
            if !bounded_approved {
                return GovernorDecision {
                    target_state: AssetState::Candidate,
                    reason: "task class not approved for autonomous promotion".into(),
                    reason_code: TransitionReasonCode::CandidateCollectingEvidence,
                    cooling_window: self.cooling_window_for(self.config.retry_cooldown_secs),
                    revocation_reason: Some(RevocationReason::BoundedTaskClassDenied),
                };
            }
        }

        if input.success_count >= self.config.promote_after_successes {
            return GovernorDecision {
                target_state: AssetState::Promoted,
                reason: "success threshold reached".into(),
                reason_code: TransitionReasonCode::PromotionSuccessThreshold,
                cooling_window: Some(CoolingWindow {
                    cooldown_secs: self.config.cooldown_secs,
                }),
                revocation_reason: None,
            };
        }

        GovernorDecision {
            target_state: AssetState::Candidate,
            reason: "collecting more successful executions".into(),
            reason_code: TransitionReasonCode::CandidateCollectingEvidence,
            cooling_window: None,
            revocation_reason: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_input(
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
            evidence_completeness: EvidenceCompletenessStatus::Complete,
            environment_compatibility: EnvironmentCompatibilityStatus::Compatible,
            bounded_task_class_approved: Some(true),
        }
    }

    #[test]
    fn test_promote_after_successes_threshold() {
        let governor = DefaultGovernor::new(GovernorConfig::default());
        // Should promote after 3 successes
        let result = governor.evaluate(create_test_input(3, 1, 100, 0));
        assert_eq!(result.target_state, AssetState::Promoted);
    }

    #[test]
    fn test_revoke_after_replay_failures() {
        let governor = DefaultGovernor::new(GovernorConfig {
            retry_cooldown_secs: 45,
            ..Default::default()
        });
        // Should revoke after 2 replay failures
        let result = governor.evaluate(create_test_input(5, 1, 100, 2));
        assert_eq!(result.target_state, AssetState::Revoked);
        assert!(matches!(
            result.revocation_reason,
            Some(RevocationReason::ReplayRegression)
        ));
        assert_eq!(result.cooling_window.unwrap().cooldown_secs, 45);
    }

    #[test]
    fn test_blast_radius_exceeds_threshold() {
        let governor = DefaultGovernor::new(GovernorConfig {
            retry_cooldown_secs: 90,
            ..Default::default()
        });
        // Blast radius exceeds max_files_changed
        let result = governor.evaluate(create_test_input(5, 10, 100, 0));
        assert_eq!(result.target_state, AssetState::Candidate);
        assert!(result.reason.contains("blast radius"));
        assert_eq!(result.cooling_window.unwrap().cooldown_secs, 90);
    }

    #[test]
    fn test_cooling_window_applied_on_promotion() {
        let governor = DefaultGovernor::new(GovernorConfig::default());
        let result = governor.evaluate(create_test_input(3, 1, 100, 0));
        assert!(result.cooling_window.is_some());
        assert_eq!(result.cooling_window.unwrap().cooldown_secs, 30 * 60);
    }

    #[test]
    fn test_default_config_values() {
        let config = GovernorConfig::default();
        assert_eq!(config.promote_after_successes, 3);
        assert_eq!(config.max_files_changed, 5);
        assert_eq!(config.max_lines_changed, 300);
        assert_eq!(config.cooldown_secs, 30 * 60);
        assert_eq!(config.retry_cooldown_secs, 0);
        assert_eq!(config.revoke_after_replay_failures, 2);
        assert_eq!(config.max_mutations_per_window, 100);
        assert_eq!(config.mutation_window_secs, 60 * 60);
        assert_eq!(config.confidence_decay_rate_per_hour, 0.05);
        assert_eq!(config.max_confidence_drop, 0.35);
    }

    #[test]
    fn test_rate_limit_blocks_when_window_is_full() {
        let governor = DefaultGovernor::new(GovernorConfig {
            max_mutations_per_window: 2,
            mutation_window_secs: 60,
            cooldown_secs: 0,
            ..Default::default()
        });
        let mut input = create_test_input(3, 1, 100, 0);
        input.recent_mutation_ages_secs = vec![5, 30];

        let result = governor.evaluate(input);

        assert_eq!(result.target_state, AssetState::Candidate);
        assert!(result.reason.contains("rate limit"));
        assert_eq!(result.cooling_window.unwrap().cooldown_secs, 30);
    }

    #[test]
    fn test_cooling_window_blocks_rapid_retry() {
        let governor = DefaultGovernor::new(GovernorConfig {
            retry_cooldown_secs: 60,
            ..Default::default()
        });
        let mut input = create_test_input(3, 1, 100, 0);
        input.recent_mutation_ages_secs = vec![15];

        let result = governor.evaluate(input);

        assert_eq!(result.target_state, AssetState::Candidate);
        assert!(result.reason.contains("cooling"));
        assert_eq!(result.cooling_window.unwrap().cooldown_secs, 45);
    }

    #[test]
    fn test_confidence_decay_triggers_regression_revocation() {
        let governor = DefaultGovernor::new(GovernorConfig {
            confidence_decay_rate_per_hour: 1.0,
            max_confidence_drop: 0.2,
            retry_cooldown_secs: 30,
            ..Default::default()
        });
        let mut input = create_test_input(1, 1, 100, 0);
        input.current_confidence = 0.9;
        input.historical_peak_confidence = 0.9;
        input.confidence_last_updated_secs = Some(60 * 60);

        let result = governor.evaluate(input);

        assert_eq!(result.target_state, AssetState::Revoked);
        assert!(result.reason.contains("confidence regression"));
        assert!(matches!(
            result.revocation_reason,
            Some(RevocationReason::ReplayRegression)
        ));
        assert_eq!(result.cooling_window.unwrap().cooldown_secs, 30);
    }

    // -----------------------------------------------------------------------
    // Comprehensive demotion/revocation tests (Issue #388)
    // -----------------------------------------------------------------------

    #[test]
    fn revocation_on_exact_failure_threshold() {
        let governor = DefaultGovernor::new(GovernorConfig::default());
        // Exactly at the threshold (default: 2)
        let result = governor.evaluate(create_test_input(10, 1, 10, 2));
        assert_eq!(result.target_state, AssetState::Revoked);
        assert_eq!(
            result.reason_code,
            TransitionReasonCode::DowngradeReplayRegression
        );
    }

    #[test]
    fn no_revocation_below_failure_threshold() {
        let governor = DefaultGovernor::new(GovernorConfig::default());
        // 1 failure, threshold is 2
        let result = governor.evaluate(create_test_input(10, 1, 10, 1));
        assert_ne!(result.target_state, AssetState::Revoked);
    }

    #[test]
    fn replay_failure_revocation_has_priority_over_promotion() {
        let governor = DefaultGovernor::new(GovernorConfig::default());
        // Would qualify for promotion (5 successes >= 3) but has 2 replay failures
        let result = governor.evaluate(create_test_input(5, 1, 10, 2));
        assert_eq!(result.target_state, AssetState::Revoked);
    }

    #[test]
    fn confidence_regression_revocation_with_zero_age() {
        let governor = DefaultGovernor::new(GovernorConfig {
            max_confidence_drop: 0.2,
            ..Default::default()
        });
        let input = GovernorInput {
            candidate_source: CandidateSource::Local,
            success_count: 5,
            blast_radius: BlastRadius {
                files_changed: 1,
                lines_changed: 10,
            },
            replay_failures: 0,
            recent_mutation_ages_secs: Vec::new(),
            current_confidence: 0.5,
            historical_peak_confidence: 0.9,
            confidence_last_updated_secs: Some(0), // no decay, but raw diff is 0.4 >= 0.2
            evidence_completeness: EvidenceCompletenessStatus::Complete,
            environment_compatibility: EnvironmentCompatibilityStatus::Compatible,
            bounded_task_class_approved: Some(true),
        };
        let result = governor.evaluate(input);
        assert_eq!(result.target_state, AssetState::Revoked);
        assert_eq!(
            result.reason_code,
            TransitionReasonCode::DowngradeConfidenceRegression
        );
    }

    #[test]
    fn no_confidence_regression_when_drop_is_small() {
        let governor = DefaultGovernor::new(GovernorConfig {
            max_confidence_drop: 0.35,
            ..Default::default()
        });
        let input = GovernorInput {
            candidate_source: CandidateSource::Local,
            success_count: 5,
            blast_radius: BlastRadius {
                files_changed: 1,
                lines_changed: 10,
            },
            replay_failures: 0,
            recent_mutation_ages_secs: Vec::new(),
            current_confidence: 0.7,
            historical_peak_confidence: 0.9,
            confidence_last_updated_secs: Some(0),
            evidence_completeness: EvidenceCompletenessStatus::Complete,
            environment_compatibility: EnvironmentCompatibilityStatus::Compatible,
            bounded_task_class_approved: Some(true),
        };
        let result = governor.evaluate(input);
        // drop is 0.2, threshold is 0.35 → no revocation
        assert_ne!(result.target_state, AssetState::Revoked);
    }

    #[test]
    fn decayed_confidence_accounts_for_time() {
        let governor = DefaultGovernor::new(GovernorConfig::default());
        // default decay_rate=0.05, max_confidence_drop=0.35
        let input = GovernorInput {
            candidate_source: CandidateSource::Local,
            success_count: 3,
            blast_radius: BlastRadius {
                files_changed: 1,
                lines_changed: 10,
            },
            replay_failures: 0,
            recent_mutation_ages_secs: Vec::new(),
            current_confidence: 0.8,
            historical_peak_confidence: 0.8,
            confidence_last_updated_secs: Some(24 * 3600), // 24 hours
            evidence_completeness: EvidenceCompletenessStatus::Complete,
            environment_compatibility: EnvironmentCompatibilityStatus::Compatible,
            bounded_task_class_approved: Some(true),
        };
        let result = governor.evaluate(input);
        // decayed = 0.8 * exp(-0.05*24) ≈ 0.24, peak=0.8, drop=0.56 >= 0.35
        assert_eq!(result.target_state, AssetState::Revoked);
    }

    #[test]
    fn revocation_reason_codes_are_correct() {
        let governor = DefaultGovernor::new(GovernorConfig {
            retry_cooldown_secs: 10,
            ..Default::default()
        });
        // Replay regression
        let r1 = governor.evaluate(create_test_input(0, 1, 10, 5));
        assert!(matches!(
            r1.revocation_reason,
            Some(RevocationReason::ReplayRegression)
        ));
    }

    // -----------------------------------------------------------------------
    // Evidence completeness gating tests (Issue #393)
    // -----------------------------------------------------------------------

    #[test]
    fn incomplete_evidence_blocks_promotion() {
        let governor = DefaultGovernor::new(GovernorConfig::default());
        let mut input = create_test_input(10, 1, 100, 0);
        input.evidence_completeness = EvidenceCompletenessStatus::Incomplete;
        let result = governor.evaluate(input);
        assert_eq!(result.target_state, AssetState::Candidate);
        assert!(matches!(
            result.revocation_reason,
            Some(RevocationReason::EvidenceIncomplete)
        ));
    }

    #[test]
    fn pending_validation_blocks_promotion() {
        let governor = DefaultGovernor::new(GovernorConfig::default());
        let mut input = create_test_input(10, 1, 100, 0);
        input.evidence_completeness = EvidenceCompletenessStatus::PendingValidation;
        let result = governor.evaluate(input);
        assert_eq!(result.target_state, AssetState::Candidate);
        assert!(matches!(
            result.revocation_reason,
            Some(RevocationReason::EvidenceIncomplete)
        ));
    }

    #[test]
    fn complete_evidence_allows_promotion() {
        let governor = DefaultGovernor::new(GovernorConfig::default());
        let mut input = create_test_input(3, 1, 100, 0);
        input.evidence_completeness = EvidenceCompletenessStatus::Complete;
        let result = governor.evaluate(input);
        assert_eq!(result.target_state, AssetState::Promoted);
    }

    // -----------------------------------------------------------------------
    // Environment compatibility gating tests (Issue #393)
    // -----------------------------------------------------------------------

    #[test]
    fn incompatible_environment_blocks_promotion() {
        let governor = DefaultGovernor::new(GovernorConfig::default());
        let mut input = create_test_input(10, 1, 100, 0);
        input.environment_compatibility = EnvironmentCompatibilityStatus::Incompatible;
        let result = governor.evaluate(input);
        assert_eq!(result.target_state, AssetState::Candidate);
        assert!(matches!(
            result.revocation_reason,
            Some(RevocationReason::EnvironmentIncompatible)
        ));
    }

    #[test]
    fn drift_detected_blocks_promotion() {
        let governor = DefaultGovernor::new(GovernorConfig::default());
        let mut input = create_test_input(10, 1, 100, 0);
        input.environment_compatibility = EnvironmentCompatibilityStatus::DriftDetected;
        let result = governor.evaluate(input);
        assert_eq!(result.target_state, AssetState::Candidate);
        assert!(matches!(
            result.revocation_reason,
            Some(RevocationReason::EnvironmentIncompatible)
        ));
    }

    #[test]
    fn compatible_environment_allows_promotion() {
        let governor = DefaultGovernor::new(GovernorConfig::default());
        let mut input = create_test_input(3, 1, 100, 0);
        input.environment_compatibility = EnvironmentCompatibilityStatus::Compatible;
        let result = governor.evaluate(input);
        assert_eq!(result.target_state, AssetState::Promoted);
    }

    // -----------------------------------------------------------------------
    // Bounded task class gating tests (Issue #393)
    // -----------------------------------------------------------------------

    #[test]
    fn denied_bounded_task_class_blocks_promotion() {
        let governor = DefaultGovernor::new(GovernorConfig::default());
        let mut input = create_test_input(10, 1, 100, 0);
        input.bounded_task_class_approved = Some(false);
        let result = governor.evaluate(input);
        assert_eq!(result.target_state, AssetState::Candidate);
        assert!(matches!(
            result.revocation_reason,
            Some(RevocationReason::BoundedTaskClassDenied)
        ));
    }

    #[test]
    fn approved_bounded_task_class_allows_promotion() {
        let governor = DefaultGovernor::new(GovernorConfig::default());
        let mut input = create_test_input(3, 1, 100, 0);
        input.bounded_task_class_approved = Some(true);
        let result = governor.evaluate(input);
        assert_eq!(result.target_state, AssetState::Promoted);
    }

    #[test]
    fn none_bounded_task_class_allows_promotion() {
        // When bounded_task_class_approved is None, promotion proceeds based on other gates
        let governor = DefaultGovernor::new(GovernorConfig::default());
        let mut input = create_test_input(3, 1, 100, 0);
        input.bounded_task_class_approved = None;
        let result = governor.evaluate(input);
        assert_eq!(result.target_state, AssetState::Promoted);
    }
}
