use oris::langgraph::{
    function_node, InMemorySaver, MessagesState, RunnableConfig, StateGraph, END, START,
};
use oris::schemas::messages::Message;

/// Replay example for LangGraph persistence
///
/// This example demonstrates:
/// 1. Executing a graph and saving checkpoints
/// 2. Replaying from a specific checkpoint
/// 3. Forking state at a checkpoint
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create nodes
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

    // Build the graph
    let mut graph = StateGraph::<MessagesState>::new();
    graph.add_node("node1", node1)?;
    graph.add_node("node2", node2)?;
    graph.add_edge(START, "node1");
    graph.add_edge("node1", "node2");
    graph.add_edge("node2", END);

    // Create checkpointer
    let checkpointer = std::sync::Arc::new(InMemorySaver::new());

    // Compile with checkpointer
    let compiled = graph.compile_with_persistence(Some(checkpointer.clone()), None)?;

    // First execution
    println!("=== First Execution ===");
    let config = RunnableConfig::with_thread_id("thread-replay-1");
    let initial_state = MessagesState::with_messages(vec![Message::new_human_message("start")]);
    let final_state = compiled
        .invoke_with_config(Some(initial_state), &config)
        .await?;

    println!("Final messages:");
    for message in &final_state.messages {
        println!(
            "  {}: {}",
            message.message_type.to_string(),
            message.content
        );
    }

    // Get state history
    let history = compiled.get_state_history(&config).await?;
    println!("\nCheckpoints created: {}", history.len());

    // Find a checkpoint to replay from (e.g., after node1)
    if let Some(checkpoint) = history
        .iter()
        .find(|s| s.metadata.get("node").and_then(|v| v.as_str()) == Some("node1"))
    {
        let checkpoint_id = checkpoint.checkpoint_id().unwrap().clone();
        println!("\n=== Replaying from checkpoint: {} ===", checkpoint_id);

        // Replay from checkpoint (None means resume from checkpoint)
        let replay_config = RunnableConfig::with_checkpoint("thread-replay-1", checkpoint_id);
        let replay_state = compiled
            .invoke_with_config(
                None, // None means resume from checkpoint
                &replay_config,
            )
            .await?;

        println!("Replay final messages:");
        for message in &replay_state.messages {
            println!(
                "  {}: {}",
                message.message_type.to_string(),
                message.content
            );
        }
    }

    // Example: Update state at a checkpoint (fork)
    println!("\n=== Forking state ===");
    if let Some(checkpoint) = history.first() {
        let checkpoint_id = checkpoint.checkpoint_id().unwrap().clone();
        let fork_config = RunnableConfig::with_checkpoint("thread-replay-1", checkpoint_id);

        // Update state with new values
        use std::collections::HashMap;
        let mut update = HashMap::new();
        update.insert(
            "messages".to_string(),
            serde_json::to_value(vec![Message::new_ai_message("Forked message")])?,
        );

        let forked_snapshot = compiled
            .update_state(&fork_config, &update, Some("fork_node"))
            .await?;
        println!(
            "Forked checkpoint ID: {:?}",
            forked_snapshot.checkpoint_id()
        );
        println!(
            "Forked state messages count: {}",
            forked_snapshot.values.messages.len()
        );
    }

    Ok(())
}
