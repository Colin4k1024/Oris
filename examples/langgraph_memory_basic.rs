use oris::langgraph::{
    function_node_with_store, InMemorySaver, InMemoryStore, LangGraphError, MessagesState,
    RunnableConfig, StateGraph, END, START,
};
use oris::schemas::messages::Message;

/// Basic memory example for LangGraph
///
/// This example demonstrates:
/// 1. Using store for long-term memory across threads
/// 2. Accessing config and store in nodes
/// 3. Storing and retrieving user-specific memories
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a node that uses store for memory
    let call_model = function_node_with_store(
        "call_model",
        |state: &MessagesState,
         config: &RunnableConfig,
         store: oris::langgraph::StoreBox| {
            let user_id = config.get_user_id().unwrap_or("default".to_string());
            let last_message = state
                .messages
                .last()
                .map(|m| m.content.clone())
                .unwrap_or_default();
            async move {
                use std::collections::HashMap;

                let namespace = ["memories", user_id.as_str()];
                let last_msg_ref = last_message.as_str();

                let memories = store
                    .search(&namespace, Some(last_msg_ref), Some(3))
                    .await
                    .map_err(|e| LangGraphError::ExecutionError(format!("Store error: {}", e)))?;

                // Build context from memories
                let memory_context: String = memories
                    .iter()
                    .map(|item| {
                        if let Some(data) = item.value.get("data") {
                            data.as_str().unwrap_or("").to_string()
                        } else {
                            String::new()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");

                // Check if user wants to remember something
                let last_msg_lower = last_message.to_lowercase();
                if last_msg_lower.contains("remember") {
                    // Extract memory from message (simplified)
                    let memory_text = last_message.replace("remember", "").trim().to_string();
                    if !memory_text.is_empty() {
                        let memory_id = format!("memory-{}", chrono::Utc::now().timestamp());
                        store
                            .put(
                                &namespace,
                                &memory_id,
                                serde_json::json!({"data": memory_text}),
                            )
                            .await
                            .map_err(|e| {
                                LangGraphError::ExecutionError(format!("Store error: {}", e))
                            })?;
                    }
                }

                // Generate response (simplified - in real app, call LLM)
                let response_text = if !memory_context.is_empty() {
                    format!(
                        "Based on your memories: {}. Response to: {}",
                        memory_context, last_message
                    )
                } else {
                    format!("Response to: {}", last_message)
                };

                let mut update = HashMap::new();
                update.insert(
                    "messages".to_string(),
                    serde_json::to_value(vec![Message::new_ai_message(response_text)])?,
                );
                Ok(update)
            }
        },
    );

    // Build the graph
    let mut graph = StateGraph::<MessagesState>::new();
    graph.add_node("call_model", call_model)?;
    graph.add_edge(START, "call_model");
    graph.add_edge("call_model", END);

    // Create checkpointer and store
    let checkpointer = std::sync::Arc::new(InMemorySaver::new());
    let store = std::sync::Arc::new(InMemoryStore::new());

    // Compile with checkpointer and store
    let compiled =
        graph.compile_with_persistence(Some(checkpointer.clone()), Some(store.clone()))?;

    // First conversation thread - user introduces themselves
    println!("=== Thread 1: User introduces themselves ===");
    let config1 = {
        let mut cfg = RunnableConfig::with_thread_id("thread-1");
        cfg.configurable
            .insert("user_id".to_string(), serde_json::json!("user-123"));
        cfg
    };

    let initial_state1 = MessagesState::with_messages(vec![Message::new_human_message(
        "Hi! Remember: my name is Bob",
    )]);

    let result1 = compiled
        .invoke_with_config(Some(initial_state1), &config1)
        .await?;
    println!("Response: {}", result1.messages.last().unwrap().content);

    // Second conversation thread - same user, different thread
    println!("\n=== Thread 2: Same user, different thread ===");
    let config2 = {
        let mut cfg = RunnableConfig::with_thread_id("thread-2");
        cfg.configurable
            .insert("user_id".to_string(), serde_json::json!("user-123"));
        cfg
    };

    let initial_state2 =
        MessagesState::with_messages(vec![Message::new_human_message("What is my name?")]);

    let result2 = compiled
        .invoke_with_config(Some(initial_state2), &config2)
        .await?;
    println!("Response: {}", result2.messages.last().unwrap().content);

    Ok(())
}
