//! Run the kernel via KernelRunner from async code (no deadlock).
//!
//! Run with: cargo run -p oris-runtime --example kernel_runner_async

use oris_runtime::graph::{
    function_node, CompiledGraph, GraphStepFnAdapter, GraphStepReducer, GraphStepState,
    MessagesState, StateGraph, END, START,
};
use oris_runtime::kernel::driver::{Kernel, RunStatus};
use oris_runtime::kernel::event_store::InMemoryEventStore;
use oris_runtime::kernel::runner::KernelRunner;
use oris_runtime::kernel::stubs::{AllowAllPolicy, NoopActionExecutor};
use oris_runtime::schemas::messages::Message;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
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

    let compiled: Arc<CompiledGraph<MessagesState>> = Arc::new(graph.compile().unwrap());
    let adapter = GraphStepFnAdapter::new(compiled);

    let kernel: Kernel<GraphStepState<MessagesState>> = Kernel {
        events: Box::new(InMemoryEventStore::new()),
        snaps: None,
        reducer: Box::new(GraphStepReducer),
        exec: Box::new(NoopActionExecutor),
        step: Box::new(adapter),
        policy: Box::new(AllowAllPolicy),
        effect_sink: None,
        mode: oris_runtime::kernel::KernelMode::Normal,
    };

    let runner = KernelRunner::new(kernel);
    let run_id = "async-run".to_string();
    let initial = GraphStepState::new(MessagesState::with_messages(vec![
        Message::new_human_message("Hello from async"),
    ]));

    let status = runner.run_until_blocked_async(&run_id, initial).await?;
    assert!(matches!(status, RunStatus::Completed));
    println!("Run completed via KernelRunner::run_until_blocked_async");
    Ok(())
}
