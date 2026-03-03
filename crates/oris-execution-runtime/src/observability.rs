//! Backpressure and kernel observability types for scheduling and telemetry.
//!
//! **RejectionReason**: why a dispatch or API request was rejected (e.g. tenant limit),
//! for safe backpressure and clear API responses.
//!
//! **KernelObservability**: shared structure for runtime-derived kernel telemetry
//! (reasoning timeline, lease graph, replay cost, interrupt gap). The runtime
//! populates these fields from checkpoint and trace context data so APIs can
//! surface stable observability without inventing a second schema.

#[cfg(feature = "execution-server")]
use crate::graph_bridge::ExecutionCheckpointView;
use oris_kernel::KernelTraceEvent;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Reason for rejecting a dispatch or API request (e.g. rate limit, tenant cap).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum RejectionReason {
    /// Tenant-level limit exceeded; optional description of the limit.
    TenantLimit(Option<String>),
    /// Worker or capacity limit.
    CapacityLimit(Option<String>),
    /// Other rejections (policy, invalid request, etc.).
    Other(String),
}

impl RejectionReason {
    pub fn tenant_limit(description: impl Into<String>) -> Self {
        RejectionReason::TenantLimit(Some(description.into()))
    }

    pub fn capacity_limit(description: impl Into<String>) -> Self {
        RejectionReason::CapacityLimit(Some(description.into()))
    }
}

/// Runtime-derived kernel observability / telemetry.
///
/// Fields are optional so responses can remain backward compatible when a
/// given execution path does not have enough source data.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct KernelObservability {
    /// Optional reasoning or decision timeline (e.g. scheduler steps).
    pub reasoning_timeline: Option<Vec<String>>,
    /// Optional lease/ownership snapshot (e.g. attempt → worker).
    pub lease_graph: Option<Vec<(String, String)>>,
    /// Optional replay cost hint (e.g. event count or duration).
    pub replay_cost: Option<u64>,
    /// Optional interrupt handling latency (e.g. ms).
    pub interrupt_latency_ms: Option<u64>,
}

impl KernelObservability {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_reasoning_timeline(mut self, entries: Vec<String>) -> Self {
        self.reasoning_timeline = Some(entries);
        self
    }

    pub fn with_lease_graph(mut self, edges: Vec<(String, String)>) -> Self {
        self.lease_graph = Some(edges);
        self
    }

    pub fn with_replay_cost(mut self, cost: u64) -> Self {
        self.replay_cost = Some(cost);
        self
    }

    pub fn with_interrupt_latency_ms(mut self, ms: u64) -> Self {
        self.interrupt_latency_ms = Some(ms);
        self
    }

    pub fn from_kernel_trace(trace: &[KernelTraceEvent]) -> Self {
        let reasoning_timeline = if trace.is_empty() {
            None
        } else {
            Some(trace.iter().map(format_trace_event).collect())
        };
        let replay_cost = if trace.is_empty() {
            None
        } else {
            Some(trace.len() as u64)
        };
        let interrupt_latency_ms = interrupt_latency_from_trace_timestamps(trace).or_else(|| {
            trace
                .iter()
                .position(|event| event.kind == "Interrupted")
                .zip(trace.iter().position(|event| event.kind == "Resumed"))
                .and_then(|(interrupted, resumed)| resumed.checked_sub(interrupted))
                .map(|delta| delta as u64)
        });

        Self {
            reasoning_timeline,
            lease_graph: None,
            replay_cost,
            interrupt_latency_ms,
        }
    }

    #[cfg(feature = "execution-server")]
    pub fn from_checkpoint_history(run_id: &str, history: &[ExecutionCheckpointView]) -> Self {
        Self::from_checkpoint_history_with_lease_graph(run_id, history, None)
    }

    #[cfg(feature = "execution-server")]
    pub fn from_checkpoint_history_with_lease_graph(
        run_id: &str,
        history: &[ExecutionCheckpointView],
        lease_graph: Option<Vec<(String, String)>>,
    ) -> Self {
        let trace: Vec<KernelTraceEvent> = history
            .iter()
            .enumerate()
            .map(|(index, checkpoint)| KernelTraceEvent {
                run_id: run_id.to_string(),
                seq: (index + 1) as u64,
                step_id: checkpoint.checkpoint_id.clone(),
                action_id: None,
                kind: "CheckpointSaved".into(),
                timestamp_ms: Some(checkpoint.created_at.timestamp_millis()),
            })
            .collect();
        let mut observability = Self::from_kernel_trace(&trace);
        observability.lease_graph = lease_graph.filter(|edges| !edges.is_empty());
        observability.interrupt_latency_ms = history
            .windows(2)
            .filter_map(|window| {
                let delta_ms = (window[1].created_at - window[0].created_at).num_milliseconds();
                (delta_ms >= 0).then_some(delta_ms as u64)
            })
            .max();
        observability
    }
}

fn format_trace_event(event: &KernelTraceEvent) -> String {
    let mut entry = format!("{}#{}", event.kind, event.seq);
    if let Some(step_id) = &event.step_id {
        entry.push('(');
        entry.push_str(step_id);
        entry.push(')');
    } else if let Some(action_id) = &event.action_id {
        entry.push('(');
        entry.push_str(action_id);
        entry.push(')');
    }
    entry
}

fn interrupt_latency_from_trace_timestamps(trace: &[KernelTraceEvent]) -> Option<u64> {
    let mut interrupted_at = None;
    let mut max_delta_ms = None;
    for event in trace {
        match event.kind.as_str() {
            "Interrupted" => interrupted_at = event.timestamp_ms,
            "Resumed" => {
                if let (Some(start_ms), Some(end_ms)) = (interrupted_at.take(), event.timestamp_ms)
                {
                    if end_ms >= start_ms {
                        let delta = (end_ms - start_ms) as u64;
                        max_delta_ms =
                            Some(max_delta_ms.map_or(delta, |current: u64| current.max(delta)));
                    }
                }
            }
            _ => {}
        }
    }
    max_delta_ms
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejection_reason_tenant_limit() {
        let r = RejectionReason::tenant_limit("max concurrent runs");
        assert!(
            matches!(r, RejectionReason::TenantLimit(Some(ref s)) if s == "max concurrent runs")
        );
    }

    #[test]
    fn kernel_observability_builder() {
        let o = KernelObservability::new()
            .with_reasoning_timeline(vec!["step1".into()])
            .with_replay_cost(42)
            .with_interrupt_latency_ms(10);
        assert_eq!(o.reasoning_timeline, Some(vec!["step1".into()]));
        assert_eq!(o.replay_cost, Some(42));
        assert_eq!(o.interrupt_latency_ms, Some(10));
    }

    #[test]
    fn kernel_observability_from_trace() {
        let trace = vec![
            KernelTraceEvent {
                run_id: "r1".into(),
                seq: 1,
                step_id: Some("n1".into()),
                action_id: None,
                kind: "Interrupted".into(),
                timestamp_ms: None,
            },
            KernelTraceEvent {
                run_id: "r1".into(),
                seq: 2,
                step_id: Some("n1".into()),
                action_id: None,
                kind: "Resumed".into(),
                timestamp_ms: None,
            },
        ];

        let o = KernelObservability::from_kernel_trace(&trace);
        assert_eq!(o.replay_cost, Some(2));
        assert_eq!(o.interrupt_latency_ms, Some(1));
        assert_eq!(
            o.reasoning_timeline,
            Some(vec!["Interrupted#1(n1)".into(), "Resumed#2(n1)".into()])
        );
    }

    #[cfg(feature = "execution-server")]
    #[test]
    fn kernel_observability_from_checkpoint_history() {
        use crate::graph_bridge::ExecutionCheckpointView;
        use chrono::TimeZone;

        let history = vec![
            ExecutionCheckpointView {
                checkpoint_id: Some("cp-1".into()),
                created_at: chrono::Utc.timestamp_millis_opt(1_700_000_000_000).unwrap(),
            },
            ExecutionCheckpointView {
                checkpoint_id: Some("cp-2".into()),
                created_at: chrono::Utc.timestamp_millis_opt(1_700_000_001_000).unwrap(),
            },
        ];

        let o = KernelObservability::from_checkpoint_history_with_lease_graph(
            "r-checkpoint",
            &history,
            Some(vec![("attempt-1".into(), "worker-1".into())]),
        );
        assert_eq!(o.replay_cost, Some(2));
        assert_eq!(
            o.reasoning_timeline,
            Some(vec![
                "CheckpointSaved#1(cp-1)".into(),
                "CheckpointSaved#2(cp-2)".into(),
            ])
        );
        assert_eq!(
            o.lease_graph,
            Some(vec![("attempt-1".into(), "worker-1".into())])
        );
        assert_eq!(o.interrupt_latency_ms, Some(1_000));
    }
}
