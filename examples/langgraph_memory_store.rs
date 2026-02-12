use oris::langgraph::{
    function_node_with_store, InMemorySaver, InMemoryStore, LangGraphError, MessagesState,
    RunnableConfig, StateGraph, END, START,
};
use oris::schemas::messages::Message;

/// Store usage example for LangGraph
///
/// This example demonstrates:
/// 1. Storing user-specific data in the store
/// 2. Retrieving data across different threads
/// 3. Using namespaces to organize data
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a node that stores and retrieves data
    let process_node = function_node_with_store(
        "process",
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

                let namespace = ["user_data", user_id.as_str()];

                // Store user preference if mentioned
                if last_message.contains("I like") {
                    let preference = last_message.replace("I like", "").trim().to_string();
                    if !preference.is_empty() {
                        store
                            .put(
                                &namespace,
                                "preference",
                                serde_json::json!({"value": preference}),
                            )
                            .await
                            .map_err(|e| {
                                LangGraphError::ExecutionError(format!("Store error: {}", e))
                            })?;
                    }
                }

                // Retrieve stored preference
                let stored_pref = store
                    .get(&namespace, "preference")
                    .await
                    .map_err(|e| LangGraphError::ExecutionError(format!("Store error: {}", e)))?;
                let pref_text = stored_pref
                    .and_then(|item| {
                        item.value
                            .get("value")
                            .and_then(|v| v.as_str())
                            .map(String::from)
                    })
                    .unwrap_or_else(|| "no preference".to_string());

                // Generate response
                let response = if pref_text != "no preference" {
                    format!(
                        "I remember you like {}. You said: {}",
                        pref_text, last_message
                    )
                } else {
                    format!(
                        "You said: {}. I'll remember your preferences.",
                        last_message
                    )
                };

                let mut update = HashMap::new();
                update.insert(
                    "messages".to_string(),
                    serde_json::to_value(vec![Message::new_ai_message(response)])?,
                );
                Ok(update)
            }
        },
    );

    // Build the graph
    let mut graph = StateGraph::<MessagesState>::new();
    graph.add_node("process", process_node)?;
    graph.add_edge(START, "process");
    graph.add_edge("process", END);

    // Create checkpointer and store
    let checkpointer = std::sync::Arc::new(InMemorySaver::new());
    let store = std::sync::Arc::new(InMemoryStore::new());

    // Compile with checkpointer and store
    let compiled =
        graph.compile_with_persistence(Some(checkpointer.clone()), Some(store.clone()))?;

    // First interaction - store preference
    println!("=== First interaction: Store preference ===");
    let config1 = {
        let mut cfg = RunnableConfig::with_thread_id("thread-1");
        cfg.configurable
            .insert("user_id".to_string(), serde_json::json!("alice"));
        cfg
    };

    let state1 = MessagesState::with_messages(vec![Message::new_human_message("I like pizza")]);

    let result1 = compiled.invoke_with_config(Some(state1), &config1).await?;
    println!("Response: {}", result1.messages.last().unwrap().content);

    // Second interaction - retrieve preference
    println!("\n=== Second interaction: Retrieve preference ===");
    let config2 = {
        let mut cfg = RunnableConfig::with_thread_id("thread-2");
        cfg.configurable
            .insert("user_id".to_string(), serde_json::json!("alice"));
        cfg
    };

    let state2 = MessagesState::with_messages(vec![Message::new_human_message("What do I like?")]);

    let result2 = compiled.invoke_with_config(Some(state2), &config2).await?;
    println!("Response: {}", result2.messages.last().unwrap().content);

    Ok(())
}
