#[cfg(test)]
mod tests {
    use crate::langgraph::{
        error::LangGraphError,
        function_node,
        interrupts::{interrupt, set_interrupt_context, Command, InterruptContext},
        persistence::{InMemorySaver, RunnableConfig},
        state::MessagesState,
        StateGraph, END, START,
    };

    #[tokio::test]
    async fn test_interrupt_basic() {
        // Test interrupt without resume value
        let ctx = InterruptContext::new();
        let result = set_interrupt_context(ctx, async { interrupt("test interrupt").await }).await;

        assert!(result.is_err());
        if let Err(e) = result {
            assert_eq!(e.value(), &serde_json::json!("test interrupt"));
        }
    }

    #[tokio::test]
    async fn test_interrupt_with_resume() {
        // Test interrupt with resume value
        let ctx = InterruptContext::with_resume_value(serde_json::json!("resume_value"));
        let result = set_interrupt_context(ctx, async { interrupt("test interrupt").await }).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), serde_json::json!("resume_value"));
    }

    #[tokio::test]
    async fn test_command_resume() {
        let cmd = Command::resume(true);
        assert!(cmd.is_resume());
        assert_eq!(cmd.resume_value(), Some(&serde_json::json!(true)));
    }

    #[tokio::test]
    async fn test_command_goto() {
        let cmd = Command::goto("node1");
        assert!(cmd.is_goto());
        assert_eq!(cmd.goto_node(), Some("node1"));
    }

    #[tokio::test]
    async fn test_interrupt_in_node() {
        let approval_node = function_node("approval", |_state: &MessagesState| async move {
            let approved = interrupt("Approve?")
                .await
                .map_err(|e| LangGraphError::InterruptError(e))?;

            use std::collections::HashMap;
            let mut update = HashMap::new();
            update.insert(
                "messages".to_string(),
                serde_json::to_value(vec![crate::schemas::messages::Message::new_ai_message(
                    format!("Approved: {}", approved),
                )])?,
            );
            Ok(update)
        });

        let mut graph = StateGraph::<MessagesState>::new();
        graph.add_node("approval", approval_node).unwrap();
        graph.add_edge(START, "approval");
        graph.add_edge("approval", END);

        let checkpointer = std::sync::Arc::new(InMemorySaver::new());
        let compiled = graph
            .compile_with_persistence(Some(checkpointer), None)
            .unwrap();

        let config = RunnableConfig::with_thread_id("test-thread");
        let initial_state = MessagesState::new();

        // First call - should interrupt
        let result = compiled
            .invoke_with_config_interrupt(initial_state, &config)
            .await
            .unwrap();
        assert!(result.has_interrupt());

        // Resume with approval
        let resumed = compiled
            .invoke_with_config_interrupt(Command::resume(true), &config)
            .await
            .unwrap();

        assert!(!resumed.has_interrupt());
        assert_eq!(resumed.state.messages.len(), 1);
    }
}
