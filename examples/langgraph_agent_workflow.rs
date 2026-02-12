use oris::langgraph::{function_node, MessagesState, StateGraph, END, START};
use oris::schemas::messages::Message;
use std::collections::HashMap;

/// Complex agent workflow example for LangGraph
///
/// This example demonstrates:
/// 1. Multiple nodes in a workflow
/// 2. Conditional edges for routing
/// 3. State updates and merging
/// 4. Streaming execution
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Node 1: Process input
    let process_input = function_node("process_input", |state: &MessagesState| {
        let msg_content = state
            .messages
            .last()
            .map(|m| m.content.clone())
            .unwrap_or_default();
        async move {
            let mut update = HashMap::new();
            if !msg_content.is_empty() {
                let processed = format!("Processed: {}", msg_content);
                update.insert(
                    "messages".to_string(),
                    serde_json::to_value(vec![Message::new_ai_message(processed)])?,
                );
            }
            Ok(update)
        }
    });

    // Node 2: Decide next action (condition node)
    let decide_action = function_node("decide_action", |state: &MessagesState| {
        let needs_help = state
            .messages
            .iter()
            .any(|msg| msg.content.to_lowercase().contains("help"));
        let action = if needs_help { "help" } else { "respond" };
        async move {
            let mut update = HashMap::new();
            update.insert("action".to_string(), serde_json::to_value(action)?);
            Ok(update)
        }
    });

    // Node 3: Provide help
    let provide_help = function_node("provide_help", |_state: &MessagesState| async move {
        let mut update = HashMap::new();
        update.insert(
            "messages".to_string(),
            serde_json::to_value(vec![Message::new_ai_message(
                "I can help you with various tasks. Just ask me anything!",
            )])?,
        );
        Ok(update)
    });

    // Node 4: Generate response
    let generate_response = function_node("generate_response", |state: &MessagesState| {
        let msg_content = state
            .messages
            .last()
            .map(|m| m.content.clone())
            .unwrap_or_default();
        async move {
            let mut update = HashMap::new();
            if !msg_content.is_empty() {
                let response = format!("You said: {}", msg_content);
                update.insert(
                    "messages".to_string(),
                    serde_json::to_value(vec![Message::new_ai_message(response)])?,
                );
            }
            Ok(update)
        }
    });

    // Build the graph
    let mut graph = StateGraph::<MessagesState>::new();

    // Add nodes
    graph.add_node("process_input", process_input)?;
    graph.add_node("decide_action", decide_action)?;
    graph.add_node("provide_help", provide_help)?;
    graph.add_node("generate_response", generate_response)?;

    // Add edges
    graph.add_edge(START, "process_input");
    graph.add_edge("process_input", "decide_action");

    // Conditional edge from decide_action
    let mut mapping = HashMap::new();
    mapping.insert("help".to_string(), "provide_help".to_string());
    mapping.insert("respond".to_string(), "generate_response".to_string());

    graph.add_conditional_edges(
        "decide_action",
        |state: &MessagesState| {
            let action = serde_json::to_value(state)
                .ok()
                .and_then(|j| j.get("action").and_then(|v| v.as_str()).map(String::from))
                .unwrap_or_else(|| "respond".to_string());
            async move { Ok(action) }
        },
        mapping,
    );

    graph.add_edge("provide_help", END);
    graph.add_edge("generate_response", END);

    // Compile the graph
    let compiled = graph.compile()?;

    // Example 1: Regular invocation
    println!("=== Example 1: Regular invocation ===");
    let initial_state =
        MessagesState::with_messages(vec![Message::new_human_message("Hello, how are you?")]);
    let final_state = compiled.invoke(initial_state).await?;

    println!("Final messages:");
    for message in &final_state.messages {
        println!(
            "  {}: {}",
            message.message_type.to_string(),
            message.content
        );
    }

    // Example 2: Help request
    println!("\n=== Example 2: Help request ===");
    let initial_state =
        MessagesState::with_messages(vec![Message::new_human_message("I need help")]);
    let final_state = compiled.invoke(initial_state).await?;

    println!("Final messages:");
    for message in &final_state.messages {
        println!(
            "  {}: {}",
            message.message_type.to_string(),
            message.content
        );
    }

    // Example 3: Streaming execution
    println!("\n=== Example 3: Streaming execution ===");
    use futures::StreamExt;
    let initial_state =
        MessagesState::with_messages(vec![Message::new_human_message("Test streaming")]);
    let mut stream = compiled.stream(initial_state);

    while let Some(event) = stream.next().await {
        match event {
            oris::langgraph::StreamEvent::NodeStart { node, .. } => {
                println!("  → Starting node: {}", node);
            }
            oris::langgraph::StreamEvent::NodeEnd { node, .. } => {
                println!("  ← Completed node: {}", node);
            }
            oris::langgraph::StreamEvent::GraphEnd { .. } => {
                println!("  ✓ Graph completed");
            }
            oris::langgraph::StreamEvent::Error { error } => {
                eprintln!("  ✗ Error: {:?}", error);
            }
            oris::langgraph::StreamEvent::MessageChunk { .. }
            | oris::langgraph::StreamEvent::CustomData { .. } => {}
        }
    }

    Ok(())
}
