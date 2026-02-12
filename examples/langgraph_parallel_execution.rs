use oris::langgraph::{
    function_node, DurabilityMode, InMemorySaver, MessagesState, RunnableConfig, StateGraph, END,
    START,
};
use oris::schemas::messages::Message;

/// Parallel execution example for LangGraph
///
/// This example demonstrates:
/// 1. Creating a graph with multiple nodes that can execute in parallel
/// 2. Using super-step execution model
/// 3. Different durability modes
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create nodes that can execute in parallel
    let node1 = function_node("node1", |_state: &MessagesState| async move {
        use std::collections::HashMap;
        let mut update = HashMap::new();
        update.insert(
            "messages".to_string(),
            serde_json::to_value(vec![Message::new_ai_message("Message from node1")])?,
        );
        Ok(update)
    });

    let node2 = function_node("node2", |_state: &MessagesState| async move {
        use std::collections::HashMap;
        let mut update = HashMap::new();
        update.insert(
            "messages".to_string(),
            serde_json::to_value(vec![Message::new_ai_message("Message from node2")])?,
        );
        Ok(update)
    });

    let node3 = function_node("node3", |_state: &MessagesState| async move {
        use std::collections::HashMap;
        let mut update = HashMap::new();
        update.insert(
            "messages".to_string(),
            serde_json::to_value(vec![Message::new_ai_message("Message from node3")])?,
        );
        Ok(update)
    });

    // Build the graph
    // node1 and node2 can execute in parallel (both from START)
    // node3 executes after node1 and node2 complete
    let mut graph = StateGraph::<MessagesState>::new();
    graph.add_node("node1", node1)?;
    graph.add_node("node2", node2)?;
    graph.add_node("node3", node3)?;

    // Both node1 and node2 start from START (parallel execution)
    graph.add_edge(START, "node1");
    graph.add_edge(START, "node2");

    // node3 executes after both node1 and node2
    graph.add_edge("node1", "node3");
    graph.add_edge("node2", "node3");
    graph.add_edge("node3", END);

    // Create checkpointer
    let checkpointer = std::sync::Arc::new(InMemorySaver::new());

    // Compile with checkpointer
    let compiled = graph.compile_with_persistence(Some(checkpointer.clone()), None)?;

    // Example 1: Execute with Sync durability mode
    println!("=== Example 1: Sync durability mode ===");
    let config = RunnableConfig::with_thread_id("thread-parallel-1");
    let initial_state = MessagesState::with_messages(vec![Message::new_human_message("start")]);
    let final_state = compiled
        .invoke_with_config_and_mode(Some(initial_state), &config, DurabilityMode::Sync)
        .await?;

    println!("Final messages count: {}", final_state.messages.len());
    for message in &final_state.messages {
        println!(
            "  {}: {}",
            message.message_type.to_string(),
            message.content
        );
    }

    // Example 2: Execute with Async durability mode
    println!("\n=== Example 2: Async durability mode ===");
    let config = RunnableConfig::with_thread_id("thread-parallel-2");
    let initial_state = MessagesState::with_messages(vec![Message::new_human_message("start")]);
    let final_state = compiled
        .invoke_with_config_and_mode(Some(initial_state), &config, DurabilityMode::Async)
        .await?;

    println!("Final messages count: {}", final_state.messages.len());

    // Example 3: Execute with Exit durability mode
    println!("\n=== Example 3: Exit durability mode ===");
    let config = RunnableConfig::with_thread_id("thread-parallel-3");
    let initial_state = MessagesState::with_messages(vec![Message::new_human_message("start")]);
    let final_state = compiled
        .invoke_with_config_and_mode(Some(initial_state), &config, DurabilityMode::Exit)
        .await?;

    println!("Final messages count: {}", final_state.messages.len());

    // Get state history to see checkpoints
    let history = compiled.get_state_history(&config).await?;
    println!("\nCheckpoints created: {}", history.len());
    for (i, snapshot) in history.iter().enumerate() {
        println!(
            "  {}: step={:?}, executed_nodes={:?}",
            i + 1,
            snapshot.metadata.get("step"),
            snapshot.metadata.get("executed_nodes")
        );
    }

    Ok(())
}
