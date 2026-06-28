//! Prometheus metrics for the Oris evolution kernel.
//!
//! Exposes 5 core metrics:
//! - `oris_evolution_cycles_total` — counter of completed pipeline cycles
//! - `oris_confidence_distribution` — histogram of gene confidence scores
//! - `oris_intake_queue_depth` — gauge of pending intake events
//! - `oris_acceptance_rate` — gauge (0.0–1.0) of accepted vs total proposals
//! - `oris_replay_hit_rate` — gauge (0.0–1.0) of replay cache hits
//!
//! Enable with the `prometheus` feature flag on `oris-evokernel`.

use prometheus_client::encoding::text::encode;
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::gauge::Gauge;
use prometheus_client::metrics::histogram::{exponential_buckets, Histogram};
use prometheus_client::registry::Registry;
use std::sync::Arc;

/// Core evolution metrics collected at runtime.
#[derive(Clone)]
pub struct EvolutionMetrics {
    inner: Arc<Inner>,
}

struct Inner {
    registry: Registry,
    pub cycles_total: Counter,
    pub confidence_distribution: Histogram,
    pub intake_queue_depth: Gauge,
    pub acceptance_rate: Gauge<f64, std::sync::atomic::AtomicU64>,
    pub replay_hit_rate: Gauge<f64, std::sync::atomic::AtomicU64>,
}

impl EvolutionMetrics {
    /// Create a new metrics instance with all counters/gauges registered.
    pub fn new() -> Self {
        let mut registry = Registry::default();

        let cycles_total = Counter::default();
        registry.register(
            "oris_evolution_cycles_total",
            "Total completed evolution pipeline cycles",
            cycles_total.clone(),
        );

        let confidence_distribution = Histogram::new(exponential_buckets(0.05, 2.0, 8));
        registry.register(
            "oris_confidence_distribution",
            "Distribution of gene confidence scores",
            confidence_distribution.clone(),
        );

        let intake_queue_depth: Gauge = Gauge::default();
        registry.register(
            "oris_intake_queue_depth",
            "Number of pending intake events in the queue",
            intake_queue_depth.clone(),
        );

        let acceptance_rate: Gauge<f64, std::sync::atomic::AtomicU64> = Gauge::default();
        registry.register(
            "oris_acceptance_rate",
            "Ratio of accepted proposals to total evaluated",
            acceptance_rate.clone(),
        );

        let replay_hit_rate: Gauge<f64, std::sync::atomic::AtomicU64> = Gauge::default();
        registry.register(
            "oris_replay_hit_rate",
            "Ratio of replay cache hits to total lookups",
            replay_hit_rate.clone(),
        );

        Self {
            inner: Arc::new(Inner {
                registry,
                cycles_total,
                confidence_distribution,
                intake_queue_depth,
                acceptance_rate,
                replay_hit_rate,
            }),
        }
    }

    /// Increment the cycle counter after a pipeline execution completes.
    pub fn record_cycle(&self) {
        self.inner.cycles_total.inc();
    }

    /// Record a gene confidence observation.
    pub fn observe_confidence(&self, confidence: f64) {
        self.inner.confidence_distribution.observe(confidence);
    }

    /// Set the current intake queue depth.
    pub fn set_intake_queue_depth(&self, depth: i64) {
        self.inner.intake_queue_depth.set(depth);
    }

    /// Update the acceptance rate (accepted / total). Value in [0.0, 1.0].
    pub fn set_acceptance_rate(&self, rate: f64) {
        self.inner.acceptance_rate.set(rate);
    }

    /// Update the replay hit rate (hits / lookups). Value in [0.0, 1.0].
    pub fn set_replay_hit_rate(&self, rate: f64) {
        self.inner.replay_hit_rate.set(rate);
    }

    /// Encode all metrics in Prometheus text exposition format.
    pub fn encode(&self) -> String {
        let mut buf = String::new();
        encode(&mut buf, &self.inner.registry).expect("encoding should not fail");
        buf
    }
}

impl Default for EvolutionMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Returns the Prometheus text exposition output for use in an HTTP handler.
///
/// Usage with axum:
/// ```ignore
/// let metrics = EvolutionMetrics::new();
/// let app = axum::Router::new()
///     .route("/metrics", axum::routing::get({
///         let m = metrics.clone();
///         move || async move { m.encode() }
///     }));
/// ```
pub fn metrics_handler(metrics: &EvolutionMetrics) -> String {
    metrics.encode()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycles_counter_increments() {
        let m = EvolutionMetrics::new();
        m.record_cycle();
        m.record_cycle();
        m.record_cycle();
        let output = m.encode();
        assert!(output.contains("oris_evolution_cycles_total"));
        assert!(output.contains(" 3"));
    }

    #[test]
    fn confidence_histogram_records() {
        let m = EvolutionMetrics::new();
        m.observe_confidence(0.5);
        m.observe_confidence(0.9);
        let output = m.encode();
        assert!(output.contains("oris_confidence_distribution"));
        assert!(output.contains("_count 2"));
    }

    #[test]
    fn gauge_values_update() {
        let m = EvolutionMetrics::new();
        m.set_intake_queue_depth(42);
        m.set_acceptance_rate(0.85);
        m.set_replay_hit_rate(0.6);
        let output = m.encode();
        assert!(output.contains("oris_intake_queue_depth"));
        assert!(output.contains("oris_acceptance_rate"));
        assert!(output.contains("oris_replay_hit_rate"));
    }

    #[test]
    fn encode_produces_valid_prometheus_format() {
        let m = EvolutionMetrics::new();
        m.record_cycle();
        m.observe_confidence(0.75);
        let output = m.encode();
        // Prometheus text format has # HELP and # TYPE lines
        assert!(output.contains("# HELP"));
        assert!(output.contains("# TYPE"));
        assert!(output.contains("counter"));
        assert!(output.contains("histogram"));
        assert!(output.contains("gauge"));
    }

    #[test]
    fn metrics_are_clone_safe() {
        let m = EvolutionMetrics::new();
        let m2 = m.clone();
        m.record_cycle();
        m2.record_cycle();
        let output = m.encode();
        assert!(output.contains(" 2"));
    }
}
