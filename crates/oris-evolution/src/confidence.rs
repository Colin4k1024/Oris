//! Confidence Lifecycle Scheduler
//!
//! This module implements automatic confidence decay and lifecycle management
//! for genes and capsules within the evolution crate.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::core::{
    AssetState, Capsule, GeneId, MIN_REPLAY_CONFIDENCE, REPLAY_CONFIDENCE_DECAY_RATE_PER_HOUR,
};

/// Confidence scheduler configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConfidenceSchedulerConfig {
    /// How often to run the decay check (in seconds)
    pub check_interval_secs: u64,
    /// Maximum confidence boost per reuse success
    pub confidence_boost_per_success: f32,
    /// Maximum confidence (capped at 1.0)
    pub max_confidence: f32,
    /// Enable/disable the scheduler
    pub enabled: bool,
}

impl Default for ConfidenceSchedulerConfig {
    fn default() -> Self {
        Self {
            check_interval_secs: 3600, // 1 hour
            confidence_boost_per_success: 0.1,
            max_confidence: 1.0,
            enabled: true,
        }
    }
}

/// Confidence update action
#[derive(Clone, Debug)]
pub enum ConfidenceAction {
    /// Apply decay to a capsule
    DecayCapsule {
        capsule_id: String,
        gene_id: GeneId,
        old_confidence: f32,
        new_confidence: f32,
    },
    /// Demote asset to quarantined due to low confidence
    DemoteToQuarantined { asset_id: String, confidence: f32 },
    /// Boost confidence on successful reuse
    BoostConfidence {
        asset_id: String,
        old_confidence: f32,
        new_confidence: f32,
    },
}

/// Scheduler errors
#[derive(Error, Debug)]
pub enum SchedulerError {
    #[error("Scheduler not running")]
    NotRunning,

    #[error("IO error: {0}")]
    IoError(String),

    #[error("Store error: {0}")]
    StoreError(String),
}

/// Trait for confidence lifecycle management
pub trait ConfidenceScheduler: Send + Sync {
    /// Apply confidence decay to a single capsule
    fn apply_decay_to_capsule(&self, capsule_confidence: f32, age_hours: f32) -> f32;

    /// Boost confidence on successful reuse
    fn boost_confidence(&self, current: f32) -> f32;

    /// Check if confidence is below minimum threshold
    fn should_quarantine(&self, confidence: f32) -> bool;
}

/// Standard implementation of confidence scheduler
pub struct StandardConfidenceScheduler {
    config: ConfidenceSchedulerConfig,
}

impl StandardConfidenceScheduler {
    pub fn new(config: ConfidenceSchedulerConfig) -> Self {
        Self { config }
    }

    pub fn with_default_config() -> Self {
        Self::new(ConfidenceSchedulerConfig::default())
    }

    /// Calculate decayed confidence
    pub fn calculate_decay(confidence: f32, hours: f32) -> f32 {
        if confidence <= 0.0 {
            return 0.0;
        }
        let decay = (-REPLAY_CONFIDENCE_DECAY_RATE_PER_HOUR * hours).exp();
        (confidence * decay).clamp(0.0, 1.0)
    }

    /// Calculate age in hours from a timestamp
    pub fn calculate_age_hours(created_at_ms: i64, now_ms: i64) -> f32 {
        let diff_ms = now_ms - created_at_ms;
        let diff_secs = diff_ms / 1000;
        diff_secs as f32 / 3600.0
    }
}

impl ConfidenceScheduler for StandardConfidenceScheduler {
    fn apply_decay_to_capsule(&self, capsule_confidence: f32, age_hours: f32) -> f32 {
        Self::calculate_decay(capsule_confidence, age_hours)
    }

    fn boost_confidence(&self, current: f32) -> f32 {
        let new_confidence = current + self.config.confidence_boost_per_success;
        new_confidence.min(self.config.max_confidence)
    }

    fn should_quarantine(&self, confidence: f32) -> bool {
        confidence < MIN_REPLAY_CONFIDENCE
    }
}

/// Confidence metrics
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ConfidenceMetrics {
    pub decay_checks_total: u64,
    pub capsules_decayed_total: u64,
    pub capsules_quarantined_total: u64,
    pub confidence_boosts_total: u64,
}

/// Apply decay to a capsule and return actions
pub fn process_capsule_confidence(
    scheduler: &dyn ConfidenceScheduler,
    capsule_id: &str,
    gene_id: &GeneId,
    confidence: f32,
    created_at_ms: i64,
    current_time_ms: i64,
    state: AssetState,
) -> Vec<ConfidenceAction> {
    let mut actions = Vec::new();

    // Only process promoted capsules
    if state != AssetState::Promoted {
        return actions;
    }

    let age_hours =
        StandardConfidenceScheduler::calculate_age_hours(created_at_ms, current_time_ms);

    if age_hours > 0.0 {
        let old_conf = confidence;
        let new_conf = scheduler.apply_decay_to_capsule(old_conf, age_hours);

        if (new_conf - old_conf).abs() > 0.001 {
            actions.push(ConfidenceAction::DecayCapsule {
                capsule_id: capsule_id.to_string(),
                gene_id: gene_id.clone(),
                old_confidence: old_conf,
                new_confidence: new_conf,
            });
        }

        // Check quarantine threshold
        if scheduler.should_quarantine(new_conf) {
            actions.push(ConfidenceAction::DemoteToQuarantined {
                asset_id: capsule_id.to_string(),
                confidence: new_conf,
            });
        }
    }

    actions
}

// ---------------------------------------------------------------------------
// ConfidenceController — continuous failure-rate-based confidence tracking
// ---------------------------------------------------------------------------

/// A single outcome record associated with an asset.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OutcomeRecord {
    pub asset_id: String,
    pub success: bool,
    pub recorded_at_ms: i64,
}

/// Configuration for [`ConfidenceController`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ControllerConfig {
    /// Rolling time window in milliseconds used to compute failure rate
    /// (default: 3 600 000 ms = 1 hour).
    pub window_ms: i64,
    /// Failure-rate threshold in [0.0, 1.0] that triggers a downgrade step
    /// (default: 0.5).
    pub failure_rate_threshold: f32,
    /// Minimum number of outcomes inside the window before any downgrade
    /// decision is made (default: 3).
    pub min_samples: usize,
    /// Confidence amount subtracted per downgrade step, also used as the
    /// recovery boost per success (default: 0.15).
    pub downgrade_penalty: f32,
    /// Assets with confidence strictly below this value are considered
    /// non-selectable and require re-validation (default:
    /// `MIN_REPLAY_CONFIDENCE`).
    pub min_selectable_confidence: f32,
    /// Confidence assigned to assets that are not yet tracked (default: 1.0).
    pub initial_confidence: f32,
}

impl Default for ControllerConfig {
    fn default() -> Self {
        Self {
            window_ms: 3_600_000,
            failure_rate_threshold: 0.5,
            min_samples: 3,
            downgrade_penalty: 0.15,
            min_selectable_confidence: MIN_REPLAY_CONFIDENCE,
            initial_confidence: 1.0,
        }
    }
}

/// An observability event emitted whenever an asset is automatically
/// downgraded by the [`ConfidenceController`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DowngradeEvent {
    pub asset_id: String,
    pub old_confidence: f32,
    pub new_confidence: f32,
    /// Observed failure rate inside the rolling window.
    pub failure_rate: f32,
    /// Number of outcomes that were inside the window.
    pub window_samples: usize,
    pub event_at_ms: i64,
    /// `true` when `new_confidence` fell below `min_selectable_confidence`,
    /// indicating that re-validation should be triggered.
    pub revalidation_required: bool,
}

/// Continuous confidence controller for genes and capsules.
///
/// Tracks per-asset success / failure outcomes within a rolling time window.
/// When the failure rate exceeds the configured threshold and the minimum
/// sample count is met, the asset's confidence score is automatically
/// downgraded and a [`DowngradeEvent`] is appended to an internal
/// observability log.  Successive successes can recover confidence.
///
/// # Selector integration
///
/// Call [`is_selectable`](ConfidenceController::is_selectable) before
/// choosing a gene/capsule for reuse.  Assets below
/// [`ControllerConfig::min_selectable_confidence`] return `false` and
/// should be skipped until they have been re-validated.
pub struct ConfidenceController {
    config: ControllerConfig,
    scores: HashMap<String, f32>,
    history: HashMap<String, Vec<OutcomeRecord>>,
    downgrade_log: Vec<DowngradeEvent>,
}

impl ConfidenceController {
    /// Create a new controller with the given configuration.
    pub fn new(config: ControllerConfig) -> Self {
        Self {
            config,
            scores: HashMap::new(),
            history: HashMap::new(),
            downgrade_log: Vec::new(),
        }
    }

    /// Create a controller using [`ControllerConfig::default`].
    pub fn with_default_config() -> Self {
        Self::new(ControllerConfig::default())
    }

    /// Current confidence score for `asset_id`.
    /// Returns [`ControllerConfig::initial_confidence`] for unknown assets.
    pub fn confidence(&self, asset_id: &str) -> f32 {
        self.scores
            .get(asset_id)
            .copied()
            .unwrap_or(self.config.initial_confidence)
    }

    /// Returns `true` when the asset's confidence is at or above
    /// [`ControllerConfig::min_selectable_confidence`].
    pub fn is_selectable(&self, asset_id: &str) -> bool {
        self.confidence(asset_id) >= self.config.min_selectable_confidence
    }

    /// Record a **successful** outcome for `asset_id` at `now_ms`.
    ///
    /// Applies a recovery boost (capped at `initial_confidence`).
    pub fn record_success(&mut self, asset_id: &str, now_ms: i64) {
        self.history
            .entry(asset_id.to_string())
            .or_default()
            .push(OutcomeRecord {
                asset_id: asset_id.to_string(),
                success: true,
                recorded_at_ms: now_ms,
            });
        let initial = self.config.initial_confidence;
        let penalty = self.config.downgrade_penalty;
        let entry = self.scores.entry(asset_id.to_string()).or_insert(initial);
        *entry = (*entry + penalty).min(initial);
    }

    /// Record a **failure** outcome for `asset_id` at `now_ms`.
    ///
    /// Immediately evaluates the rolling-window failure rate and downgrades
    /// confidence if the threshold is exceeded.
    pub fn record_failure(&mut self, asset_id: &str, now_ms: i64) {
        self.history
            .entry(asset_id.to_string())
            .or_default()
            .push(OutcomeRecord {
                asset_id: asset_id.to_string(),
                success: false,
                recorded_at_ms: now_ms,
            });
        if let Some(evt) =
            Self::compute_downgrade(&self.history, &self.scores, asset_id, now_ms, &self.config)
        {
            *self
                .scores
                .entry(asset_id.to_string())
                .or_insert(evt.old_confidence) = evt.new_confidence;
            self.downgrade_log.push(evt);
        }
    }

    /// Sweep all tracked assets and apply downgrade logic at `now_ms`.
    ///
    /// Returns every [`DowngradeEvent`] generated in this sweep (also
    /// appended to the internal log).
    pub fn run_downgrade_check(&mut self, now_ms: i64) -> Vec<DowngradeEvent> {
        let asset_ids: Vec<String> = self.history.keys().cloned().collect();
        let mut events = Vec::new();
        for id in &asset_ids {
            if let Some(evt) =
                Self::compute_downgrade(&self.history, &self.scores, id, now_ms, &self.config)
            {
                *self.scores.entry(id.clone()).or_insert(evt.old_confidence) = evt.new_confidence;
                self.downgrade_log.push(evt.clone());
                events.push(evt);
            }
        }
        events
    }

    /// Full observability log of every downgrade event since construction.
    pub fn downgrade_log(&self) -> &[DowngradeEvent] {
        &self.downgrade_log
    }

    /// Asset IDs whose confidence has fallen below
    /// [`ControllerConfig::min_selectable_confidence`] and therefore require
    /// re-validation before they can be reused.
    pub fn assets_requiring_revalidation(&self) -> Vec<String> {
        self.scores
            .iter()
            .filter(|(_, &v)| v < self.config.min_selectable_confidence)
            .map(|(k, _)| k.clone())
            .collect()
    }

    // --- private helpers ---

    /// Pure function: decide whether `asset_id` should be downgraded given
    /// current `history` and `scores`.  Returns `None` when no action is
    /// needed.
    fn compute_downgrade(
        history: &HashMap<String, Vec<OutcomeRecord>>,
        scores: &HashMap<String, f32>,
        asset_id: &str,
        now_ms: i64,
        config: &ControllerConfig,
    ) -> Option<DowngradeEvent> {
        let window_start = now_ms - config.window_ms;
        let records = history.get(asset_id)?;
        let window: Vec<&OutcomeRecord> = records
            .iter()
            .filter(|r| r.recorded_at_ms >= window_start)
            .collect();
        let total = window.len();
        if total < config.min_samples {
            return None;
        }
        let failures = window.iter().filter(|r| !r.success).count();
        let rate = failures as f32 / total as f32;
        if rate < config.failure_rate_threshold {
            return None;
        }
        let old = scores
            .get(asset_id)
            .copied()
            .unwrap_or(config.initial_confidence);
        let new_val = (old - config.downgrade_penalty).max(0.0);
        Some(DowngradeEvent {
            asset_id: asset_id.to_string(),
            old_confidence: old,
            new_confidence: new_val,
            failure_rate: rate,
            window_samples: total,
            event_at_ms: now_ms,
            revalidation_required: new_val < config.min_selectable_confidence,
        })
    }
}

// ---------------------------------------------------------------------------
// BayesianConfidenceUpdater and ConfidenceSnapshot
// ---------------------------------------------------------------------------

/// A snapshot of the current Bayesian posterior for an asset's confidence.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ConfidenceSnapshot {
    /// Posterior mean: α / (α + β).
    pub mean: f32,
    /// Posterior variance: αβ / ((α+β)²(α+β+1)).
    pub variance: f32,
    /// Total observations (successes + failures) since the updater was created.
    pub sample_count: u32,
    /// `true` when `sample_count ≥ 10` and `variance < 0.01`, indicating a stable
    /// credible interval.
    pub is_stable: bool,
}

/// Per-class Beta-distribution prior.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BetaPrior {
    /// Alpha parameter (pseudo-success count).
    pub alpha: f32,
    /// Beta parameter (pseudo-failure count).
    pub beta: f32,
}

impl BetaPrior {
    pub fn new(alpha: f32, beta: f32) -> Self {
        assert!(
            alpha > 0.0 && beta > 0.0,
            "Beta distribution parameters must be positive"
        );
        Self { alpha, beta }
    }
}

/// Return the canonical built-in priors.
///
/// Encodes a weak prior leaning toward success (α=2, β=1) reflecting that
/// genes entering the pool should already have passed static validation.
pub fn builtin_priors() -> BetaPrior {
    BetaPrior::new(2.0, 1.0)
}

/// Bayesian confidence updater using a Beta-Bernoulli conjugate model.
///
/// # Model
///
/// Maintains parameters `(α, β)` of a Beta distribution.  On each observation:
/// - Success: `α += 1`
/// - Failure: `β += 1`
///
/// The posterior mean `α / (α + β)` is used as the point estimate.
pub struct BayesianConfidenceUpdater {
    alpha: f32,
    beta: f32,
}

impl BayesianConfidenceUpdater {
    /// Create an updater with an explicit prior.
    pub fn new(prior: BetaPrior) -> Self {
        Self {
            alpha: prior.alpha,
            beta: prior.beta,
        }
    }

    /// Create an updater with the `builtin_priors()` prior.
    pub fn with_builtin_prior() -> Self {
        Self::new(builtin_priors())
    }

    /// Record a success observation (`α += 1`).
    pub fn update_success(&mut self) {
        self.alpha += 1.0;
    }

    /// Record a failure observation (`β += 1`).
    pub fn update_failure(&mut self) {
        self.beta += 1.0;
    }

    /// Apply `successes` and `failures` in bulk.
    pub fn update(&mut self, successes: u32, failures: u32) {
        self.alpha += successes as f32;
        self.beta += failures as f32;
    }

    /// Posterior mean: `α / (α + β)`.
    pub fn posterior_mean(&self) -> f32 {
        self.alpha / (self.alpha + self.beta)
    }

    /// Posterior variance: `αβ / ((α+β)²(α+β+1))`.
    pub fn posterior_variance(&self) -> f32 {
        let ab = self.alpha + self.beta;
        (self.alpha * self.beta) / (ab * ab * (ab + 1.0))
    }

    /// Total observations recorded above the prior (i.e. `(α - α₀) + (β - β₀)`).
    pub fn sample_count(&self, prior: &BetaPrior) -> u32 {
        let raw = (self.alpha - prior.alpha) + (self.beta - prior.beta);
        raw.round().max(0.0) as u32
    }

    /// Build a `ConfidenceSnapshot` from the current posterior state.
    ///
    /// `prior` is used only to compute `sample_count`; pass `builtin_priors()`
    /// unless you constructed this updater with a custom prior.
    pub fn snapshot(&self, prior: &BetaPrior) -> ConfidenceSnapshot {
        let mean = self.posterior_mean();
        let variance = self.posterior_variance();
        let count = self.sample_count(prior);
        let is_stable = count >= 10 && variance < 0.01;
        ConfidenceSnapshot {
            mean,
            variance,
            sample_count: count,
            is_stable,
        }
    }

    /// Current alpha parameter (useful for serialisation/inspection).
    pub fn alpha(&self) -> f32 {
        self.alpha
    }

    /// Current beta parameter (useful for serialisation/inspection).
    pub fn beta(&self) -> f32 {
        self.beta
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_decay() {
        // Initial confidence 1.0, after 0 hours should be 1.0
        let conf = StandardConfidenceScheduler::calculate_decay(1.0, 0.0);
        assert!((conf - 1.0).abs() < 0.001);

        // After ~13.86 hours (ln(2)/0.05), confidence should be ~0.5
        let conf = StandardConfidenceScheduler::calculate_decay(1.0, 13.86);
        assert!((conf - 0.5).abs() < 0.01);

        // After 24 hours: e^(-0.05*24) ≈ 0.30
        let conf = StandardConfidenceScheduler::calculate_decay(1.0, 24.0);
        assert!((conf - 0.30).abs() < 0.02);

        // Zero confidence stays zero
        let conf = StandardConfidenceScheduler::calculate_decay(0.0, 100.0);
        assert!(conf.abs() < 0.001);
    }

    #[test]
    fn test_should_quarantine() {
        let scheduler = StandardConfidenceScheduler::with_default_config();

        // Above threshold - should not quarantine
        assert!(!scheduler.should_quarantine(0.5));
        assert!(!scheduler.should_quarantine(0.35));
        assert!(!scheduler.should_quarantine(0.36));

        // Below threshold - should quarantine
        assert!(scheduler.should_quarantine(0.34));
        assert!(scheduler.should_quarantine(0.0));
    }

    #[test]
    fn test_boost_confidence() {
        let scheduler = StandardConfidenceScheduler::with_default_config();

        // Boost from 0.5 should be 0.6
        let conf = scheduler.boost_confidence(0.5);
        assert!((conf - 0.6).abs() < 0.001);

        // Boost should cap at 1.0
        let conf = scheduler.boost_confidence(1.0);
        assert!((conf - 1.0).abs() < 0.001);

        // Boost from 0.95 should cap at 1.0
        let conf = scheduler.boost_confidence(0.95);
        assert!((conf - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_default_config() {
        let config = ConfidenceSchedulerConfig::default();
        assert!(config.enabled);
        assert_eq!(config.check_interval_secs, 3600);
        assert!((config.confidence_boost_per_success - 0.1).abs() < 0.001);
    }

    #[test]
    fn test_calculate_age_hours() {
        // 1 hour = 3600 seconds = 3600000 ms
        let age = StandardConfidenceScheduler::calculate_age_hours(0, 3600000);
        assert!((age - 1.0).abs() < 0.001);

        // 24 hours
        let age = StandardConfidenceScheduler::calculate_age_hours(0, 86400000);
        assert!((age - 24.0).abs() < 0.001);

        // Less than an hour
        let age = StandardConfidenceScheduler::calculate_age_hours(0, 1800000);
        assert!((age - 0.5).abs() < 0.001);
    }

    // -----------------------------------------------------------------------
    // ConfidenceController tests
    // -----------------------------------------------------------------------

    const NOW: i64 = 1_000_000_000_000; // arbitrary fixed "now" in ms
    const WINDOW: i64 = 3_600_000; // 1 hour window

    fn controller_with_3_samples() -> ConfidenceController {
        ConfidenceController::new(ControllerConfig {
            window_ms: WINDOW,
            failure_rate_threshold: 0.5,
            min_samples: 3,
            downgrade_penalty: 0.15,
            min_selectable_confidence: MIN_REPLAY_CONFIDENCE,
            initial_confidence: 1.0,
        })
    }

    #[test]
    fn test_controller_initial_confidence_is_one() {
        let ctrl = controller_with_3_samples();
        // Unknown asset → initial confidence
        assert!((ctrl.confidence("gene-1") - 1.0).abs() < 0.001);
        assert!(ctrl.is_selectable("gene-1"));
    }

    #[test]
    fn test_controller_successive_failures_downgrade() {
        let mut ctrl = controller_with_3_samples();
        // 3 failures in the window → failure rate 1.0 ≥ 0.5 → first downgrade
        ctrl.record_failure("gene-x", NOW);
        ctrl.record_failure("gene-x", NOW + 1);
        ctrl.record_failure("gene-x", NOW + 2);
        let c = ctrl.confidence("gene-x");
        // Expected: 1.0 - 0.15 = 0.85
        assert!((c - 0.85).abs() < 0.01, "expected ~0.85, got {c}");
        assert_eq!(ctrl.downgrade_log().len(), 1);
    }

    #[test]
    fn test_controller_below_min_not_selectable() {
        let mut ctrl = ConfidenceController::new(ControllerConfig {
            window_ms: WINDOW,
            failure_rate_threshold: 0.5,
            min_samples: 2,
            downgrade_penalty: 0.35,
            min_selectable_confidence: MIN_REPLAY_CONFIDENCE,
            initial_confidence: 0.5, // start near the edge
        });
        // Two failures → rate 1.0 ≥ 0.5, 2 ≥ min_samples=2 → downgrade
        ctrl.record_failure("gene-low", NOW);
        ctrl.record_failure("gene-low", NOW + 1);
        // 0.5 - 0.35 = 0.15 < MIN_REPLAY_CONFIDENCE (0.35)
        assert!(!ctrl.is_selectable("gene-low"));
        assert_eq!(ctrl.downgrade_log()[0].revalidation_required, true);
        let rv = ctrl.assets_requiring_revalidation();
        assert!(rv.contains(&"gene-low".to_string()));
    }

    #[test]
    fn test_controller_recovery_via_successes() {
        let mut ctrl = controller_with_3_samples();
        // Drive confidence down first
        ctrl.record_failure("gene-r", NOW);
        ctrl.record_failure("gene-r", NOW + 1);
        ctrl.record_failure("gene-r", NOW + 2);
        let after_failures = ctrl.confidence("gene-r");
        // Now record two successes to partially recover
        ctrl.record_success("gene-r", NOW + 3);
        ctrl.record_success("gene-r", NOW + 4);
        let after_recovery = ctrl.confidence("gene-r");
        assert!(
            after_recovery > after_failures,
            "recovery expected: {after_recovery} > {after_failures}"
        );
    }

    #[test]
    fn test_controller_no_downgrade_below_min_samples() {
        let mut ctrl = controller_with_3_samples();
        // Only 2 failures — below min_samples=3 → no downgrade
        ctrl.record_failure("gene-few", NOW);
        ctrl.record_failure("gene-few", NOW + 1);
        assert!((ctrl.confidence("gene-few") - 1.0).abs() < 0.001);
        assert!(ctrl.downgrade_log().is_empty());
    }

    #[test]
    fn test_controller_failures_outside_window_ignored() {
        let mut ctrl = controller_with_3_samples();
        // 2 failures far outside the 1-hour window (below min_samples=3 so no
        // downgrade fires when they are recorded).
        let old = NOW - WINDOW - 1;
        ctrl.record_failure("gene-old", old);
        ctrl.record_failure("gene-old", old + 1);
        // run_downgrade_check at NOW: these 2 records are outside the window,
        // count = 0 → below min_samples → no new downgrade event.
        let events = ctrl.run_downgrade_check(NOW);
        assert!(events.is_empty(), "expected no downgrade, got {events:?}");
        assert!((ctrl.confidence("gene-old") - 1.0).abs() < 0.001);
        assert!(ctrl.downgrade_log().is_empty());
    }

    #[test]
    fn test_controller_run_downgrade_check_batch() {
        let mut ctrl = controller_with_3_samples();
        // Seed failures for two assets
        for i in 0..3 {
            ctrl.history
                .entry("asset-a".to_string())
                .or_default()
                .push(OutcomeRecord {
                    asset_id: "asset-a".to_string(),
                    success: false,
                    recorded_at_ms: NOW + i,
                });
            ctrl.history
                .entry("asset-b".to_string())
                .or_default()
                .push(OutcomeRecord {
                    asset_id: "asset-b".to_string(),
                    success: false,
                    recorded_at_ms: NOW + i,
                });
        }
        let events = ctrl.run_downgrade_check(NOW + 10);
        assert_eq!(events.len(), 2);
        assert_eq!(ctrl.downgrade_log().len(), 2);
    }

    #[test]
    fn test_controller_downgrade_event_fields() {
        let mut ctrl = controller_with_3_samples();
        ctrl.record_failure("gene-fields", NOW);
        ctrl.record_failure("gene-fields", NOW + 1);
        ctrl.record_failure("gene-fields", NOW + 2);
        let log = ctrl.downgrade_log();
        assert_eq!(log.len(), 1);
        let evt = &log[0];
        assert_eq!(evt.asset_id, "gene-fields");
        assert!((evt.old_confidence - 1.0).abs() < 0.001);
        assert!((evt.new_confidence - 0.85).abs() < 0.01);
        assert!((evt.failure_rate - 1.0).abs() < 0.001);
        assert_eq!(evt.window_samples, 3);
        assert_eq!(evt.event_at_ms, NOW + 2);
    }

    // -----------------------------------------------------------------------
    // BayesianConfidenceUpdater tests
    // -----------------------------------------------------------------------

    #[test]
    fn bayesian_updater_prior_mean() {
        // builtin_priors: α=2, β=1 → mean = 2/3 ≈ 0.667
        let updater = BayesianConfidenceUpdater::with_builtin_prior();
        let mean = updater.posterior_mean();
        assert!((mean - 2.0 / 3.0).abs() < 0.001, "mean={mean}");
    }

    #[test]
    fn bayesian_updater_converges_to_true_rate() {
        // 100 observations at 70% success-rate; posterior mean should approach 0.70.
        let mut updater = BayesianConfidenceUpdater::with_builtin_prior();
        updater.update(70, 30);
        let mean = updater.posterior_mean();
        assert!(
            (mean - 0.70).abs() < 0.02,
            "expected mean ≈ 0.70, got {mean}"
        );
    }

    #[test]
    fn bayesian_updater_sample_count() {
        let prior = builtin_priors();
        let mut updater = BayesianConfidenceUpdater::with_builtin_prior();
        updater.update(5, 5);
        assert_eq!(updater.sample_count(&prior), 10);
    }

    #[test]
    fn bayesian_updater_snapshot_is_stable_after_observations() {
        let prior = builtin_priors();
        let mut updater = BayesianConfidenceUpdater::with_builtin_prior();
        // 50 successes, 50 failures → balanced → variance should be small after many samples
        updater.update(50, 50);
        let snap = updater.snapshot(&prior);
        assert_eq!(snap.sample_count, 100);
        // variance = αβ/((α+β)²(α+β+1)); with α=52,β=51 → very small
        assert!(snap.is_stable, "should be stable with 100 samples");
    }

    #[test]
    fn bayesian_updater_sequential_updates_equal_bulk() {
        let mut seq = BayesianConfidenceUpdater::with_builtin_prior();
        for _ in 0..7 {
            seq.update_success();
        }
        for _ in 0..3 {
            seq.update_failure();
        }

        let mut bulk = BayesianConfidenceUpdater::with_builtin_prior();
        bulk.update(7, 3);

        assert!((seq.posterior_mean() - bulk.posterior_mean()).abs() < 1e-6);
        assert!((seq.posterior_variance() - bulk.posterior_variance()).abs() < 1e-9);
    }
}
