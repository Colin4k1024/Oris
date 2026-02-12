use futures::StreamExt;
use oris::langgraph::{
    function_node, MessagesState, StateGraph, StreamChunk, StreamMode, END, START,
};
use oris::schemas::messages::Message;

/// Streaming example for LangGraph
///
/// This example demonstrates:
/// 1. Basic streaming with different modes
/// 2. Streaming state updates
/// 3. Streaming full state values
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create nodes
    let node1 = function_node("node1", |_state: &MessagesState| async move {
        use std::collections::HashMap;
        let mut update = HashMap::new();
        update.insert(
            "messages".to_string(),
            serde_json::to_value(vec![Message::new_ai_message("Hello from node1")])?,
        );
        Ok(update)
    });

    let node2 = function_node("node2", |_state: &MessagesState| async move {
        use std::collections::HashMap;
        let mut update = HashMap::new();
        update.insert(
            "messages".to_string(),
            serde_json::to_value(vec![Message::new_ai_message("Hello from node2")])?,
        );
        Ok(update)
    });

    // Build the graph
    let mut graph = StateGraph::<MessagesState>::new();
    graph.add_node("node1", node1)?;
    graph.add_node("node2", node2)?;

    graph.add_edge(START, "node1");
    graph.add_edge("node1", "node2");
    graph.add_edge("node2", END);

    let compiled = graph.compile()?;
    let initial_state = MessagesState::with_messages(vec![Message::new_human_message("start")]);

    // Example 1: Stream with updates mode
    println!("=== Example 1: Updates mode ===");
    let mut stream = compiled.stream_with_mode(initial_state.clone(), StreamMode::Updates);
    while let Some(chunk) = stream.next().await {
        match chunk {
            StreamChunk::Updates { node, update } => {
                println!("Node: {}, Update: {:?}", node, update);
            }
            _ => {}
        }
    }

    // Example 2: Stream with values mode
    println!("\n=== Example 2: Values mode ===");
    let mut stream = compiled.stream_with_mode(initial_state.clone(), StreamMode::Values);
    while let Some(chunk) = stream.next().await {
        match chunk {
            StreamChunk::Values { state } => {
                println!("State messages count: {}", state.messages.len());
            }
            _ => {}
        }
    }

    // Example 3: Stream with multiple modes
    println!("\n=== Example 3: Multiple modes ===");
    let mut stream =
        compiled.stream_with_modes(initial_state, vec![StreamMode::Updates, StreamMode::Values]);
    while let Some((mode, chunk)) = stream.next().await {
        println!("Mode: {:?}, Chunk: {:?}", mode, chunk.mode());
    }

    Ok(())
}
