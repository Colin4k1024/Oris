//! Automatic publishing gate for evolution assets.
//!
//! This module provides automatic publishing of assets when they reach
//! certain states (e.g., "promoted"). It includes:
//! - Configurable publish targets
//! - Retry with exponential backoff
//! - Publish status tracking

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Get current timestamp as ISO 8601 string
fn now_timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let nanos = now.subsec_nanos();
    format!("{}.{:09}Z", secs, nanos)
}

/// Configuration for the publish gate
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PublishGateConfig {
    /// List of publish targets (HTTP endpoints, IPFS, etc.)
    pub targets: Vec<PublishTarget>,
    /// Whether automatic publishing is enabled
    #[serde(default = "default_enabled")]
    pub auto_publish_enabled: bool,
    /// States that trigger automatic publishing
    #[serde(default = "default_publish_states")]
    pub publish_on_states: Vec<String>,
    /// Maximum retry attempts for failed publishes
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    /// Base delay for exponential backoff (milliseconds)
    #[serde(default = "default_backoff_base_ms")]
    pub backoff_base_ms: u64,
    /// Maximum backoff delay (milliseconds)
    #[serde(default = "default_backoff_max_ms")]
    pub backoff_max_ms: u64,
    /// Whether to verify publish success
    #[serde(default = "default_verify")]
    pub verify_publish: bool,
}

fn default_enabled() -> bool {
    true
}
fn default_publish_states() -> Vec<String> {
    vec!["promoted".to_string()]
}
fn default_max_retries() -> u32 {
    3
}
fn default_backoff_base_ms() -> u64 {
    1000
}
fn default_backoff_max_ms() -> u64 {
    30000
}
fn default_verify() -> bool {
    true
}

/// A publish target endpoint
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PublishTarget {
    /// Unique identifier for the target
    pub target_id: String,
    /// Target type (http, ipfs, etc.)
    pub target_type: String,
    /// Endpoint URL
    pub endpoint: String,
    /// Optional API key
    pub api_key: Option<String>,
    /// Whether this target is enabled
    #[serde(default = "default_target_enabled")]
    pub enabled: bool,
}

fn default_target_enabled() -> bool {
    true
}

/// Status of a publish operation
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PublishStatus {
    /// Publish pending
    Pending,
    /// Publish in progress
    InProgress,
    /// Publish succeeded
    Succeeded,
    /// Publish failed
    Failed { error: String, retry_count: u32 },
    /// Publish verified
    Verified,
}

/// Information about a publish attempt
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PublishRecord {
    pub asset_id: String,
    pub asset_type: String,
    pub target_id: String,
    pub status: PublishStatus,
    pub created_at: String,
    pub updated_at: String,
    pub retry_count: u32,
    pub last_error: Option<String>,
}

/// Result of a publish operation
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PublishResult {
    pub success: bool,
    pub target_id: String,
    pub asset_id: String,
    pub error: Option<String>,
    pub published_at: String,
}

/// Publish gate for automatic publishing
#[derive(Clone)]
pub struct PublishGate {
    config: PublishGateConfig,
    publish_history: Arc<RwLock<HashMap<String, Vec<PublishRecord>>>>,
    local_peer_id: String,
}

impl PublishGate {
    /// Create a new publish gate
    pub fn new(config: PublishGateConfig, local_peer_id: String) -> Self {
        Self {
            config,
            publish_history: Arc::new(RwLock::new(HashMap::new())),
            local_peer_id,
        }
    }

    /// Check if auto-publish is enabled for a given state
    pub fn should_auto_publish(&self, state: &str) -> bool {
        if !self.config.auto_publish_enabled {
            return false;
        }
        self.config.publish_on_states.iter().any(|s| s == state)
    }

    /// Get enabled publish targets
    pub fn get_targets(&self) -> Vec<PublishTarget> {
        self.config
            .targets
            .iter()
            .filter(|t| t.enabled)
            .cloned()
            .collect()
    }

    /// Record a publish attempt
    pub fn record_publish_attempt(
        &self,
        asset_id: &str,
        asset_type: &str,
        target_id: &str,
        status: PublishStatus,
    ) {
        let mut history = self.publish_history.write().unwrap();
        let key = format!("{}:{}", asset_id, target_id);

        let record = PublishRecord {
            asset_id: asset_id.to_string(),
            asset_type: asset_type.to_string(),
            target_id: target_id.to_string(),
            status,
            created_at: now_timestamp(),
            updated_at: now_timestamp(),
            retry_count: 0,
            last_error: None,
        };

        history.entry(key).or_insert_with(Vec::new).push(record);
    }

    /// Get publish history for an asset
    pub fn get_publish_history(&self, asset_id: &str) -> Vec<PublishRecord> {
        let history = self.publish_history.read().unwrap();
        let mut records = Vec::new();

        for (key, recs) in history.iter() {
            if key.starts_with(asset_id) {
                records.extend(recs.clone());
            }
        }

        records
    }

    /// Calculate backoff delay for retry
    pub fn calculate_backoff(&self, retry_count: u32) -> Duration {
        let base = self.config.backoff_base_ms as u64;
        let max = self.config.backoff_max_ms as u64;
        let delay = base * (2_u64.pow(retry_count));
        Duration::from_millis(delay.min(max))
    }

    /// Check if we should retry a failed publish
    pub fn should_retry(&self, asset_id: &str, target_id: &str) -> bool {
        let history = self.publish_history.read().unwrap();
        let key = format!("{}:{}", asset_id, target_id);

        if let Some(records) = history.get(&key) {
            if let Some(last) = records.last() {
                if let PublishStatus::Failed { retry_count, .. } = &last.status {
                    return *retry_count < self.config.max_retries;
                }
            }
        }

        false
    }

    /// Get publish status for an asset
    pub fn get_publish_status(&self, asset_id: &str) -> HashMap<String, PublishStatus> {
        let history = self.publish_history.read().unwrap();
        let mut statuses = HashMap::new();

        for (key, records) in history.iter() {
            if key.starts_with(asset_id) {
                if let Some(last) = records.last() {
                    statuses.insert(last.target_id.clone(), last.status.clone());
                }
            }
        }

        statuses
    }

    /// Get config
    pub fn config(&self) -> &PublishGateConfig {
        &self.config
    }

    /// Get local peer ID
    pub fn local_peer_id(&self) -> &str {
        &self.local_peer_id
    }
}

/// Builder for creating publish targets
pub struct PublishTargetBuilder {
    target_id: String,
    target_type: String,
    endpoint: String,
    api_key: Option<String>,
    enabled: bool,
}

impl PublishTargetBuilder {
    pub fn new(target_id: String, target_type: String, endpoint: String) -> Self {
        Self {
            target_id,
            target_type,
            endpoint,
            api_key: None,
            enabled: true,
        }
    }

    pub fn api_key(mut self, key: String) -> Self {
        self.api_key = Some(key);
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn build(self) -> PublishTarget {
        PublishTarget {
            target_id: self.target_id,
            target_type: self.target_type,
            endpoint: self.endpoint,
            api_key: self.api_key,
            enabled: self.enabled,
        }
    }
}

/// Default publish gate configuration
impl Default for PublishGateConfig {
    fn default() -> Self {
        Self {
            targets: Vec::new(),
            auto_publish_enabled: false,
            publish_on_states: default_publish_states(),
            max_retries: default_max_retries(),
            backoff_base_ms: default_backoff_base_ms(),
            backoff_max_ms: default_backoff_max_ms(),
            verify_publish: default_verify(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_publish_gate_creation() {
        let config = PublishGateConfig::default();
        let gate = PublishGate::new(config, "local-peer".to_string());

        assert!(!gate.should_auto_publish("promoted")); // auto_publish is disabled by default
    }

    #[test]
    fn test_auto_publish_enabled() {
        let mut config = PublishGateConfig::default();
        config.auto_publish_enabled = true;

        let gate = PublishGate::new(config, "local-peer".to_string());

        assert!(gate.should_auto_publish("promoted"));
        assert!(!gate.should_auto_publish("draft"));
    }

    #[test]
    fn test_backoff_calculation() {
        let config = PublishGateConfig {
            backoff_base_ms: 1000,
            backoff_max_ms: 30000,
            ..Default::default()
        };
        let gate = PublishGate::new(config, "peer".to_string());

        // First retry: 1000ms * 2^0 = 1000ms
        assert_eq!(gate.calculate_backoff(0), Duration::from_millis(1000));

        // Second retry: 1000ms * 2^1 = 2000ms
        assert_eq!(gate.calculate_backoff(1), Duration::from_millis(2000));

        // Third retry: 1000ms * 2^2 = 4000ms
        assert_eq!(gate.calculate_backoff(2), Duration::from_millis(4000));

        // Capped at max
        assert_eq!(gate.calculate_backoff(10), Duration::from_millis(30000));
    }

    #[test]
    fn test_target_builder() {
        let target = PublishTargetBuilder::new(
            "http-target".to_string(),
            "http".to_string(),
            "https://example.com/publish".to_string(),
        )
        .api_key("secret".to_string())
        .enabled(true)
        .build();

        assert_eq!(target.target_id, "http-target");
        assert_eq!(target.target_type, "http");
        assert_eq!(target.endpoint, "https://example.com/publish");
        assert_eq!(target.api_key, Some("secret".to_string()));
    }
}
