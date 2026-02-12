use oris::langgraph::{
    function_node, interrupt, Command, InMemorySaver, LangGraphError, MessagesState,
    RunnableConfig, StateGraph, StateOrCommand, END, START,
};
use oris::schemas::messages::Message;
use std::collections::HashMap;

/// Review and edit example with interrupts
///
/// This example demonstrates how to use interrupts to let humans
/// review and edit content before continuing.
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Review node - pauses for human review and editing
    let review_node = function_node("review", |_state: &MessagesState| async move {
        // Simulate generated content (in a real scenario, this would come from an LLM)
        let generated_text = "This is the initial draft that needs review.";

        // Pause and show the content for review
        let edited_content = interrupt(serde_json::json!({
            "instruction": "Review and edit this content",
            "content": generated_text
        }))
        .await
        .map_err(LangGraphError::InterruptError)?;

        // Extract the edited content
        let edited = if let Some(content) = edited_content.as_str() {
            content.to_string()
        } else {
            edited_content.to_string()
        };

        // Update the state with the edited version
        let mut update = HashMap::new();
        update.insert(
            "messages".to_string(),
            serde_json::to_value(vec![Message::new_ai_message(format!(
                "Edited content: {}",
                edited
            ))])?,
        );
        Ok(update)
    });

    // Build the graph
    let mut graph = StateGraph::<MessagesState>::new();
    graph.add_node("review", review_node)?;
    graph.add_edge(START, "review");
    graph.add_edge("review", END);

    // Create checkpointer
    let checkpointer = std::sync::Arc::new(InMemorySaver::new());
    let compiled = graph.compile_with_persistence(Some(checkpointer.clone()), None)?;

    let config = RunnableConfig::with_thread_id("review-42");
    let initial_state = MessagesState::with_messages(vec![Message::new_human_message("start")]);

    // Initial run - hits the interrupt
    let initial = compiled
        .invoke_with_config_interrupt(StateOrCommand::State(initial_state), &config)
        .await?;

    if let Some(interrupts) = initial.interrupt {
        println!("Interrupted for review:");
        for interrupt in &interrupts {
            println!("  {}", interrupt.value);
        }
    }

    // Resume with the edited text
    println!("\nResuming with edited text");
    let resumed = compiled
        .invoke_with_config_interrupt(
            StateOrCommand::Command(Command::resume(serde_json::json!(
                "Improved draft after review"
            ))),
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
