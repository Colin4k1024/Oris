use oris::langgraph::{
    function_node, interrupt, Command, InMemorySaver, MessagesState, RunnableConfig, StateGraph,
    StateOrCommand, END, START,
};
use oris::schemas::messages::Message;
use std::collections::HashMap;

/// Interrupt example for LangGraph
///
/// This example demonstrates:
/// 1. Using interrupt() to pause execution
/// 2. Resuming with Command::resume()
/// 3. Approval workflow pattern
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create an approval node that uses interrupt
    let approval_node = function_node("approval", |_state: &MessagesState| async move {
        use oris::langgraph::LangGraphError;

        // Pause and ask for approval
        let approved = interrupt("Do you approve this action?")
            .await
            .map_err(LangGraphError::InterruptError)?;

        // When resumed, `approved` contains the resume value
        let mut update = HashMap::new();
        update.insert(
            "messages".to_string(),
            serde_json::to_value(vec![Message::new_ai_message(format!(
                "Approval result: {}",
                approved
            ))])?,
        );
        Ok(update)
    });

    // Build the graph
    let mut graph = StateGraph::<MessagesState>::new();
    graph.add_node("approval", approval_node)?;
    graph.add_edge(START, "approval");
    graph.add_edge("approval", END);

    // Create checkpointer (required for interrupts)
    let checkpointer = std::sync::Arc::new(InMemorySaver::new());

    // Compile with checkpointer
    let compiled = graph.compile_with_persistence(Some(checkpointer.clone()), None)?;

    // Initial run - hits the interrupt and pauses
    let config = RunnableConfig::with_thread_id("thread-1");
    let initial_state = MessagesState::with_messages(vec![Message::new_human_message("start")]);

    let result = compiled
        .invoke_with_config_interrupt(StateOrCommand::State(initial_state), &config)
        .await?;

    // Check what was interrupted
    if let Some(interrupts) = result.interrupt {
        println!("Interrupted: {:?}", interrupts);
        if let Some(interrupt) = interrupts.first() {
            println!("Interrupt value: {}", interrupt.value);
        }
    }

    // Resume with the human's response
    let resumed = compiled
        .invoke_with_config_interrupt(
            StateOrCommand::Command(Command::resume(serde_json::json!(true))),
            &config,
        )
        .await?;

    println!(
        "Final state messages count: {}",
        resumed.state.messages.len()
    );
    for message in &resumed.state.messages {
        println!(
            "  {}: {}",
            message.message_type.to_string(),
            message.content
        );
    }

    Ok(())
}
