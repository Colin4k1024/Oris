use oris::langgraph::{function_node, MessagesState, StateGraph, END, START};
use oris::schemas::messages::Message;
use std::collections::HashMap;

/// Subgraph example with shared state
///
/// This example demonstrates using a subgraph as a node in a parent graph,
/// where both graphs share the same state type (MessagesState).
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a subgraph
    let mut subgraph = StateGraph::<MessagesState>::new();

    let sub_node1 = function_node("sub_node1", |_state: &MessagesState| async move {
        let mut update = HashMap::new();
        update.insert(
            "messages".to_string(),
            serde_json::to_value(vec![Message::new_ai_message("Subgraph node 1")])?,
        );
        Ok(update)
    });

    let sub_node2 = function_node("sub_node2", |_state: &MessagesState| async move {
        let mut update = HashMap::new();
        update.insert(
            "messages".to_string(),
            serde_json::to_value(vec![Message::new_ai_message("Subgraph node 2")])?,
        );
        Ok(update)
    });

    subgraph.add_node("sub_node1", sub_node1)?;
    subgraph.add_node("sub_node2", sub_node2)?;
    subgraph.add_edge(START, "sub_node1");
    subgraph.add_edge("sub_node1", "sub_node2");
    subgraph.add_edge("sub_node2", END);

    let compiled_subgraph = subgraph.compile()?;

    // Create parent graph
    let mut parent_graph = StateGraph::<MessagesState>::new();

    let parent_node = function_node("parent_node", |_state: &MessagesState| async move {
        let mut update = HashMap::new();
        update.insert(
            "messages".to_string(),
            serde_json::to_value(vec![Message::new_ai_message("Parent node")])?,
        );
        Ok(update)
    });

    parent_graph.add_node("parent_node", parent_node)?;
    parent_graph.add_subgraph("subgraph_node", compiled_subgraph)?;

    parent_graph.add_edge(START, "parent_node");
    parent_graph.add_edge("parent_node", "subgraph_node");
    parent_graph.add_edge("subgraph_node", END);

    let compiled = parent_graph.compile()?;

    // Execute the graph
    let initial_state = MessagesState::new();
    let result = compiled.invoke(initial_state).await?;

    println!("Final messages count: {}", result.messages.len());
    for (i, msg) in result.messages.iter().enumerate() {
        println!("Message {}: {}", i, msg.content);
    }

    Ok(())
}
