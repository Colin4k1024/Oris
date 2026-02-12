#[cfg(feature = "sqlite-persistence")]
use oris::langgraph::{
    function_node, MessagesState, RunnableConfig, SqliteSaver, StateGraph, END, START,
};
use oris::schemas::messages::Message;
use std::fs;

/// SQLite persistence example for LangGraph
///
/// This example demonstrates:
/// 1. Creating a graph with SQLite checkpointer
/// 2. Persistent storage across process restarts
/// 3. Retrieving state from database
///
/// Note: This example requires the `sqlite-persistence` feature.
#[cfg(feature = "sqlite-persistence")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a temporary database file
    let db_path = "langgraph_checkpoints.db";

    // Remove existing database if it exists
    if fs::metadata(db_path).is_ok() {
        fs::remove_file(db_path)?;
    }

    // Create a simple node
    let mock_llm = function_node("mock_llm", |_state: &MessagesState| async move {
        use std::collections::HashMap;
        let mut update = HashMap::new();
        update.insert(
            "messages".to_string(),
            serde_json::to_value(vec![Message::new_ai_message("hello from SQLite")])?,
        );
        Ok(update)
    });

    // Build the graph
    let mut graph = StateGraph::<MessagesState>::new();
    graph.add_node("mock_llm", mock_llm)?;
    graph.add_edge(START, "mock_llm");
    graph.add_edge("mock_llm", END);

    // Create SQLite checkpointer
    let checkpointer = std::sync::Arc::new(SqliteSaver::new(db_path)?);

    // Compile with checkpointer
    let compiled = graph.compile_with_persistence(Some(checkpointer.clone()), None)?;

    // Invoke with config
    let config = RunnableConfig::with_thread_id("thread-sqlite-1");
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

    // Get state history from database
    let history = compiled.get_state_history(&config).await?;
    println!(
        "\nState history from database ({} checkpoints):",
        history.len()
    );
    for (i, snapshot) in history.iter().enumerate() {
        println!(
            "  {}: checkpoint_id={:?}, created_at={}",
            i + 1,
            snapshot.checkpoint_id(),
            snapshot.created_at
        );
    }

    // Clean up
    fs::remove_file(db_path)?;
    println!("\nDatabase file removed.");

    Ok(())
}

#[cfg(not(feature = "sqlite-persistence"))]
fn main() {
    eprintln!("This example requires the 'sqlite-persistence' feature");
    eprintln!(
        "Run with: cargo run --example langgraph_persistence_sqlite --features sqlite-persistence"
    );
}
