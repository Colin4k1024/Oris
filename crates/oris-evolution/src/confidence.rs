//! Confidence Lifecycle Scheduler
//!
//! This module implements automatic confidence decay and lifecycle management
//! for genes and capsules within the evolution crate.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::core::{
    AssetState, GeneId, MIN_REPLAY_CONFIDENCE, REPLAY_CONFIDENCE_DECAY_RATE_PER_HOUR,
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

// ---------------------------------------------------------------------------
// Multi-dimensional Confidence Profile (Issue #384)
// ---------------------------------------------------------------------------

/// Freshness dimension: how recently the asset was validated.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct FreshnessDimension {
    /// Last validation timestamp in milliseconds since epoch.
    pub last_validated_ms: i64,
    /// Freshness score in [0.0, 1.0], decays over time.
    pub score: f32,
    /// Maximum age in hours before the asset is considered stale.
    pub max_age_hours: f32,
}

impl Default for FreshnessDimension {
    fn default() -> Self {
        Self {
            last_validated_ms: 0,
            score: 1.0,
            max_age_hours: 24.0,
        }
    }
}

impl FreshnessDimension {
    /// Recompute freshness score based on current time.
    pub fn refresh_score(&mut self, now_ms: i64) {
        let age_hours = (now_ms - self.last_validated_ms) as f32 / 3_600_000.0;
        self.score = if age_hours <= 0.0 {
            1.0
        } else {
            (-REPLAY_CONFIDENCE_DECAY_RATE_PER_HOUR * age_hours)
                .exp()
                .clamp(0.0, 1.0)
        };
    }

    /// Returns true if the asset is stale (age exceeds max_age_hours).
    pub fn is_stale(&self, now_ms: i64) -> bool {
        let age_hours = (now_ms - self.last_validated_ms) as f32 / 3_600_000.0;
        age_hours > self.max_age_hours
    }

    /// Record a fresh validation.
    pub fn record_validation(&mut self, now_ms: i64) {
        self.last_validated_ms = now_ms;
        self.score = 1.0;
    }
}

/// Compatibility dimension: how well the asset matches the current environment.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CompatibilityDimension {
    /// Environment fingerprint hash at last validation.
    pub validated_env_hash: String,
    /// Compatibility score in [0.0, 1.0].
    pub score: f32,
    /// Number of environment changes detected since last validation.
    pub drift_count: u32,
    /// Maximum tolerated drift count before requiring revalidation.
    pub max_drift_tolerance: u32,
}

impl Default for CompatibilityDimension {
    fn default() -> Self {
        Self {
            validated_env_hash: String::new(),
            score: 1.0,
            drift_count: 0,
            max_drift_tolerance: 3,
        }
    }
}

impl CompatibilityDimension {
    /// Record a detected environment drift event, reducing compatibility.
    pub fn record_drift(&mut self) {
        self.drift_count += 1;
        let penalty = self.drift_count as f32 * 0.15;
        self.score = (1.0 - penalty).clamp(0.0, 1.0);
    }

    /// Reset compatibility after revalidation in the current environment.
    pub fn record_revalidation(&mut self, env_hash: String) {
        self.validated_env_hash = env_hash;
        self.score = 1.0;
        self.drift_count = 0;
    }

    /// Returns true if drift exceeds tolerance.
    pub fn exceeds_drift_tolerance(&self) -> bool {
        self.drift_count > self.max_drift_tolerance
    }
}

/// Reuse evidence dimension: track record of successful reuse.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ReuseEvidenceDimension {
    /// Total successful reuses.
    pub success_count: u32,
    /// Total failed reuses.
    pub failure_count: u32,
    /// Reuse score in [0.0, 1.0] based on success rate.
    pub score: f32,
}

impl Default for ReuseEvidenceDimension {
    fn default() -> Self {
        Self {
            success_count: 0,
            failure_count: 0,
            score: 0.667, // Prior: 2/(2+1) matching Bayesian prior
        }
    }
}

impl ReuseEvidenceDimension {
    /// Record a successful reuse.
    pub fn record_success(&mut self) {
        self.success_count += 1;
        self.recompute_score();
    }

    /// Record a failed reuse.
    pub fn record_failure(&mut self) {
        self.failure_count += 1;
        self.recompute_score();
    }

    fn recompute_score(&mut self) {
        // Beta-Bernoulli with prior alpha=2, beta=1
        let alpha = 2.0 + self.success_count as f32;
        let beta = 1.0 + self.failure_count as f32;
        self.score = alpha / (alpha + beta);
    }

    /// Total observations.
    pub fn total_observations(&self) -> u32 {
        self.success_count + self.failure_count
    }
}

/// Thresholds for replay admissibility based on confidence profile.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ReplayAdmissibilityThresholds {
    /// Minimum freshness score for replay eligibility.
    pub min_freshness: f32,
    /// Minimum compatibility score for replay eligibility.
    pub min_compatibility: f32,
    /// Minimum reuse evidence score for replay eligibility.
    pub min_reuse_evidence: f32,
    /// Minimum composite score for replay eligibility.
    pub min_composite: f32,
}

impl Default for ReplayAdmissibilityThresholds {
    fn default() -> Self {
        Self {
            min_freshness: MIN_REPLAY_CONFIDENCE,
            min_compatibility: 0.5,
            min_reuse_evidence: 0.3,
            min_composite: MIN_REPLAY_CONFIDENCE,
        }
    }
}

/// Lifecycle state of a confidence profile.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ConfidenceLifecycleState {
    /// Newly created, not yet fully evaluated.
    Initial,
    /// Actively maintained with recent evidence.
    Active,
    /// Freshness or compatibility is degrading.
    Degrading,
    /// Requires revalidation before replay.
    NeedsRevalidation,
    /// Demoted due to failures or drift.
    Demoted,
    /// Fully revoked, not eligible for any use.
    Revoked,
}

impl Default for ConfidenceLifecycleState {
    fn default() -> Self {
        Self::Initial
    }
}

/// Multi-dimensional confidence profile that models confidence as a lifecycle.
///
/// Instead of a single f32 score, confidence is decomposed into three
/// orthogonal dimensions: freshness (time-based decay), compatibility
/// (environment drift), and reuse evidence (historical success rate).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ConfidenceProfile {
    /// Asset identifier this profile belongs to.
    pub asset_id: String,
    /// Freshness dimension.
    pub freshness: FreshnessDimension,
    /// Compatibility dimension.
    pub compatibility: CompatibilityDimension,
    /// Reuse evidence dimension.
    pub reuse_evidence: ReuseEvidenceDimension,
    /// Lifecycle state derived from dimension scores.
    pub lifecycle_state: ConfidenceLifecycleState,
    /// Thresholds for replay admissibility.
    pub thresholds: ReplayAdmissibilityThresholds,
    /// Creation timestamp in milliseconds.
    pub created_at_ms: i64,
    /// Last update timestamp in milliseconds.
    pub updated_at_ms: i64,
}

impl ConfidenceProfile {
    /// Create a new profile for an asset.
    pub fn new(asset_id: impl Into<String>, now_ms: i64) -> Self {
        let mut freshness = FreshnessDimension::default();
        freshness.last_validated_ms = now_ms;
        Self {
            asset_id: asset_id.into(),
            freshness,
            compatibility: CompatibilityDimension::default(),
            reuse_evidence: ReuseEvidenceDimension::default(),
            lifecycle_state: ConfidenceLifecycleState::Initial,
            thresholds: ReplayAdmissibilityThresholds::default(),
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
        }
    }

    /// Compute the composite confidence score (weighted mean of dimensions).
    pub fn composite_score(&self) -> f32 {
        let score = 0.4 * self.freshness.score
            + 0.35 * self.compatibility.score
            + 0.25 * self.reuse_evidence.score;
        score.clamp(0.0, 1.0)
    }

    /// Check if this asset is eligible for replay.
    pub fn is_replay_eligible(&self) -> bool {
        self.freshness.score >= self.thresholds.min_freshness
            && self.compatibility.score >= self.thresholds.min_compatibility
            && self.reuse_evidence.score >= self.thresholds.min_reuse_evidence
            && self.composite_score() >= self.thresholds.min_composite
            && !matches!(
                self.lifecycle_state,
                ConfidenceLifecycleState::Demoted
                    | ConfidenceLifecycleState::Revoked
                    | ConfidenceLifecycleState::NeedsRevalidation
            )
    }

    /// Update the lifecycle state based on current dimension scores.
    pub fn update_lifecycle_state(&mut self, now_ms: i64) {
        self.freshness.refresh_score(now_ms);
        self.updated_at_ms = now_ms;

        if matches!(self.lifecycle_state, ConfidenceLifecycleState::Revoked) {
            return; // Revoked is terminal unless explicitly reinstated
        }

        if self.compatibility.exceeds_drift_tolerance() {
            self.lifecycle_state = ConfidenceLifecycleState::NeedsRevalidation;
        } else if self.freshness.is_stale(now_ms) {
            self.lifecycle_state = ConfidenceLifecycleState::NeedsRevalidation;
        } else if self.freshness.score < self.thresholds.min_freshness
            || self.compatibility.score < self.thresholds.min_compatibility
        {
            self.lifecycle_state = ConfidenceLifecycleState::Degrading;
        } else if self.reuse_evidence.total_observations() == 0 {
            self.lifecycle_state = ConfidenceLifecycleState::Initial;
        } else {
            self.lifecycle_state = ConfidenceLifecycleState::Active;
        }
    }

    /// Demote this profile (e.g., after repeated failures).
    pub fn demote(&mut self, now_ms: i64) {
        self.lifecycle_state = ConfidenceLifecycleState::Demoted;
        self.updated_at_ms = now_ms;
    }

    /// Revoke this profile permanently.
    pub fn revoke(&mut self, now_ms: i64) {
        self.lifecycle_state = ConfidenceLifecycleState::Revoked;
        self.updated_at_ms = now_ms;
    }
}

// ---------------------------------------------------------------------------
// Decay Rules (Issue #385)
// ---------------------------------------------------------------------------

/// Configuration for multi-dimensional decay rules.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DecayRulesConfig {
    /// Freshness decay rate per hour (exponential).
    pub freshness_decay_rate_per_hour: f32,
    /// Compatibility penalty per drift event.
    pub drift_penalty_per_event: f32,
    /// Minimum composite confidence for promotion eligibility.
    pub min_promotion_confidence: f32,
    /// Minimum composite confidence for replay eligibility.
    pub min_replay_confidence: f32,
    /// Hours after which an asset is considered stale.
    pub stale_threshold_hours: f32,
    /// Enable automatic demotion when composite drops below threshold.
    pub auto_demote_enabled: bool,
}

impl Default for DecayRulesConfig {
    fn default() -> Self {
        Self {
            freshness_decay_rate_per_hour: REPLAY_CONFIDENCE_DECAY_RATE_PER_HOUR,
            drift_penalty_per_event: 0.15,
            min_promotion_confidence: 0.6,
            min_replay_confidence: MIN_REPLAY_CONFIDENCE,
            stale_threshold_hours: 24.0,
            auto_demote_enabled: true,
        }
    }
}

/// Result of applying decay rules to a confidence profile.
#[derive(Clone, Debug)]
pub struct DecayResult {
    /// Previous composite score.
    pub previous_composite: f32,
    /// New composite score.
    pub new_composite: f32,
    /// Whether the asset was demoted by this decay pass.
    pub demoted: bool,
    /// Whether the asset now requires revalidation.
    pub needs_revalidation: bool,
    /// Individual dimension changes.
    pub freshness_delta: f32,
    pub compatibility_delta: f32,
}

/// Apply decay rules to a confidence profile based on elapsed time and drift events.
///
/// This function updates the profile's freshness dimension based on time elapsed,
/// applies drift penalties, and transitions the lifecycle state as needed.
pub fn apply_decay_rules(
    profile: &mut ConfidenceProfile,
    now_ms: i64,
    drift_events: u32,
    config: &DecayRulesConfig,
) -> DecayResult {
    let previous_composite = profile.composite_score();
    let old_freshness = profile.freshness.score;
    let old_compatibility = profile.compatibility.score;

    // Apply freshness decay
    profile.freshness.refresh_score(now_ms);

    // Apply drift penalties
    for _ in 0..drift_events {
        profile.compatibility.record_drift();
    }

    // Update lifecycle state
    profile.update_lifecycle_state(now_ms);

    let new_composite = profile.composite_score();

    // Auto-demotion check
    let demoted = config.auto_demote_enabled
        && new_composite < config.min_replay_confidence
        && !matches!(
            profile.lifecycle_state,
            ConfidenceLifecycleState::Demoted | ConfidenceLifecycleState::Revoked
        );

    if demoted {
        profile.demote(now_ms);
    }

    let needs_revalidation = matches!(
        profile.lifecycle_state,
        ConfidenceLifecycleState::NeedsRevalidation
    );

    DecayResult {
        previous_composite,
        new_composite,
        demoted,
        needs_revalidation,
        freshness_delta: profile.freshness.score - old_freshness,
        compatibility_delta: profile.compatibility.score - old_compatibility,
    }
}

/// Batch-process decay for multiple profiles.
pub fn apply_decay_batch(
    profiles: &mut [ConfidenceProfile],
    now_ms: i64,
    drift_events_per_asset: &HashMap<String, u32>,
    config: &DecayRulesConfig,
) -> Vec<(String, DecayResult)> {
    profiles
        .iter_mut()
        .map(|p| {
            let drift = drift_events_per_asset
                .get(&p.asset_id)
                .copied()
                .unwrap_or(0);
            let result = apply_decay_rules(p, now_ms, drift, config);
            (p.asset_id.clone(), result)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Drift-Aware Trust Degradation (Issue #387)
// ---------------------------------------------------------------------------

/// Categories of environment drift that affect replay trust.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum DriftSignalKind {
    /// Rust compiler version changed.
    CompilerVersionChange,
    /// Cargo.lock dependency graph changed.
    DependencyChange,
    /// Target triple changed.
    TargetTripleChange,
    /// OS version changed.
    OsChange,
    /// Custom drift signal.
    Custom(String),
}

/// A single drift signal detected from the environment.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DriftSignal {
    /// Kind of drift detected.
    pub kind: DriftSignalKind,
    /// Old value (for diagnostics).
    pub old_value: String,
    /// New value (for diagnostics).
    pub new_value: String,
    /// Severity weight in [0.0, 1.0]. Higher = more impactful.
    pub severity: f32,
    /// Timestamp when drift was detected (ms).
    pub detected_at_ms: i64,
}

/// Configuration for drift-aware trust degradation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DriftDegradationConfig {
    /// Base penalty per drift event (multiplied by severity).
    pub base_penalty: f32,
    /// Maximum cumulative drift penalty before forced revalidation.
    pub max_cumulative_penalty: f32,
    /// Whether compiler version changes force immediate revalidation.
    pub compiler_change_forces_revalidation: bool,
}

impl Default for DriftDegradationConfig {
    fn default() -> Self {
        Self {
            base_penalty: 0.15,
            max_cumulative_penalty: 0.6,
            compiler_change_forces_revalidation: true,
        }
    }
}

/// Result of applying drift signals to a profile.
#[derive(Clone, Debug)]
pub struct DriftDegradationResult {
    /// Total penalty applied.
    pub total_penalty: f32,
    /// Whether forced revalidation was triggered.
    pub forced_revalidation: bool,
    /// Number of drift signals processed.
    pub signals_processed: u32,
    /// Previous composite score.
    pub previous_composite: f32,
    /// New composite score.
    pub new_composite: f32,
}

/// Apply drift signals to a confidence profile, degrading trust proportionally.
pub fn apply_drift_degradation(
    profile: &mut ConfidenceProfile,
    signals: &[DriftSignal],
    now_ms: i64,
    config: &DriftDegradationConfig,
) -> DriftDegradationResult {
    let previous_composite = profile.composite_score();
    let mut total_penalty: f32 = 0.0;
    let mut forced_revalidation = false;

    for signal in signals {
        let penalty = config.base_penalty * signal.severity;
        total_penalty += penalty;
        profile.compatibility.record_drift();

        if config.compiler_change_forces_revalidation
            && signal.kind == DriftSignalKind::CompilerVersionChange
        {
            forced_revalidation = true;
        }
    }

    total_penalty = total_penalty.min(config.max_cumulative_penalty);

    if forced_revalidation || total_penalty >= config.max_cumulative_penalty {
        profile.lifecycle_state = ConfidenceLifecycleState::NeedsRevalidation;
        forced_revalidation = true;
        // Only refresh freshness score, don't override lifecycle state
        profile.freshness.refresh_score(now_ms);
        profile.updated_at_ms = now_ms;
    } else {
        profile.update_lifecycle_state(now_ms);
    }

    DriftDegradationResult {
        total_penalty,
        forced_revalidation,
        signals_processed: signals.len() as u32,
        previous_composite,
        new_composite: profile.composite_score(),
    }
}

/// Compare two environment fingerprint hashes and produce drift signals.
pub fn detect_drift_signals(
    old_env_hash: &str,
    new_env_hash: &str,
    old_components: &[(&str, &str)],
    new_components: &[(&str, &str)],
    now_ms: i64,
) -> Vec<DriftSignal> {
    if old_env_hash == new_env_hash {
        return Vec::new();
    }

    let old_map: HashMap<&str, &str> = old_components.iter().copied().collect();
    let new_map: HashMap<&str, &str> = new_components.iter().copied().collect();

    let mut signals = Vec::new();

    for (key, new_val) in &new_map {
        if let Some(old_val) = old_map.get(key) {
            if old_val != new_val {
                let (kind, severity) = match *key {
                    "rustc_version" => (DriftSignalKind::CompilerVersionChange, 0.9),
                    "cargo_lock_hash" => (DriftSignalKind::DependencyChange, 0.6),
                    "target_triple" => (DriftSignalKind::TargetTripleChange, 0.8),
                    "os" => (DriftSignalKind::OsChange, 0.5),
                    other => (DriftSignalKind::Custom(other.to_string()), 0.4),
                };
                signals.push(DriftSignal {
                    kind,
                    old_value: old_val.to_string(),
                    new_value: new_val.to_string(),
                    severity,
                    detected_at_ms: now_ms,
                });
            }
        }
    }

    signals
}

// ---------------------------------------------------------------------------
// Shadow Revalidation Scheduling (Issue #386)
// ---------------------------------------------------------------------------

/// Priority class for revalidation scheduling.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum RevalidationPriority {
    /// Low priority: asset is still above thresholds but aging.
    Low,
    /// Medium priority: asset is degrading.
    Medium,
    /// High priority: asset is below threshold or has high reuse count.
    High,
    /// Critical: asset was demoted and needs immediate revalidation.
    Critical,
}

/// A scheduled revalidation target.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RevalidationTarget {
    /// Asset identifier.
    pub asset_id: String,
    /// Priority of this revalidation.
    pub priority: RevalidationPriority,
    /// Current composite confidence score.
    pub current_composite: f32,
    /// Whether the freshness dimension is stale.
    pub freshness_stale: bool,
    /// Whether compatibility drift exceeds tolerance.
    pub drift_exceeded: bool,
    /// Total reuse count (higher reuse = higher value asset).
    pub reuse_count: u32,
    /// Scheduled revalidation timestamp (ms). 0 means ASAP.
    pub scheduled_at_ms: i64,
}

/// Configuration for the revalidation scheduler.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RevalidationSchedulerConfig {
    /// Maximum number of concurrent revalidations.
    pub max_concurrent: usize,
    /// Interval between revalidation sweeps in seconds.
    pub sweep_interval_secs: u64,
    /// High-value threshold: assets with reuse_count above this get priority boost.
    pub high_value_reuse_threshold: u32,
    /// Confidence threshold below which revalidation is scheduled.
    pub revalidation_confidence_threshold: f32,
}

impl Default for RevalidationSchedulerConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 5,
            sweep_interval_secs: 300,
            high_value_reuse_threshold: 10,
            revalidation_confidence_threshold: 0.5,
        }
    }
}

/// Outcome of a shadow revalidation attempt.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ShadowRevalidationOutcome {
    /// Asset passed revalidation, confidence restored.
    Passed,
    /// Asset failed revalidation, confidence reduced further.
    Failed,
    /// Revalidation was skipped (e.g., asset was revoked).
    Skipped,
}

/// Result of processing a revalidation.
#[derive(Clone, Debug)]
pub struct RevalidationResult {
    pub asset_id: String,
    pub outcome: ShadowRevalidationOutcome,
    pub old_composite: f32,
    pub new_composite: f32,
}

/// Scan profiles and produce a priority-sorted list of revalidation targets.
pub fn schedule_revalidations(
    profiles: &[ConfidenceProfile],
    now_ms: i64,
    config: &RevalidationSchedulerConfig,
) -> Vec<RevalidationTarget> {
    let mut targets: Vec<RevalidationTarget> = profiles
        .iter()
        .filter(|p| {
            !matches!(
                p.lifecycle_state,
                ConfidenceLifecycleState::Revoked | ConfidenceLifecycleState::Active
            )
        })
        .filter(|p| {
            matches!(
                p.lifecycle_state,
                ConfidenceLifecycleState::Demoted | ConfidenceLifecycleState::NeedsRevalidation
            ) || p.composite_score() < config.revalidation_confidence_threshold
        })
        .map(|p| {
            let reuse_count = p.reuse_evidence.total_observations();
            let priority = compute_revalidation_priority(p, config);
            RevalidationTarget {
                asset_id: p.asset_id.clone(),
                priority,
                current_composite: p.composite_score(),
                freshness_stale: p.freshness.is_stale(now_ms),
                drift_exceeded: p.compatibility.exceeds_drift_tolerance(),
                reuse_count,
                scheduled_at_ms: 0, // ASAP
            }
        })
        .collect();

    // Sort by priority (Critical first), then by composite (lowest first)
    targets.sort_by(|a, b| {
        b.priority.cmp(&a.priority).then_with(|| {
            a.current_composite
                .partial_cmp(&b.current_composite)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    });

    // Limit to max_concurrent
    targets.truncate(config.max_concurrent);
    targets
}

fn compute_revalidation_priority(
    profile: &ConfidenceProfile,
    config: &RevalidationSchedulerConfig,
) -> RevalidationPriority {
    if matches!(profile.lifecycle_state, ConfidenceLifecycleState::Demoted) {
        return RevalidationPriority::Critical;
    }
    if profile.composite_score() < MIN_REPLAY_CONFIDENCE {
        return RevalidationPriority::High;
    }
    if profile.reuse_evidence.total_observations() >= config.high_value_reuse_threshold {
        return RevalidationPriority::High;
    }
    if matches!(
        profile.lifecycle_state,
        ConfidenceLifecycleState::NeedsRevalidation
    ) {
        return RevalidationPriority::Medium;
    }
    RevalidationPriority::Low
}

/// Apply a revalidation outcome to a profile, adjusting confidence.
pub fn apply_revalidation_outcome(
    profile: &mut ConfidenceProfile,
    outcome: ShadowRevalidationOutcome,
    now_ms: i64,
    env_hash: Option<String>,
) -> RevalidationResult {
    let old_composite = profile.composite_score();

    match outcome {
        ShadowRevalidationOutcome::Passed => {
            profile.freshness.record_validation(now_ms);
            if let Some(hash) = env_hash {
                profile.compatibility.record_revalidation(hash);
            }
            profile.reuse_evidence.record_success();
            profile.update_lifecycle_state(now_ms);
        }
        ShadowRevalidationOutcome::Failed => {
            profile.reuse_evidence.record_failure();
            profile.update_lifecycle_state(now_ms);
            // If already degrading and fails, demote
            if profile.composite_score() < MIN_REPLAY_CONFIDENCE {
                profile.demote(now_ms);
            }
        }
        ShadowRevalidationOutcome::Skipped => {}
    }

    RevalidationResult {
        asset_id: profile.asset_id.clone(),
        outcome,
        old_composite,
        new_composite: profile.composite_score(),
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

    // -----------------------------------------------------------------------
    // ConfidenceProfile tests (Issue #384)
    // -----------------------------------------------------------------------

    #[test]
    fn confidence_profile_new_has_initial_state() {
        let now = 1_700_000_000_000i64;
        let profile = ConfidenceProfile::new("asset-1", now);
        assert_eq!(profile.lifecycle_state, ConfidenceLifecycleState::Initial);
        assert!(profile.is_replay_eligible());
        assert!(profile.composite_score() > 0.8);
    }

    #[test]
    fn confidence_profile_freshness_decay() {
        let now = 1_700_000_000_000i64;
        let mut profile = ConfidenceProfile::new("asset-2", now);
        let later = now + 25 * 3_600_000;
        profile.update_lifecycle_state(later);
        assert_eq!(
            profile.lifecycle_state,
            ConfidenceLifecycleState::NeedsRevalidation
        );
    }

    #[test]
    fn confidence_profile_drift_triggers_revalidation() {
        let now = 1_700_000_000_000i64;
        let mut profile = ConfidenceProfile::new("asset-3", now);
        for _ in 0..4 {
            profile.compatibility.record_drift();
        }
        profile.update_lifecycle_state(now + 1000);
        assert_eq!(
            profile.lifecycle_state,
            ConfidenceLifecycleState::NeedsRevalidation
        );
    }

    #[test]
    fn confidence_profile_reuse_evidence_updates() {
        let mut reuse = ReuseEvidenceDimension::default();
        reuse.record_success();
        reuse.record_success();
        reuse.record_failure();
        assert!((reuse.score - 0.667).abs() < 0.01);
        assert_eq!(reuse.total_observations(), 3);
    }

    #[test]
    fn confidence_profile_composite_score_weighted() {
        let now = 1_700_000_000_000i64;
        let mut profile = ConfidenceProfile::new("asset-4", now);
        profile.freshness.score = 0.8;
        profile.compatibility.score = 0.6;
        profile.reuse_evidence.score = 0.5;
        assert!((profile.composite_score() - 0.655).abs() < 0.01);
    }

    #[test]
    fn confidence_profile_demote_and_revoke() {
        let now = 1_700_000_000_000i64;
        let mut profile = ConfidenceProfile::new("asset-5", now);
        profile.demote(now + 1000);
        assert_eq!(profile.lifecycle_state, ConfidenceLifecycleState::Demoted);
        assert!(!profile.is_replay_eligible());

        profile.revoke(now + 2000);
        assert_eq!(profile.lifecycle_state, ConfidenceLifecycleState::Revoked);
        profile.update_lifecycle_state(now + 3000);
        assert_eq!(profile.lifecycle_state, ConfidenceLifecycleState::Revoked);
    }

    #[test]
    fn compatibility_revalidation_resets_drift() {
        let mut compat = CompatibilityDimension::default();
        compat.record_drift();
        compat.record_drift();
        assert_eq!(compat.drift_count, 2);
        assert!(compat.score < 1.0);
        compat.record_revalidation("new-env-hash".to_string());
        assert_eq!(compat.drift_count, 0);
        assert!((compat.score - 1.0).abs() < 0.001);
    }

    #[test]
    fn replay_admissibility_thresholds_gate_correctly() {
        let now = 1_700_000_000_000i64;
        let mut profile = ConfidenceProfile::new("asset-6", now);
        profile.compatibility.score = 0.3;
        assert!(!profile.is_replay_eligible());
    }

    // -----------------------------------------------------------------------
    // Decay Rules tests (Issue #385)
    // -----------------------------------------------------------------------

    #[test]
    fn decay_rules_freshness_decays_over_time() {
        let now = 1_700_000_000_000i64;
        let mut profile = ConfidenceProfile::new("decay-1", now);
        let config = DecayRulesConfig::default();
        let later = now + 10 * 3_600_000; // 10 hours later
        let result = apply_decay_rules(&mut profile, later, 0, &config);
        assert!(result.freshness_delta < 0.0);
        assert!(result.new_composite < result.previous_composite);
    }

    #[test]
    fn decay_rules_drift_events_reduce_compatibility() {
        let now = 1_700_000_000_000i64;
        let mut profile = ConfidenceProfile::new("decay-2", now);
        let config = DecayRulesConfig::default();
        let result = apply_decay_rules(&mut profile, now + 1000, 2, &config);
        assert!(result.compatibility_delta < 0.0);
        assert_eq!(profile.compatibility.drift_count, 2);
    }

    #[test]
    fn decay_rules_auto_demotion_below_threshold() {
        let now = 1_700_000_000_000i64;
        let mut profile = ConfidenceProfile::new("decay-3", now);
        let config = DecayRulesConfig::default();
        // Fast-forward 48 hours so freshness decays heavily
        let far_future = now + 48 * 3_600_000;
        let result = apply_decay_rules(&mut profile, far_future, 5, &config);
        assert!(result.demoted || result.needs_revalidation);
    }

    #[test]
    fn decay_rules_batch_processing() {
        let now = 1_700_000_000_000i64;
        let mut profiles = vec![
            ConfidenceProfile::new("batch-1", now),
            ConfidenceProfile::new("batch-2", now),
        ];
        let mut drift_map = HashMap::new();
        drift_map.insert("batch-1".to_string(), 1u32);
        drift_map.insert("batch-2".to_string(), 0u32);
        let config = DecayRulesConfig::default();
        let results = apply_decay_batch(&mut profiles, now + 3_600_000, &drift_map, &config);
        assert_eq!(results.len(), 2);
        // batch-1 had drift, should have lower compatibility
        assert!(profiles[0].compatibility.score < profiles[1].compatibility.score);
    }

    #[test]
    fn decay_rules_no_demotion_when_disabled() {
        let now = 1_700_000_000_000i64;
        let mut profile = ConfidenceProfile::new("decay-4", now);
        let config = DecayRulesConfig {
            auto_demote_enabled: false,
            ..DecayRulesConfig::default()
        };
        let far_future = now + 48 * 3_600_000;
        let result = apply_decay_rules(&mut profile, far_future, 5, &config);
        assert!(!result.demoted);
    }

    // -----------------------------------------------------------------------
    // Revalidation Scheduling tests (Issue #386)
    // -----------------------------------------------------------------------

    #[test]
    fn revalidation_schedules_degrading_profiles() {
        let now = 1_700_000_000_000i64;
        let mut profile = ConfidenceProfile::new("reval-1", now);
        // Make it degrading
        profile.freshness.score = 0.3;
        profile.compatibility.score = 0.4;
        profile.lifecycle_state = ConfidenceLifecycleState::Degrading;

        let config = RevalidationSchedulerConfig::default();
        let targets = schedule_revalidations(&[profile], now, &config);
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].asset_id, "reval-1");
    }

    #[test]
    fn revalidation_skips_active_and_revoked() {
        let now = 1_700_000_000_000i64;
        let mut active = ConfidenceProfile::new("active", now);
        active.lifecycle_state = ConfidenceLifecycleState::Active;

        let mut revoked = ConfidenceProfile::new("revoked", now);
        revoked.lifecycle_state = ConfidenceLifecycleState::Revoked;

        let config = RevalidationSchedulerConfig::default();
        let targets = schedule_revalidations(&[active, revoked], now, &config);
        assert!(targets.is_empty());
    }

    #[test]
    fn revalidation_priority_demoted_is_critical() {
        let now = 1_700_000_000_000i64;
        let mut profile = ConfidenceProfile::new("demoted-1", now);
        profile.lifecycle_state = ConfidenceLifecycleState::Demoted;
        profile.freshness.score = 0.2;

        let config = RevalidationSchedulerConfig::default();
        let targets = schedule_revalidations(&[profile], now, &config);
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].priority, RevalidationPriority::Critical);
    }

    #[test]
    fn revalidation_respects_max_concurrent() {
        let now = 1_700_000_000_000i64;
        let profiles: Vec<ConfidenceProfile> = (0..10)
            .map(|i| {
                let mut p = ConfidenceProfile::new(format!("asset-{i}"), now);
                p.freshness.score = 0.2;
                p.lifecycle_state = ConfidenceLifecycleState::NeedsRevalidation;
                p
            })
            .collect();

        let config = RevalidationSchedulerConfig {
            max_concurrent: 3,
            ..Default::default()
        };
        let targets = schedule_revalidations(&profiles, now, &config);
        assert_eq!(targets.len(), 3);
    }

    #[test]
    fn apply_revalidation_passed_restores_confidence() {
        let now = 1_700_000_000_000i64;
        let mut profile = ConfidenceProfile::new("reval-pass", now);
        profile.freshness.score = 0.3;
        profile.lifecycle_state = ConfidenceLifecycleState::NeedsRevalidation;
        let old = profile.composite_score();

        let result = apply_revalidation_outcome(
            &mut profile,
            ShadowRevalidationOutcome::Passed,
            now + 1000,
            Some("env-hash-new".to_string()),
        );
        assert_eq!(result.outcome, ShadowRevalidationOutcome::Passed);
        assert!(result.new_composite > old);
    }

    #[test]
    fn apply_revalidation_failed_may_demote() {
        let now = 1_700_000_000_000i64;
        let mut profile = ConfidenceProfile::new("reval-fail", now);
        // Set last_validated far in the past so refresh_score keeps freshness low
        profile.freshness.last_validated_ms = now - 30 * 3_600_000; // 30 hours ago
        profile.freshness.score = 0.2;
        profile.compatibility.score = 0.3;
        profile.lifecycle_state = ConfidenceLifecycleState::NeedsRevalidation;

        let result = apply_revalidation_outcome(
            &mut profile,
            ShadowRevalidationOutcome::Failed,
            now + 1000,
            None,
        );
        assert_eq!(result.outcome, ShadowRevalidationOutcome::Failed);
        // With low freshness and compatibility, should be demoted
        assert_eq!(profile.lifecycle_state, ConfidenceLifecycleState::Demoted);
    }

    // -----------------------------------------------------------------------
    // Drift-Aware Trust Degradation tests (Issue #387)
    // -----------------------------------------------------------------------

    #[test]
    fn drift_degradation_reduces_compatibility() {
        let now = 1_700_000_000_000i64;
        let mut profile = ConfidenceProfile::new("drift-1", now);
        let signals = vec![DriftSignal {
            kind: DriftSignalKind::DependencyChange,
            old_value: "hash-a".to_string(),
            new_value: "hash-b".to_string(),
            severity: 0.6,
            detected_at_ms: now,
        }];
        let config = DriftDegradationConfig::default();
        let result = apply_drift_degradation(&mut profile, &signals, now + 100, &config);
        assert_eq!(result.signals_processed, 1);
        assert!(result.new_composite < result.previous_composite);
        assert!(!result.forced_revalidation);
    }

    #[test]
    fn drift_compiler_change_forces_revalidation() {
        let now = 1_700_000_000_000i64;
        let mut profile = ConfidenceProfile::new("drift-2", now);
        let signals = vec![DriftSignal {
            kind: DriftSignalKind::CompilerVersionChange,
            old_value: "1.75.0".to_string(),
            new_value: "1.76.0".to_string(),
            severity: 0.9,
            detected_at_ms: now,
        }];
        let config = DriftDegradationConfig::default();
        let result = apply_drift_degradation(&mut profile, &signals, now + 100, &config);
        assert!(result.forced_revalidation);
        assert_eq!(
            profile.lifecycle_state,
            ConfidenceLifecycleState::NeedsRevalidation
        );
    }

    #[test]
    fn drift_multiple_signals_cumulative() {
        let now = 1_700_000_000_000i64;
        let mut profile = ConfidenceProfile::new("drift-3", now);
        let signals = vec![
            DriftSignal {
                kind: DriftSignalKind::DependencyChange,
                old_value: "a".to_string(),
                new_value: "b".to_string(),
                severity: 0.6,
                detected_at_ms: now,
            },
            DriftSignal {
                kind: DriftSignalKind::OsChange,
                old_value: "linux".to_string(),
                new_value: "macos".to_string(),
                severity: 0.5,
                detected_at_ms: now,
            },
        ];
        let config = DriftDegradationConfig::default();
        let result = apply_drift_degradation(&mut profile, &signals, now + 100, &config);
        assert_eq!(result.signals_processed, 2);
        assert_eq!(profile.compatibility.drift_count, 2);
    }

    #[test]
    fn detect_drift_signals_finds_changes() {
        let now = 1_700_000_000_000i64;
        let old_components = vec![
            ("rustc_version", "1.75.0"),
            ("cargo_lock_hash", "abc"),
            ("target_triple", "x86_64-unknown-linux-gnu"),
        ];
        let new_components = vec![
            ("rustc_version", "1.76.0"),
            ("cargo_lock_hash", "def"),
            ("target_triple", "x86_64-unknown-linux-gnu"),
        ];
        let signals = detect_drift_signals(
            "hash-old",
            "hash-new",
            &old_components,
            &new_components,
            now,
        );
        assert_eq!(signals.len(), 2);
        assert!(signals
            .iter()
            .any(|s| s.kind == DriftSignalKind::CompilerVersionChange));
        assert!(signals
            .iter()
            .any(|s| s.kind == DriftSignalKind::DependencyChange));
    }

    #[test]
    fn detect_drift_signals_same_env_returns_empty() {
        let now = 1_700_000_000_000i64;
        let components = vec![("rustc_version", "1.75.0")];
        let signals = detect_drift_signals("same", "same", &components, &components, now);
        assert!(signals.is_empty());
    }
}
