use oris::langgraph::{
    function_node, InMemorySaver, MessagesState, RunnableConfig, StateGraph, END, START,
};
use oris::schemas::messages::Message;

/// Basic persistence example for LangGraph
///
/// This example demonstrates:
/// 1. Creating a graph with a checkpointer
/// 2. Invoking the graph with a thread_id
/// 3. Retrieving state and state history
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a simple node
    let mock_llm = function_node("mock_llm", |_state: &MessagesState| async move {
        use std::collections::HashMap;
        let mut update = HashMap::new();
        update.insert(
            "messages".to_string(),
            serde_json::to_value(vec![Message::new_ai_message("hello world")])?,
        );
        Ok(update)
    });

    // Build the graph
    let mut graph = StateGraph::<MessagesState>::new();
    graph.add_node("mock_llm", mock_llm)?;
    graph.add_edge(START, "mock_llm");
    graph.add_edge("mock_llm", END);

    // Create checkpointer
    let checkpointer = std::sync::Arc::new(InMemorySaver::new());

    // Compile with checkpointer
    let compiled = graph.compile_with_persistence(Some(checkpointer.clone()), None)?;

    // Invoke with config (includes thread_id)
    let config = RunnableConfig::with_thread_id("thread-1");
    let initial_state = MessagesState::with_messages(vec![Message::new_human_message("hi!")]);
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

    // Get the latest state
    let snapshot = compiled.get_state(&config).await?;
    println!("\nLatest checkpoint:");
    println!("  Thread ID: {}", snapshot.thread_id());
    println!("  Checkpoint ID: {:?}", snapshot.checkpoint_id());
    println!("  Next nodes: {:?}", snapshot.next);
    println!("  Created at: {}", snapshot.created_at);

    // Get state history
    let history = compiled.get_state_history(&config).await?;
    println!("\nState history ({} checkpoints):", history.len());
    for (i, snapshot) in history.iter().enumerate() {
        println!(
            "  {}: checkpoint_id={:?}, step={:?}",
            i + 1,
            snapshot.checkpoint_id(),
            snapshot.metadata.get("step")
        );
    }

    Ok(())
}
