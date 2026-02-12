use oris::langgraph::{
    function_node, InMemorySaver, MessagesState, RunnableConfig, StateGraph, END, START,
};
use oris::schemas::messages::Message;
use std::collections::HashMap;

/// Time-travel example for LangGraph
///
/// This example demonstrates:
/// 1. Running a graph and getting execution history
/// 2. Selecting a checkpoint from history
/// 3. Optionally updating the state
/// 4. Resuming execution from the checkpoint
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create nodes
    let generate_topic = function_node("generate_topic", |_state: &MessagesState| async move {
        use std::collections::HashMap;
        let mut update = HashMap::new();
        update.insert(
            "messages".to_string(),
            serde_json::to_value(vec![Message::new_ai_message(
                "Topic: The Secret Life of Socks in the Dryer",
            )])?,
        );
        Ok(update)
    });

    let write_joke = function_node("write_joke", |state: &MessagesState| {
        let topic = state
            .messages
            .last()
            .map(|m| m.content.clone())
            .unwrap_or_else(|| "default topic".to_string());
        async move {
            use std::collections::HashMap;
            let mut update = HashMap::new();
            update.insert(
                "messages".to_string(),
                serde_json::to_value(vec![Message::new_ai_message(format!(
                    "Joke about {}: Why did the sock go to therapy? Because it had separation anxiety!",
                    topic
                ))])?,
            );
            Ok(update)
        }
    });

    // Build the graph
    let mut graph = StateGraph::<MessagesState>::new();
    graph.add_node("generate_topic", generate_topic)?;
    graph.add_node("write_joke", write_joke)?;

    graph.add_edge(START, "generate_topic");
    graph.add_edge("generate_topic", "write_joke");
    graph.add_edge("write_joke", END);

    // Create checkpointer (required for time-travel)
    let checkpointer = std::sync::Arc::new(InMemorySaver::new());
    let compiled = graph.compile_with_persistence(Some(checkpointer.clone()), None)?;

    // Step 1: Run the graph
    let config = RunnableConfig::with_thread_id("time-travel-demo");
    let initial_state = MessagesState::new();
    let final_state = compiled
        .invoke_with_config(Some(initial_state), &config)
        .await?;

    println!("=== Step 1: Initial execution ===");
    for message in &final_state.messages {
        println!(
            "  {}: {}",
            message.message_type.to_string(),
            message.content
        );
    }

    // Step 2: Get state history
    println!("\n=== Step 2: Get state history ===");
    let history = compiled.get_state_history(&config).await?;
    println!("Found {} checkpoints", history.len());

    for (i, snapshot) in history.iter().enumerate() {
        println!(
            "  Checkpoint {}: next={:?}, checkpoint_id={:?}",
            i,
            snapshot.next,
            snapshot.checkpoint_id()
        );
    }

    // Step 3: Select a checkpoint (the one before write_joke)
    // States are in chronological order (oldest first)
    let selected_checkpoint = history
        .iter()
        .find(|s| s.next.contains(&"write_joke".to_string()))
        .ok_or("Checkpoint not found")?;

    println!("\n=== Step 3: Selected checkpoint ===");
    println!("  Next nodes: {:?}", selected_checkpoint.next);
    println!(
        "  Topic: {:?}",
        selected_checkpoint
            .values
            .messages
            .last()
            .map(|m| &m.content)
    );

    // Step 4: Optionally update the state
    println!("\n=== Step 4: Update state ===");
    let mut state_updates = HashMap::new();
    state_updates.insert(
        "messages".to_string(),
        serde_json::to_value(vec![Message::new_ai_message("Topic: chickens")])?,
    );

    let updated_snapshot = compiled
        .update_state(&selected_checkpoint.to_config(), &state_updates, None)
        .await?;

    println!(
        "  Updated checkpoint_id: {:?}",
        updated_snapshot.checkpoint_id()
    );

    // Step 5: Resume execution from the updated checkpoint
    println!("\n=== Step 5: Resume from checkpoint ===");
    let resumed_state = compiled
        .invoke_with_config(
            None, // None means resume from checkpoint
            &updated_snapshot.to_config(),
        )
        .await?;

    println!("Resumed execution result:");
    for message in &resumed_state.messages {
        println!(
            "  {}: {}",
            message.message_type.to_string(),
            message.content
        );
    }

    Ok(())
}
