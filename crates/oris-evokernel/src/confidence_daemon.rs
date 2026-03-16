//! Background confidence control daemon — Issue #283 (Stream D).
//!
//! `ConfidenceDaemon` is a `tokio::spawn` background task that periodically
//! revalidates tracked assets and applies automatic demotion/quarantine when
//! their confidence drops below `MIN_REPLAY_CONFIDENCE`.
//!
//! # Design
//!
//! ```text
//! ConfidenceDaemon::spawn()
//!     └─ tokio background task
//!           ↓ every poll_interval
//!       for each TrackedAsset:
//!           evaluate_confidence_revalidation()
//!             → Failed? → evaluate_asset_demotion()
//!                           → new_state = Quarantined|Demoted
//!           update shared state
//! ```
//!
//! Quarantined assets set `replay_eligible = false` on their `TrackedAsset`
//! entry, which callers must consult before replay selection.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use oris_agent_contract::{
    ConfidenceDemotionReasonCode, ConfidenceRevalidationResult, ConfidenceState, DemotionDecision,
    ReplayEligibility, RevalidationOutcome,
};
use oris_evolution::MIN_REPLAY_CONFIDENCE;
use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;
use tokio::time;

// ── ConfidenceDaemonConfig ─────────────────────────────────────────────────

/// Configuration for `ConfidenceDaemon`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConfidenceDaemonConfig {
    /// How often the daemon wakes up and revalidates all tracked assets.
    pub poll_interval: Duration,
    /// Confidence score below which an asset is eligible for automatic demotion.
    /// Must be `<= MIN_REPLAY_CONFIDENCE`.
    pub demotion_confidence_threshold: f32,
}

impl Default for ConfidenceDaemonConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(60),
            demotion_confidence_threshold: MIN_REPLAY_CONFIDENCE,
        }
    }
}

// ── TrackedAsset ───────────────────────────────────────────────────────────

/// A single asset tracked by the confidence daemon.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrackedAsset {
    /// Unique asset identifier (gene id or capsule id).
    pub asset_id: String,
    /// Current confidence lifecycle state.
    pub state: ConfidenceState,
    /// Number of consecutive failures recorded against this asset.
    pub failure_count: u32,
    /// Current decayed confidence score in `[0.0, 1.0]`.
    pub decayed_confidence: f32,
    /// Whether this asset is eligible for replay.  `false` for quarantined
    /// assets — callers must not select quarantined assets for replay.
    pub replay_eligible: bool,
}

impl TrackedAsset {
    /// Create a new healthy asset entry.
    pub fn new(asset_id: impl Into<String>, decayed_confidence: f32) -> Self {
        let replay_eligible = decayed_confidence >= MIN_REPLAY_CONFIDENCE;
        Self {
            asset_id: asset_id.into(),
            state: ConfidenceState::Active,
            failure_count: 0,
            decayed_confidence,
            replay_eligible,
        }
    }
}

// ── ConfidenceEvaluator ────────────────────────────────────────────────────

/// Trait that provides the two EvoKernel evaluation methods needed by the
/// daemon.  Implemented by `EvoKernel` and by test doubles.
pub trait ConfidenceEvaluator: Send + Sync {
    /// Determine whether an asset passes or fails revalidation.
    fn evaluate_confidence_revalidation(
        &self,
        asset_id: &str,
        current_state: ConfidenceState,
        failure_count: u32,
    ) -> ConfidenceRevalidationResult;

    /// Determine the new state (Demoted / Quarantined) after a failed
    /// revalidation.
    fn evaluate_asset_demotion(
        &self,
        asset_id: &str,
        prior_state: ConfidenceState,
        failure_count: u32,
        reason_code: ConfidenceDemotionReasonCode,
    ) -> DemotionDecision;
}

// ── ConfidenceDaemon ───────────────────────────────────────────────────────

/// Background daemon that periodically revalidates tracked assets and
/// quarantines those whose confidence falls below `MIN_REPLAY_CONFIDENCE`.
pub struct ConfidenceDaemon {
    assets: Arc<Mutex<Vec<TrackedAsset>>>,
    evaluator: Arc<dyn ConfidenceEvaluator>,
    config: ConfidenceDaemonConfig,
}

impl ConfidenceDaemon {
    /// Create a new daemon with the given evaluator and configuration.
    pub fn new(evaluator: Arc<dyn ConfidenceEvaluator>, config: ConfidenceDaemonConfig) -> Self {
        Self {
            assets: Arc::new(Mutex::new(Vec::new())),
            evaluator,
            config,
        }
    }

    /// Convenience constructor with default configuration.
    pub fn with_defaults(evaluator: Arc<dyn ConfidenceEvaluator>) -> Self {
        Self::new(evaluator, ConfidenceDaemonConfig::default())
    }

    /// Register an asset for confidence tracking.
    ///
    /// If the asset_id is already registered, its entry is updated.
    pub fn track(&self, asset: TrackedAsset) {
        let mut guard = self.assets.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(existing) = guard.iter_mut().find(|a| a.asset_id == asset.asset_id) {
            *existing = asset;
        } else {
            guard.push(asset);
        }
    }

    /// Return a snapshot of all currently tracked assets.
    pub fn snapshot(&self) -> Vec<TrackedAsset> {
        self.assets
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// Run one revalidation cycle synchronously.
    ///
    /// For each tracked asset:
    /// 1. Call `evaluate_confidence_revalidation()`.
    /// 2. If failed, call `evaluate_asset_demotion()`.
    /// 3. Transition to `Quarantined` (or `Demoted`) and set
    ///    `replay_eligible = false` for quarantined assets.
    ///
    /// Assets below `demotion_confidence_threshold` are treated as having an
    /// implicit failure to trigger revalidation.
    pub fn run_cycle(&self) {
        let mut guard = self.assets.lock().unwrap_or_else(|p| p.into_inner());
        let evaluator = Arc::clone(&self.evaluator);
        let threshold = self.config.demotion_confidence_threshold;

        for asset in guard.iter_mut() {
            // Skip already-quarantined assets — they are excluded from replay
            // and don't need further demotion cycles.
            if asset.state == ConfidenceState::Quarantined {
                asset.replay_eligible = false;
                continue;
            }

            // Determine if below confidence threshold — treat as a failure.
            let effective_failure_count = if asset.decayed_confidence < threshold {
                asset.failure_count.saturating_add(1)
            } else {
                asset.failure_count
            };

            let revalidation = evaluator.evaluate_confidence_revalidation(
                &asset.asset_id,
                asset.state,
                effective_failure_count,
            );

            let revalidation_failed = matches!(
                revalidation.revalidation_result,
                RevalidationOutcome::Failed | RevalidationOutcome::ErrorFailClosed
            ) || revalidation.fail_closed;

            if revalidation_failed {
                // Escalate failure count and call demotion.
                asset.failure_count = effective_failure_count;
                let demotion = evaluator.evaluate_asset_demotion(
                    &asset.asset_id,
                    asset.state,
                    asset.failure_count,
                    ConfidenceDemotionReasonCode::ConfidenceDecayThreshold,
                );
                // Apply the new state.
                asset.state = demotion.new_state;
                asset.replay_eligible = demotion.replay_eligibility == ReplayEligibility::Eligible;
            } else {
                // Passed — restore eligibility if previously suspended.
                if asset.decayed_confidence >= threshold {
                    asset.replay_eligible = true;
                }
            }
        }
    }

    /// Spawn the daemon as a background task.
    ///
    /// The returned `JoinHandle` runs indefinitely until the process
    /// terminates or the handle is aborted.  Callers may call
    /// `handle.abort()` to stop the daemon cleanly.
    pub fn spawn(self) -> JoinHandle<()> {
        let poll_interval = self.config.poll_interval;
        let daemon = Arc::new(self);
        tokio::spawn(async move {
            let mut interval = time::interval(poll_interval);
            loop {
                interval.tick().await;
                daemon.run_cycle();
            }
        })
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Minimal ConfidenceEvaluator test double ──────────────────────────

    struct StubEvaluator;

    impl ConfidenceEvaluator for StubEvaluator {
        fn evaluate_confidence_revalidation(
            &self,
            asset_id: &str,
            current_state: ConfidenceState,
            failure_count: u32,
        ) -> ConfidenceRevalidationResult {
            let failed = failure_count >= 3;
            ConfidenceRevalidationResult {
                revalidation_id: format!("crv-{asset_id}"),
                asset_id: asset_id.to_string(),
                confidence_state: current_state,
                revalidation_result: if failed {
                    RevalidationOutcome::Failed
                } else {
                    RevalidationOutcome::Passed
                },
                replay_eligibility: if failed {
                    ReplayEligibility::Ineligible
                } else {
                    ReplayEligibility::Eligible
                },
                summary: format!("stub: failure_count={failure_count}"),
                fail_closed: failed,
            }
        }

        fn evaluate_asset_demotion(
            &self,
            asset_id: &str,
            prior_state: ConfidenceState,
            failure_count: u32,
            reason_code: ConfidenceDemotionReasonCode,
        ) -> DemotionDecision {
            let new_state = if failure_count >= 5 {
                ConfidenceState::Quarantined
            } else {
                ConfidenceState::Demoted
            };
            DemotionDecision {
                demotion_id: format!("dem-{asset_id}"),
                asset_id: asset_id.to_string(),
                prior_state,
                new_state,
                reason_code,
                replay_eligibility: ReplayEligibility::Ineligible,
                summary: format!("stub demotion: new_state={new_state:?}"),
                quarantine_transition: new_state == ConfidenceState::Quarantined,
                fail_closed: true,
            }
        }
    }

    fn daemon_with_stub() -> ConfidenceDaemon {
        ConfidenceDaemon::new(
            Arc::new(StubEvaluator),
            ConfidenceDaemonConfig {
                poll_interval: Duration::from_secs(1),
                demotion_confidence_threshold: MIN_REPLAY_CONFIDENCE,
            },
        )
    }

    // ── confidence_daemon_healthy_asset_stays_eligible ──────────────────

    #[test]
    fn confidence_daemon_healthy_asset_stays_eligible() {
        let daemon = daemon_with_stub();
        daemon.track(TrackedAsset::new("gene-ok", 1.0));

        daemon.run_cycle();

        let snap = daemon.snapshot();
        let asset = snap.iter().find(|a| a.asset_id == "gene-ok").unwrap();
        assert!(
            asset.replay_eligible,
            "healthy asset should remain eligible"
        );
        assert_ne!(asset.state, ConfidenceState::Quarantined);
    }

    // ── confidence_daemon_below_threshold_triggers_demotion ─────────────

    #[test]
    fn confidence_daemon_below_threshold_triggers_demotion() {
        let daemon = daemon_with_stub();
        // Start with 2 existing failures and confidence below threshold.
        let mut asset = TrackedAsset::new("gene-low", 0.0);
        asset.failure_count = 2;
        daemon.track(asset);

        // After run_cycle, effective_failure_count = 3 → revalidation fails → Demoted.
        daemon.run_cycle();

        let snap = daemon.snapshot();
        let a = snap.iter().find(|a| a.asset_id == "gene-low").unwrap();
        assert!(
            matches!(
                a.state,
                ConfidenceState::Demoted | ConfidenceState::Quarantined
            ),
            "asset below threshold should be demoted, got {:?}",
            a.state
        );
        assert!(
            !a.replay_eligible,
            "demoted asset must not be replay eligible"
        );
    }

    // ── confidence_daemon_quarantine_auto_transition ─────────────────────

    #[test]
    fn confidence_daemon_quarantine_auto_transition() {
        let daemon = daemon_with_stub();
        // 4 existing failures + 1 from below-threshold push = 5 → Quarantined.
        let mut asset = TrackedAsset::new("gene-quarantine", 0.0);
        asset.failure_count = 4;
        daemon.track(asset);

        daemon.run_cycle();

        let snap = daemon.snapshot();
        let a = snap
            .iter()
            .find(|a| a.asset_id == "gene-quarantine")
            .unwrap();
        assert_eq!(
            a.state,
            ConfidenceState::Quarantined,
            "asset with 5 failures should be Quarantined"
        );
        assert!(!a.replay_eligible);
    }

    // ── confidence_daemon_quarantined_excluded_from_replay ───────────────

    #[test]
    fn confidence_daemon_quarantined_excluded_from_replay() {
        let daemon = daemon_with_stub();
        let mut already_quarantined = TrackedAsset::new("gene-q", 0.0);
        already_quarantined.state = ConfidenceState::Quarantined;
        already_quarantined.replay_eligible = false;
        daemon.track(already_quarantined);

        // Run a cycle — quarantined asset should remain excluded.
        daemon.run_cycle();

        let snap = daemon.snapshot();
        let a = snap.iter().find(|a| a.asset_id == "gene-q").unwrap();
        assert_eq!(a.state, ConfidenceState::Quarantined);
        assert!(
            !a.replay_eligible,
            "quarantined asset must never become eligible"
        );
    }

    // ── confidence_daemon_spawn_returns_join_handle ──────────────────────

    #[tokio::test]
    async fn confidence_daemon_spawn_returns_join_handle() {
        let config = ConfidenceDaemonConfig {
            poll_interval: Duration::from_millis(50),
            demotion_confidence_threshold: MIN_REPLAY_CONFIDENCE,
        };
        let daemon = ConfidenceDaemon::new(Arc::new(StubEvaluator), config);
        let handle = daemon.spawn();
        // Let it tick once.
        tokio::time::sleep(Duration::from_millis(120)).await;
        // Abort and confirm it was running.
        handle.abort();
        // aborted handle returns Err(JoinError::Cancelled)
        let result = handle.await;
        assert!(result.is_err(), "aborted handle should return an error");
    }

    // ── confidence_daemon_multiple_assets_independent ────────────────────

    #[test]
    fn confidence_daemon_multiple_assets_independent() {
        let daemon = daemon_with_stub();
        daemon.track(TrackedAsset::new("gene-a", 1.0)); // healthy
        let mut b = TrackedAsset::new("gene-b", 0.0);
        b.failure_count = 4;
        daemon.track(b); // will be quarantined

        daemon.run_cycle();

        let snap = daemon.snapshot();
        let a = snap.iter().find(|a| a.asset_id == "gene-a").unwrap();
        let b = snap.iter().find(|a| a.asset_id == "gene-b").unwrap();
        assert!(a.replay_eligible, "healthy asset must stay eligible");
        assert!(!b.replay_eligible, "quarantined asset must not be eligible");
        assert_eq!(b.state, ConfidenceState::Quarantined);
    }
}
