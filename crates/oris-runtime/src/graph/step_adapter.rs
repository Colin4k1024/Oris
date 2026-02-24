//! GraphStepFnAdapter: wraps CompiledGraph as a sync StepFn for the kernel.
//!
//! Runs one graph node per step via block_on; state includes graph state + current node.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::kernel::event::Event;
use crate::kernel::state::KernelState;
use crate::kernel::step::{InterruptInfo, Next, StepFn};

use super::compiled::CompiledGraph;
use super::state::State;
use super::step_result::GraphStepOnceResult;
use crate::graph::persistence::config::RunnableConfig;
use crate::kernel::KernelError;

/// State for the kernel when driving a graph: graph state + current node (for replay).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(bound = "S: State + serde::Serialize + serde::de::DeserializeOwned")]
pub struct GraphStepState<S: State> {
    pub graph_state: S,
    pub current_node: String,
}

impl<S: State> GraphStepState<S> {
    pub fn new(graph_state: S) -> Self {
        Self {
            graph_state,
            current_node: super::edge::START.to_string(),
        }
    }
}

impl<S: State + KernelState> KernelState for GraphStepState<S> {
    fn version(&self) -> u32 {
        self.graph_state.version()
    }
}

/// Sync StepFn that runs one graph node per next() via block_on.
pub struct GraphStepFnAdapter<S: State + KernelState> {
    pub graph: Arc<CompiledGraph<S>>,
    pub config: Option<RunnableConfig>,
}

impl<S: State + KernelState + 'static> GraphStepFnAdapter<S> {
    pub fn new(graph: Arc<CompiledGraph<S>>) -> Self {
        Self {
            graph,
            config: None,
        }
    }

    pub fn with_config(graph: Arc<CompiledGraph<S>>, config: RunnableConfig) -> Self {
        Self {
            graph,
            config: Some(config),
        }
    }
}

impl<S: State + KernelState + 'static> StepFn<GraphStepState<S>> for GraphStepFnAdapter<S> {
    /// Requires a Tokio runtime on the current thread; use `Handle::try_current()` to check.
    /// From async, use `block_in_place` or a dedicated thread.
    fn next(&self, state: &GraphStepState<S>) -> Result<Next, KernelError> {
        let handle = tokio::runtime::Handle::try_current().map_err(|_| {
            KernelError::Driver(
                "Tokio runtime required: call from a thread with an entered runtime (e.g. after Runtime::new() and rt.enter()), or use block_in_place from an async task. Do not call from inside an async task without block_in_place.".into(),
            )
        })?;
        let config = self.config.as_ref();
        let result = handle.block_on(self.graph.step_once(
            &state.graph_state,
            &state.current_node,
            config,
        ));
        match result.map_err(|e| KernelError::Driver(e.to_string()))? {
            GraphStepOnceResult::Emit {
                executed_node,
                new_state,
                next_node,
            } => {
                let graph_state = serde_json::to_value(&new_state)
                    .map_err(|e| KernelError::Driver(e.to_string()))?;
                let payload = serde_json::json!({
                    "graph_state": graph_state,
                    "next_node": next_node,
                });
                Ok(Next::Emit(vec![Event::StateUpdated {
                    step_id: Some(executed_node),
                    payload,
                }]))
            }
            GraphStepOnceResult::Interrupt { value, .. } => {
                Ok(Next::Interrupt(InterruptInfo { value }))
            }
            GraphStepOnceResult::Complete { .. } => Ok(Next::Complete),
        }
    }
}

/// Reducer that applies events to GraphStepState.
/// Supports envelope payload (`graph_state` + `next_node`) or legacy (payload = state, step_id = cursor).
#[derive(Debug, Clone, Default)]
pub struct GraphStepReducer;

impl<S> crate::kernel::Reducer<GraphStepState<S>> for GraphStepReducer
where
    S: State + KernelState + serde::de::DeserializeOwned,
{
    fn apply(
        &self,
        state: &mut GraphStepState<S>,
        event: &crate::kernel::event::SequencedEvent,
    ) -> Result<(), KernelError> {
        if let Event::StateUpdated { step_id, payload } = &event.event {
            if let (Some(gs), Some(nn)) = (
                payload.get("graph_state"),
                payload.get("next_node").and_then(|v| v.as_str()),
            ) {
                state.graph_state = serde_json::from_value(gs.clone())
                    .map_err(|e| KernelError::EventStore(e.to_string()))?;
                state.current_node = nn.to_string();
            } else {
                state.graph_state = serde_json::from_value(payload.clone())
                    .map_err(|e| KernelError::EventStore(e.to_string()))?;
                if let Some(ref next) = step_id {
                    state.current_node = next.clone();
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::state::MessagesState;
    use crate::graph::GraphStepOnceResult;
    use crate::graph::{function_node, StateGraph, END, START};
    use crate::kernel::driver::{Kernel, RunStatus};
    use crate::kernel::event_store::InMemoryEventStore;
    use crate::kernel::runner::KernelRunner;
    use crate::kernel::stubs::{AllowAllPolicy, NoopActionExecutor};
    use std::sync::Arc;

    /// Full kernel + adapter test using KernelRunner (no manual runtime/block_in_place).
    #[test]
    fn graph_step_adapter_runs_to_complete() {
        let mut graph = StateGraph::<MessagesState>::new();
        graph
            .add_node(
                "node1",
                function_node("node1", |_s: &MessagesState| async move {
                    Ok(std::collections::HashMap::new())
                }),
            )
            .unwrap();
        graph.add_edge(START, "node1");
        graph.add_edge("node1", END);
        let compiled = Arc::new(graph.compile().unwrap());
        let adapter = GraphStepFnAdapter::new(compiled);
        let kernel: Kernel<GraphStepState<MessagesState>> = Kernel {
            events: Box::new(InMemoryEventStore::new()),
            snaps: None,
            reducer: Box::new(GraphStepReducer),
            exec: Box::new(NoopActionExecutor),
            step: Box::new(adapter),
            policy: Box::new(AllowAllPolicy),
        };
        let runner = KernelRunner::new(kernel);
        let run_id = "graph-step-test".to_string();
        let initial = GraphStepState::new(MessagesState::new());
        let status = runner.run_until_blocked_sync(&run_id, initial).unwrap();
        assert!(matches!(status, RunStatus::Completed));
    }

    #[tokio::test]
    async fn graph_step_once_from_start_to_complete() {
        let mut graph = StateGraph::<MessagesState>::new();
        graph
            .add_node(
                "node1",
                function_node("node1", |_s: &MessagesState| async move {
                    Ok(std::collections::HashMap::new())
                }),
            )
            .unwrap();
        graph.add_edge(START, "node1");
        graph.add_edge("node1", END);
        let compiled = graph.compile().unwrap();
        let state = MessagesState::new();
        let r = compiled.step_once(&state, START, None).await.unwrap();
        assert!(
            matches!(r, GraphStepOnceResult::Complete { .. }),
            "START -> node1 -> END: one step runs node1 and reaches END"
        );
    }
}
