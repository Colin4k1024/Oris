use oris::langgraph::{function_node, MessagesState, StateGraph, END, START};
use oris::schemas::messages::Message;

/// Simple hello world example for LangGraph
///
/// This example demonstrates the basic usage of LangGraph:
/// 1. Create a StateGraph
/// 2. Add a node
/// 3. Add edges from START to node and from node to END
/// 4. Compile the graph
/// 5. Invoke with initial state
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a simple node that returns a hello world message
    let mock_llm = function_node("mock_llm", |_state: &MessagesState| async move {
        use std::collections::HashMap;
        let mut update = HashMap::new();
        update.insert(
            "messages".to_string(),
            serde_json::to_value(vec![Message::new_ai_message("hello world")])?,
        );
        Ok(update)
    });

    // Build the graph
    let mut graph = StateGraph::<MessagesState>::new();
    graph.add_node("mock_llm", mock_llm)?;
    graph.add_edge(START, "mock_llm");
    graph.add_edge("mock_llm", END);

    // Compile the graph
    let compiled = graph.compile()?;

    // Invoke with initial state
    let initial_state = MessagesState::with_messages(vec![Message::new_human_message("hi!")]);
    let final_state = compiled.invoke(initial_state).await?;

    // Print the result
    println!("Final messages:");
    for message in &final_state.messages {
        println!(
            "  {}: {}",
            message.message_type.to_string(),
            message.content
        );
    }

    Ok(())
}
