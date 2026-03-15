//! Deduplication, priority evaluation, and rate limiting for intake events

use crate::rules::RuleEngine;
use crate::source::IntakeEvent;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// Deduplication store to track seen events
pub struct Deduplicator {
    /// Track seen event hashes with their timestamps
    seen_events: Arc<RwLock<HashMap<String, Instant>>>,
    /// Time window for deduplication (default: 24 hours)
    window: Duration,
    /// Maximum events to track
    max_entries: usize,
}

impl Deduplicator {
    /// Create a new deduplicator
    pub fn new(window_hours: u64, max_entries: usize) -> Self {
        Self {
            seen_events: Arc::new(RwLock::new(HashMap::new())),
            window: Duration::from_secs(window_hours * 3600),
            max_entries,
        }
    }

    /// Check if an event is a duplicate
    pub fn is_duplicate(&self, event: &IntakeEvent) -> bool {
        let key = self.compute_event_key(event);
        let now = Instant::now();

        let mut seen = self.seen_events.write().unwrap();

        // Check if key exists and is within window
        if let Some(ts) = seen.get(&key) {
            if now.duration_since(*ts) < self.window {
                return true;
            }
        }

        // Add/update the event
        seen.insert(key, now);

        // Cleanup old entries if over limit
        if seen.len() > self.max_entries {
            seen.retain(|_, ts| now.duration_since(*ts) < self.window);
        }

        false
    }

    /// Compute a unique key for an event
    fn compute_event_key(&self, event: &IntakeEvent) -> String {
        // Combine source type, source event ID, and title for deduplication
        format!(
            "{}:{}:{}",
            event.source_type,
            event.source_event_id.as_deref().unwrap_or("none"),
            event.title
        )
    }

    /// Get statistics
    pub fn stats(&self) -> DeduplicatorStats {
        let seen = self.seen_events.read().unwrap();
        DeduplicatorStats {
            tracked_events: seen.len(),
            window_hours: self.window.as_secs() / 3600,
        }
    }
}

impl Default for Deduplicator {
    fn default() -> Self {
        Self::new(24, 10000)
    }
}

/// Statistics about the deduplicator
#[derive(Debug, Clone)]
pub struct DeduplicatorStats {
    pub tracked_events: usize,
    pub window_hours: u64,
}

/// Priority evaluator for intake events
pub struct PriorityEvaluator {
    /// Weights for different factors
    weights: PriorityWeights,
}

/// Weights for priority calculation
#[derive(Clone, Debug)]
pub struct PriorityWeights {
    /// Weight for severity (0-100)
    pub severity: f32,
    /// Weight for signal confidence
    pub confidence: f32,
    /// Weight for recency (recent events get higher priority)
    pub recency: f32,
    /// Weight for source reliability
    pub source_reliability: f32,
}

impl Default for PriorityWeights {
    fn default() -> Self {
        Self {
            severity: 0.4,
            confidence: 0.3,
            recency: 0.15,
            source_reliability: 0.15,
        }
    }
}

impl PriorityEvaluator {
    /// Create a new priority evaluator
    pub fn new(weights: PriorityWeights) -> Self {
        Self { weights }
    }

    /// Evaluate priority for an event (0-100, higher is more urgent)
    pub fn evaluate(&self, event: &IntakeEvent, signals: &[crate::signal::ExtractedSignal]) -> i32 {
        let severity_score = self.evaluate_severity(event);
        let confidence_score = self.evaluate_confidence(signals);
        let recency_score = self.evaluate_recency(event);
        let source_score = self.evaluate_source(event);

        let score = (severity_score * self.weights.severity
            + confidence_score * self.weights.confidence
            + recency_score * self.weights.recency
            + source_score * self.weights.source_reliability) as i32;

        score.max(0).min(100)
    }

    fn evaluate_severity(&self, event: &IntakeEvent) -> f32 {
        match event.severity {
            crate::source::IssueSeverity::Critical => 100.0,
            crate::source::IssueSeverity::High => 75.0,
            crate::source::IssueSeverity::Medium => 50.0,
            crate::source::IssueSeverity::Low => 25.0,
            crate::source::IssueSeverity::Info => 10.0,
        }
    }

    fn evaluate_confidence(&self, signals: &[crate::signal::ExtractedSignal]) -> f32 {
        if signals.is_empty() {
            return 50.0; // Default
        }

        let avg_confidence: f32 =
            signals.iter().map(|s| s.confidence).sum::<f32>() / signals.len() as f32;

        avg_confidence * 100.0
    }

    fn evaluate_recency(&self, event: &IntakeEvent) -> f32 {
        let age_hours = (chrono::Utc::now().timestamp_millis() - event.timestamp_ms) as f32
            / (1000.0 * 60.0 * 60.0);

        // Score decreases linearly over 7 days
        let score = 100.0 - (age_hours * 100.0 / 7.0 / 24.0);
        score.max(0.0).min(100.0)
    }

    fn evaluate_source(&self, event: &IntakeEvent) -> f32 {
        // Higher reliability for verified CI/CD sources
        match event.source_type {
            crate::source::IntakeSourceType::Github => 90.0,
            crate::source::IntakeSourceType::Gitlab => 90.0,
            crate::source::IntakeSourceType::Prometheus => 85.0,
            crate::source::IntakeSourceType::Sentry => 80.0,
            crate::source::IntakeSourceType::LogFile => 60.0,
            crate::source::IntakeSourceType::Http => 50.0,
        }
    }
}

impl Default for PriorityEvaluator {
    fn default() -> Self {
        Self::new(PriorityWeights::default())
    }
}

/// Rate limiter for intake events
pub struct RateLimiter {
    /// Track request timestamps
    requests: Arc<RwLock<Vec<Instant>>>,
    /// Maximum requests per minute
    max_per_minute: usize,
    /// Maximum concurrent operations
    max_concurrent: usize,
    /// Currently active operations
    active: Arc<RwLock<usize>>,
    /// Backoff duration in seconds
    backoff_seconds: u64,
}

impl RateLimiter {
    /// Create a new rate limiter
    pub fn new(max_per_minute: usize, max_concurrent: usize, backoff_seconds: u64) -> Self {
        Self {
            requests: Arc::new(RwLock::new(Vec::new())),
            max_per_minute,
            max_concurrent,
            active: Arc::new(RwLock::new(0)),
            backoff_seconds,
        }
    }

    /// Try to acquire permission to process an event
    /// Returns Ok(()) if allowed, Err(backoff_seconds) if rate limited
    pub fn try_acquire(&self) -> Result<(), u64> {
        let now = Instant::now();

        // Check concurrent limit
        {
            let active = self.active.read().unwrap();
            if *active >= self.max_concurrent {
                return Err(self.backoff_seconds);
            }
        }

        // Check rate limit
        {
            let mut requests = self.requests.write().unwrap();

            // Remove old requests (older than 1 minute)
            let one_minute_ago = now - Duration::from_secs(60);
            requests.retain(|ts| *ts > one_minute_ago);

            if requests.len() >= self.max_per_minute {
                return Err(self.backoff_seconds);
            }

            requests.push(now);
        }

        // Increment active count
        {
            let mut active = self.active.write().unwrap();
            *active += 1;
        }

        Ok(())
    }

    /// Release the permission
    pub fn release(&self) {
        let mut active = self.active.write().unwrap();
        if *active > 0 {
            *active -= 1;
        }
    }

    /// Get current stats
    pub fn stats(&self) -> RateLimiterStats {
        let active = *self.active.read().unwrap();
        let requests = self.requests.read().unwrap();
        let now = Instant::now();
        let one_minute_ago = now - Duration::from_secs(60);
        let recent_count = requests.iter().filter(|ts| **ts > one_minute_ago).count();

        RateLimiterStats {
            active_operations: active,
            requests_last_minute: recent_count,
            max_per_minute: self.max_per_minute,
            max_concurrent: self.max_concurrent,
        }
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new(60, 10, 60)
    }
}

/// Statistics about the rate limiter
#[derive(Debug, Clone)]
pub struct RateLimiterStats {
    pub active_operations: usize,
    pub requests_last_minute: usize,
    pub max_per_minute: usize,
    pub max_concurrent: usize,
}

/// Auto-prioritizer combining deduplication, priority evaluation, and rate limiting
pub struct AutoPrioritizer {
    deduplicator: Deduplicator,
    rule_engine: RuleEngine,
    evaluator: PriorityEvaluator,
    limiter: RateLimiter,
}

impl AutoPrioritizer {
    /// Create a new auto-prioritizer
    pub fn new(
        deduplicator: Deduplicator,
        rule_engine: RuleEngine,
        evaluator: PriorityEvaluator,
        limiter: RateLimiter,
    ) -> Self {
        Self {
            deduplicator,
            rule_engine,
            evaluator,
            limiter,
        }
    }

    /// Override the rule engine used between deduplication and prioritization.
    pub fn with_rule_engine(mut self, rule_engine: RuleEngine) -> Self {
        self.rule_engine = rule_engine;
        self
    }

    /// Process an event through the full prioritization pipeline
    pub fn process(
        &self,
        event: &IntakeEvent,
        signals: &[crate::signal::ExtractedSignal],
    ) -> PrioritizationResult {
        // Check deduplication
        if self.deduplicator.is_duplicate(event) {
            return PrioritizationResult::Duplicate;
        }

        let rule_result = self.rule_engine.apply(event, signals);
        if rule_result.should_skip {
            return PrioritizationResult::Filtered {
                rule_ids: rule_result
                    .applications
                    .into_iter()
                    .map(|application| application.rule_id)
                    .collect(),
            };
        }

        let event = rule_result.event;

        // Check rate limit
        if let Err(backoff) = self.limiter.try_acquire() {
            return PrioritizationResult::RateLimited(backoff);
        }

        // Evaluate priority
        let priority = self.evaluator.evaluate(&event, signals);

        PrioritizationResult::Processed(PrioritizedEvent { event, priority })
    }

    /// Release a processed event (for rate limiting)
    pub fn release(&self) {
        self.limiter.release();
    }

    /// Get stats for all components
    pub fn stats(&self) -> PrioritizerStats {
        PrioritizerStats {
            deduplicator: self.deduplicator.stats(),
            rate_limiter: self.limiter.stats(),
        }
    }
}

impl Default for AutoPrioritizer {
    fn default() -> Self {
        Self::new(
            Deduplicator::default(),
            RuleEngine::default(),
            PriorityEvaluator::default(),
            RateLimiter::default(),
        )
    }
}

/// Result of prioritization
#[derive(Debug)]
pub enum PrioritizationResult {
    /// Event was processed successfully
    Processed(PrioritizedEvent),
    /// Event is a duplicate
    Duplicate,
    /// Event was filtered out by the rule engine.
    Filtered { rule_ids: Vec<String> },
    /// Event is rate limited
    RateLimited(u64),
}

/// A prioritized intake event
#[derive(Debug, Clone)]
pub struct PrioritizedEvent {
    pub event: IntakeEvent,
    pub priority: i32,
}

/// Combined stats
#[derive(Debug)]
pub struct PrioritizerStats {
    pub deduplicator: DeduplicatorStats,
    pub rate_limiter: RateLimiterStats,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{IntakeRule, RuleAction, RuleConditions};
    use crate::source::{IntakeSourceType, IssueSeverity};

    #[test]
    fn test_deduplication() {
        let dedup = Deduplicator::new(24, 100);

        let event = IntakeEvent {
            event_id: "test-1".to_string(),
            source_type: IntakeSourceType::Github,
            source_event_id: Some("run-123".to_string()),
            title: "Build failed".to_string(),
            description: "Test".to_string(),
            severity: IssueSeverity::High,
            signals: vec![],
            raw_payload: None,
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
        };

        // First time should not be duplicate
        assert!(!dedup.is_duplicate(&event));

        // Second time should be duplicate
        assert!(dedup.is_duplicate(&event));
    }

    #[test]
    fn test_priority_evaluation() {
        let evaluator = PriorityEvaluator::default();

        let event = IntakeEvent {
            event_id: "test-1".to_string(),
            source_type: IntakeSourceType::Github,
            source_event_id: None,
            title: "Critical bug".to_string(),
            description: "Test".to_string(),
            severity: IssueSeverity::Critical,
            signals: vec![],
            raw_payload: None,
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
        };

        let signals = vec![crate::signal::ExtractedSignal {
            signal_id: "sig-1".to_string(),
            content: "test".to_string(),
            signal_type: crate::signal::SignalType::CompilerError,
            confidence: 0.9,
            source: "test".to_string(),
        }];

        let priority = evaluator.evaluate(&event, &signals);
        assert!(priority >= 50); // Should be high priority
    }

    #[test]
    fn test_rate_limiter() {
        let limiter = RateLimiter::new(10, 5, 1);

        // Should be able to acquire up to limit
        for _ in 0..5 {
            assert!(limiter.try_acquire().is_ok());
        }

        // Sixth should fail due to concurrent limit
        assert!(limiter.try_acquire().is_err());
    }

    #[test]
    fn test_auto_prioritizer() {
        let prioritizer = AutoPrioritizer::default();

        let event = IntakeEvent {
            event_id: "test-1".to_string(),
            source_type: IntakeSourceType::Github,
            source_event_id: Some("run-456".to_string()),
            title: "Test issue".to_string(),
            description: "Test".to_string(),
            severity: IssueSeverity::Medium,
            signals: vec![],
            raw_payload: None,
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
        };

        let result = prioritizer.process(&event, &[]);
        assert!(matches!(result, PrioritizationResult::Processed(_)));
    }

    #[test]
    fn test_auto_prioritizer_filters_event_via_rule_engine() {
        let prioritizer =
            AutoPrioritizer::default().with_rule_engine(RuleEngine::with_rules(vec![IntakeRule {
                id: "skip_http".to_string(),
                name: "Skip http events".to_string(),
                description: "filter low-value http events".to_string(),
                priority: 100,
                enabled: true,
                conditions: RuleConditions {
                    source_types: vec!["http".to_string()],
                    ..Default::default()
                },
                actions: vec![RuleAction::Skip],
            }]));

        let event = IntakeEvent {
            event_id: "test-filter".to_string(),
            source_type: IntakeSourceType::Http,
            source_event_id: Some("webhook-1".to_string()),
            title: "Noisy webhook".to_string(),
            description: "ignore this".to_string(),
            severity: IssueSeverity::Low,
            signals: vec![],
            raw_payload: None,
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
        };

        let result = prioritizer.process(&event, &[]);
        match result {
            PrioritizationResult::Filtered { rule_ids } => {
                assert_eq!(rule_ids, vec!["skip_http".to_string()]);
            }
            other => panic!("expected Filtered result, got {:?}", other),
        }
    }
}
