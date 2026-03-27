//! Oris Intake - Automatic Issue Intake System for Self-Evolution
//!
//! This crate provides automatic issue intake capabilities:
//! - CI/CD webhook listener (GitHub Actions, GitLab CI)
//! - Monitoring alert integration (Prometheus, Sentry)
//! - Error log signal extraction
//! - Automatic mutation/task creation
//!
//! ## Architecture
//!
//! ```text
//! External Source (CI/CD, Monitoring, Logs)
//!     |
//!     v
//! IntakeSource (trait)
//!     |
//!     v
//! SignalExtractor
//!     |
//!     v
//! MutationBuilder -> Evolution Store
//! ```

pub mod admission;
mod continuous;
pub mod evidence;
mod mutation;
pub mod planning;
mod prioritize;
pub mod proposal;
mod rules;
#[cfg(feature = "webhook")]
pub mod server;
mod signal;
mod source;

pub use admission::{
    AdmissionConfig, AdmissionDecision, AdmissionGate, AdmissionInput, RejectionFeedback,
};
pub use continuous::*;
pub use evidence::{
    is_bundle_deliverable, validate_bundle, BundleValidationResult, EvidenceBundle,
    EvidenceBundleBuilder, EvidenceCompleteness,
};
pub use mutation::*;
pub use planning::{
    builtin_planning_contracts, EvidenceType, PlanningContract, PlanningContractRegistry,
    PlanValidationResult, PlanViolation, RequiredEvidence,
};
pub use prioritize::*;
pub use proposal::{
    validate_proposal, ProposalBuilder, ProposalContract, ProposalEffect, ProposalIntent,
    ProposalRollback, ProposalScope, ProposalValidation, ProposalValidationResult,
    RollbackStrategy,
};
pub use rules::*;
pub use signal::*;
pub use source::*;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors that can occur during intake processing
#[derive(Error, Debug)]
pub enum IntakeError {
    #[error("Failed to parse webhook payload: {0}")]
    ParseError(String),

    #[error("Failed to extract signals: {0}")]
    SignalExtractionError(String),

    #[error("Failed to create mutation: {0}")]
    MutationError(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Storage error: {0}")]
    StorageError(String),
}

/// Result type for intake operations
pub type IntakeResult<T> = Result<T, IntakeError>;

/// Configuration for the intake system
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IntakeConfig {
    /// Enable/disable the intake system
    pub enabled: bool,

    /// Supported intake sources
    pub sources: Vec<IntakeSourceConfig>,

    /// Signal extraction settings
    pub signal_extraction: SignalExtractionConfig,

    /// Intake rate limiting
    pub rate_limit: RateLimitConfig,
}

impl Default for IntakeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            sources: vec![],
            signal_extraction: SignalExtractionConfig::default(),
            rate_limit: RateLimitConfig::default(),
        }
    }
}

/// Configuration for a specific intake source
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IntakeSourceConfig {
    /// Source type (github, gitlab, prometheus, sentry, etc.)
    pub source_type: String,

    /// Whether this source is enabled
    pub enabled: bool,

    /// Source-specific configuration
    pub config: serde_json::Value,
}

/// Signal extraction configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SignalExtractionConfig {
    /// Minimum confidence threshold for extracted signals
    pub min_confidence: f32,

    /// Maximum signals per intake event
    pub max_signals: usize,

    /// Whether to enable automatic pattern learning
    pub enable_pattern_learning: bool,
}

impl Default for SignalExtractionConfig {
    fn default() -> Self {
        Self {
            min_confidence: 0.5,
            max_signals: 10,
            enable_pattern_learning: false,
        }
    }
}

/// Rate limiting configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Maximum requests per minute
    pub max_requests_per_minute: usize,

    /// Maximum concurrent intakes
    pub max_concurrent: usize,

    /// Backoff duration on rate limit (seconds)
    pub backoff_seconds: u64,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_requests_per_minute: 60,
            max_concurrent: 10,
            backoff_seconds: 60,
        }
    }
}

/// Generate a unique ID for intake events
pub fn generate_intake_id(prefix: &str) -> String {
    let uuid = uuid::Uuid::new_v4();
    format!("{}-{}", prefix, uuid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_intake_id() {
        let id = generate_intake_id("intake");
        assert!(id.starts_with("intake-"));
    }

    #[test]
    fn test_default_config() {
        let config = IntakeConfig::default();
        assert!(config.enabled);
        assert!(config.sources.is_empty());
    }
}
