use oris::langgraph::{
    function_node, interrupt, Command, InMemorySaver, LangGraphError, MessagesState,
    RunnableConfig, StateGraph, StateOrCommand, END, START,
};
use oris::schemas::messages::Message;
use std::collections::HashMap;

/// Approval workflow example with interrupts
///
/// This example demonstrates a complete approval workflow where
/// execution pauses to ask for approval before proceeding.
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Approval node - pauses for human approval
    let approval_node = function_node("approval", |_state: &MessagesState| async move {
        // Extract action details from state (in a real scenario)
        let action_details = "Transfer $500 to account 12345";

        // Pause and ask for approval
        let approved = interrupt(serde_json::json!({
            "question": "Do you want to proceed with this action?",
            "details": action_details
        }))
        .await
        .map_err(LangGraphError::InterruptError)?;

        // Route based on the response
        let status = if approved.as_bool().unwrap_or(false) {
            "approved"
        } else {
            "rejected"
        };

        let mut update = HashMap::new();
        update.insert(
            "messages".to_string(),
            serde_json::to_value(vec![Message::new_ai_message(format!(
                "Action status: {}",
                status
            ))])?,
        );
        Ok(update)
    });

    // Build the graph
    let mut graph = StateGraph::<MessagesState>::new();
    graph.add_node("approval", approval_node)?;
    graph.add_edge(START, "approval");
    graph.add_edge("approval", END);

    // Create checkpointer
    let checkpointer = std::sync::Arc::new(InMemorySaver::new());
    let compiled = graph.compile_with_persistence(Some(checkpointer.clone()), None)?;

    let config = RunnableConfig::with_thread_id("approval-123");
    let initial_state = MessagesState::with_messages(vec![Message::new_human_message("start")]);

    // Initial run - hits the interrupt
    let initial = compiled
        .invoke_with_config_interrupt(StateOrCommand::State(initial_state), &config)
        .await?;

    if let Some(interrupts) = initial.interrupt {
        println!("Interrupted for approval:");
        for interrupt in &interrupts {
            println!("  {}", interrupt.value);
        }
    }

    // Resume with approval decision
    println!("\nResuming with approval: true");
    let resumed = compiled
        .invoke_with_config_interrupt(
            StateOrCommand::Command(Command::resume(serde_json::json!(true))),
            &config,
        )
        .await?;

    println!("\nFinal result:");
    for message in &resumed.state.messages {
        println!(
            "  {}: {}",
            message.message_type.to_string(),
            message.content
        );
    }

    Ok(())
}
