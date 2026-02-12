#[cfg(test)]
mod memory_tests {
    use crate::langgraph::{
        function_node_with_config, function_node_with_store,
        persistence::{InMemorySaver, InMemoryStore, RunnableConfig, Store},
        state::MessagesState,
        StateGraph, END, START,
    };
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_node_with_config() {
        let node = function_node_with_config(
            "test_node",
            |_state: &MessagesState, config: &RunnableConfig| async move {
                let thread_id = config.get_thread_id().unwrap();
                let mut update = HashMap::new();
                update.insert("thread_id".to_string(), serde_json::to_value(thread_id)?);
                Ok(update)
            },
        );

        let state = MessagesState::new();
        let config = RunnableConfig::with_thread_id("test-thread");
        let result = node
            .invoke_with_context(&state, Some(&config), None)
            .await
            .unwrap();
        assert!(result.contains_key("thread_id"));
    }

    #[tokio::test]
    async fn test_node_with_store() {
        let store = std::sync::Arc::new(InMemoryStore::new());

        let node = function_node_with_store(
            "test_node",
            |_state: &MessagesState, _config: &RunnableConfig, store: std::sync::Arc<dyn Store>| async move {
                use crate::langgraph::error::LangGraphError;

                // Store a value
                store
                    .put(
                        &["test", "namespace"],
                        "key1",
                        serde_json::json!({"value": "test_data"}),
                    )
                    .await
                    .map_err(|e| LangGraphError::ExecutionError(format!("Store error: {}", e)))?;

                // Retrieve the value
                let item = store
                    .get(&["test", "namespace"], "key1")
                    .await
                    .map_err(|e| LangGraphError::ExecutionError(format!("Store error: {}", e)))?;
                assert!(item.is_some());
                assert_eq!(
                    item.unwrap().value.get("value").unwrap().as_str().unwrap(),
                    "test_data"
                );

                let mut update = HashMap::new();
                update.insert("result".to_string(), serde_json::json!("success"));
                Ok(update)
            },
        );

        let state = MessagesState::new();
        let config = RunnableConfig::with_thread_id("test-thread");
        let result = node
            .invoke_with_context(&state, Some(&config), Some(store))
            .await
            .unwrap();
        assert_eq!(result.get("result").unwrap().as_str().unwrap(), "success");
    }

    #[tokio::test]
    async fn test_memory_across_threads() {
        let checkpointer = std::sync::Arc::new(InMemorySaver::new());
        let store = std::sync::Arc::new(InMemoryStore::new());

        let node = function_node_with_store(
            "memory_node",
            |state: &MessagesState, config: &RunnableConfig, store: std::sync::Arc<dyn Store>| {
                let user_id = config.get_user_id().unwrap_or("default".to_string());
                let messages_is_empty = state.messages.is_empty();
                async move {
                    use crate::langgraph::error::LangGraphError;

                    let namespace = ["memories", user_id.as_str()];

                    // Store a memory (only on first call to avoid duplicates in test)
                    if messages_is_empty {
                        store
                            .put(
                                &namespace,
                                "memory-1",
                                serde_json::json!({"data": "User likes pizza"}),
                            )
                            .await
                            .map_err(|e| {
                                LangGraphError::ExecutionError(format!("Store error: {}", e))
                            })?;
                    }

                    // Search for memories
                    let memories = store
                        .search(&namespace, Some("pizza"), None)
                        .await
                        .map_err(|e| {
                            LangGraphError::ExecutionError(format!("Store error: {}", e))
                        })?;

                    let mut update = HashMap::new();
                    update.insert(
                        "memories_found".to_string(),
                        serde_json::json!(memories.len()),
                    );
                    Ok(update)
                }
            },
        );

        let mut graph = StateGraph::<MessagesState>::new();
        graph.add_node("memory_node", node).unwrap();
        graph.add_edge(START, "memory_node");
        graph.add_edge("memory_node", END);

        let compiled = graph
            .compile_with_persistence(Some(checkpointer), Some(store.clone()))
            .unwrap();

        // First thread
        let config1 = {
            let mut cfg = RunnableConfig::with_thread_id("thread-1");
            cfg.configurable
                .insert("user_id".to_string(), serde_json::json!("user-123"));
            cfg
        };
        let state1 = MessagesState::new();
        let _result1 = compiled
            .invoke_with_config(Some(state1), &config1)
            .await
            .unwrap();

        // Second thread - same user, should access same memories
        let config2 = {
            let mut cfg = RunnableConfig::with_thread_id("thread-2");
            cfg.configurable
                .insert("user_id".to_string(), serde_json::json!("user-123"));
            cfg
        };
        let state2 = MessagesState::new();
        let result2 = compiled
            .invoke_with_config(Some(state2), &config2)
            .await
            .unwrap();

        // Verify memory was found
        let state_json = serde_json::to_value(&result2).unwrap();
        let memories_found = state_json.get("memories_found").unwrap().as_u64().unwrap();
        assert_eq!(memories_found, 1);
    }

    #[tokio::test]
    async fn test_store_semantic_search_support() {
        let store = InMemoryStore::new();
        assert_eq!(store.supports_semantic_search(), false);
        assert_eq!(store.embedding_dims(), None);
    }
}
