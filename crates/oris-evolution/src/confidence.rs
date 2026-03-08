//! Confidence Lifecycle Scheduler
//!
//! This module implements automatic confidence decay and lifecycle management
//! for genes and capsules within the evolution crate.

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
}
