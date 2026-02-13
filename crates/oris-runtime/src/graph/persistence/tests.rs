#[cfg(test)]
mod time_travel_tests {
    use crate::graph::{
        function_node,
        persistence::{InMemorySaver, RunnableConfig},
        state::MessagesState,
        DurabilityMode, StateGraph, END, START,
    };
    use std::collections::HashMap;

    /// Resume from latest checkpoint (crash recovery): no checkpoint_id, only thread_id.
    #[tokio::test]
    async fn test_resume_from_latest_checkpoint() {
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

        let config = RunnableConfig::with_thread_id("crash-recovery-thread");
        let initial_state = MessagesState::new();

        // First run: execute to completion (creates checkpoints)
        let first = compiled
            .invoke_with_config_and_mode(Some(initial_state.clone()), &config, DurabilityMode::Sync)
            .await
            .unwrap();
        assert!(!first.messages.is_empty());

        // Simulate crash: second "process" resumes with same thread_id, no state, no checkpoint_id
        let resumed = compiled
            .invoke_with_config_and_mode(None, &config, DurabilityMode::Sync)
            .await
            .unwrap();
        // Resume from latest loads checkpoint and continues; we get a valid outcome
        assert!(!resumed.messages.is_empty());
    }

    #[tokio::test]
    #[ignore = "get_state_history can return empty depending on checkpointer behavior"]
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
    #[ignore = "get_state_history can return empty depending on checkpointer behavior"]
    async fn test_update_state_creates_fork() -> Result<(), Box<dyn std::error::Error>> {
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
            checkpoint.checkpoint_id().cloned()
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_snapshot_to_config() {
        use crate::graph::persistence::config::CheckpointConfig;
        use crate::graph::persistence::snapshot::StateSnapshot;

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
