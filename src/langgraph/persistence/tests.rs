#[cfg(test)]
mod time_travel_tests {
    use crate::langgraph::{
        function_node,
        persistence::{InMemorySaver, RunnableConfig},
        state::MessagesState,
        StateGraph, END, START,
    };
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_time_travel_resume_from_checkpoint() {
        let node1 = function_node("node1", |_state: &MessagesState| async move {
            let mut update = HashMap::new();
            update.insert(
                "messages".to_string(),
                serde_json::to_value(vec![crate::schemas::messages::Message::new_ai_message(
                    "Node1",
                )])?,
            );
            Ok(update)
        });

        let mut graph = StateGraph::<MessagesState>::new();
        graph.add_node("node1", node1).unwrap();
        graph.add_edge(START, "node1");
        graph.add_edge("node1", END);

        let checkpointer = std::sync::Arc::new(InMemorySaver::new());
        let compiled = graph
            .compile_with_persistence(Some(checkpointer), None)
            .unwrap();

        let config = RunnableConfig::with_thread_id("test-thread");
        let initial_state = MessagesState::new();

        // Initial execution
        let _result = compiled
            .invoke_with_config(Some(initial_state), &config)
            .await
            .unwrap();

        // Get history
        let history = compiled.get_state_history(&config).await.unwrap();
        assert!(!history.is_empty());

        // Resume from first checkpoint
        let checkpoint = &history[0];
        let resumed = compiled
            .invoke_with_config(None, &checkpoint.to_config())
            .await
            .unwrap();

        assert!(!resumed.messages.is_empty());
    }

    #[tokio::test]
    async fn test_update_state_creates_fork() {
        let node1 = function_node("node1", |_state: &MessagesState| async move {
            let mut update = HashMap::new();
            update.insert(
                "messages".to_string(),
                serde_json::to_value(vec![crate::schemas::messages::Message::new_ai_message(
                    "Node1",
                )])?,
            );
            Ok(update)
        });

        let mut graph = StateGraph::<MessagesState>::new();
        graph.add_node("node1", node1).unwrap();
        graph.add_edge(START, "node1");
        graph.add_edge("node1", END);

        let checkpointer = std::sync::Arc::new(InMemorySaver::new());
        let compiled = graph
            .compile_with_persistence(Some(checkpointer), None)
            .unwrap();

        let config = RunnableConfig::with_thread_id("fork-test");
        let initial_state = MessagesState::new();

        // Initial execution
        let _result = compiled
            .invoke_with_config(Some(initial_state), &config)
            .await
            .unwrap();

        // Get a checkpoint
        let history = compiled.get_state_history(&config).await.unwrap();
        let checkpoint = &history[0];

        // Update state
        let mut updates = HashMap::new();
        updates.insert(
            "messages".to_string(),
            serde_json::to_value(vec![crate::schemas::messages::Message::new_ai_message(
                "Updated",
            )])?,
        );

        let updated = compiled
            .update_state(&checkpoint.to_config(), &updates, None)
            .await
            .unwrap();

        // Check that new checkpoint has parent
        assert!(updated.parent_config.is_some());
        assert_eq!(
            updated.parent_config.as_ref().unwrap().checkpoint_id,
            checkpoint.checkpoint_id()
        );
    }

    #[tokio::test]
    async fn test_snapshot_to_config() {
        use crate::langgraph::persistence::config::CheckpointConfig;
        use crate::langgraph::persistence::snapshot::StateSnapshot;

        let state = MessagesState::new();
        let config = CheckpointConfig::new("thread-1");
        let snapshot = StateSnapshot::new(state, vec![], config);

        let runnable_config = snapshot.to_config();
        assert_eq!(
            runnable_config.get_thread_id(),
            Some("thread-1".to_string())
        );
    }
}
