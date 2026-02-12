use oris::langgraph::{
    function_node, InMemorySaver, MessagesState, RunnableConfig, StateGraph, END, START,
};
use oris::schemas::messages::Message;
use std::collections::HashMap;

/// Time-travel fork exploration example
///
/// This example demonstrates exploring different execution paths
/// by forking from a checkpoint and trying different state values.
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a node that processes a topic
    let process_topic = function_node("process_topic", |state: &MessagesState| {
        let topic = state
            .messages
            .last()
            .map(|m| m.content.clone())
            .unwrap_or_else(|| "default".to_string());
        async move {
            use std::collections::HashMap;
            let mut update = HashMap::new();
            update.insert(
                "messages".to_string(),
                serde_json::to_value(vec![Message::new_ai_message(format!(
                    "Processed: {}",
                    topic
                ))])?,
            );
            Ok(update)
        }
    });

    // Build the graph
    let mut graph = StateGraph::<MessagesState>::new();
    graph.add_node("process_topic", process_topic)?;
    graph.add_edge(START, "process_topic");
    graph.add_edge("process_topic", END);

    let checkpointer = std::sync::Arc::new(InMemorySaver::new());
    let compiled = graph.compile_with_persistence(Some(checkpointer.clone()), None)?;

    let config = RunnableConfig::with_thread_id("fork-demo");

    // Initial execution
    let initial_state = MessagesState::with_messages(vec![Message::new_human_message("initial")]);
    let _final_state = compiled
        .invoke_with_config(Some(initial_state), &config)
        .await?;

    // Get history and find a checkpoint to fork from
    let history = compiled.get_state_history(&config).await?;
    let fork_point = history.first().ok_or("No checkpoints found")?;

    println!("=== Forking from checkpoint ===");
    println!("Original checkpoint_id: {:?}", fork_point.checkpoint_id());

    // Fork 1: Try with "topic1"
    println!("\n--- Fork 1: topic1 ---");
    let mut updates1 = HashMap::new();
    updates1.insert(
        "messages".to_string(),
        serde_json::to_value(vec![Message::new_human_message("topic1")])?,
    );
    let snapshot1 = compiled
        .update_state(&fork_point.to_config(), &updates1, None)
        .await?;
    let result1 = compiled
        .invoke_with_config(None, &snapshot1.to_config())
        .await?;
    println!("Result: {:?}", result1.messages.last().map(|m| &m.content));

    // Fork 2: Try with "topic2"
    println!("\n--- Fork 2: topic2 ---");
    let mut updates2 = HashMap::new();
    updates2.insert(
        "messages".to_string(),
        serde_json::to_value(vec![Message::new_human_message("topic2")])?,
    );
    let snapshot2 = compiled
        .update_state(&fork_point.to_config(), &updates2, None)
        .await?;
    let result2 = compiled
        .invoke_with_config(None, &snapshot2.to_config())
        .await?;
    println!("Result: {:?}", result2.messages.last().map(|m| &m.content));

    // Check fork history
    println!("\n=== Fork history ===");
    let all_history = compiled.get_state_history(&config).await?;
    println!("Total checkpoints: {}", all_history.len());
    for (i, snapshot) in all_history.iter().enumerate() {
        println!(
            "  {}: checkpoint_id={:?}, parent={:?}",
            i,
            snapshot.checkpoint_id(),
            snapshot
                .parent_config
                .as_ref()
                .and_then(|p| p.checkpoint_id.as_ref())
        );
    }

    Ok(())
}
