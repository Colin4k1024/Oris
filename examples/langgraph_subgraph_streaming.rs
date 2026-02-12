use futures::StreamExt;
use oris::langgraph::{
    function_node, MessagesState, StateGraph, StreamMode, StreamOptions, END, START,
};
use oris::schemas::messages::Message;
use std::collections::HashMap;

/// Subgraph streaming example
///
/// This example demonstrates streaming from a graph that contains subgraphs,
/// with subgraphs option enabled to see subgraph events.
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a subgraph
    let mut subgraph = StateGraph::<MessagesState>::new();

    let sub_node = function_node("sub_node", |_state: &MessagesState| async move {
        let mut update = HashMap::new();
        update.insert(
            "messages".to_string(),
            serde_json::to_value(vec![Message::new_ai_message("From subgraph")])?,
        );
        Ok(update)
    });

    subgraph.add_node("sub_node", sub_node)?;
    subgraph.add_edge(START, "sub_node");
    subgraph.add_edge("sub_node", END);

    let compiled_subgraph = subgraph.compile()?;

    // Create parent graph
    let mut parent_graph = StateGraph::<MessagesState>::new();

    let parent_node = function_node("parent_node", |_state: &MessagesState| async move {
        let mut update = HashMap::new();
        update.insert(
            "messages".to_string(),
            serde_json::to_value(vec![Message::new_ai_message("From parent")])?,
        );
        Ok(update)
    });

    parent_graph.add_node("parent_node", parent_node)?;
    parent_graph.add_subgraph("subgraph_node", compiled_subgraph)?;

    parent_graph.add_edge(START, "parent_node");
    parent_graph.add_edge("parent_node", "subgraph_node");
    parent_graph.add_edge("subgraph_node", END);

    let compiled = parent_graph.compile()?;

    // Stream with subgraphs enabled
    let initial_state = MessagesState::new();
    let options = StreamOptions::new()
        .with_modes(vec![StreamMode::Updates])
        .with_subgraphs(true);

    let mut stream = compiled.stream_with_options(initial_state, options);

    println!("Streaming events (with subgraphs):");
    while let Some(event) = stream.next().await {
        match event {
            oris::langgraph::StreamEvent::NodeStart { node, path, .. } => {
                if path.is_empty() {
                    println!("  NodeStart: {}", node);
                } else {
                    println!("  NodeStart: {} (path: {:?})", node, path);
                }
            }
            oris::langgraph::StreamEvent::NodeEnd { node, path, .. } => {
                if path.is_empty() {
                    println!("  NodeEnd: {}", node);
                } else {
                    println!("  NodeEnd: {} (path: {:?})", node, path);
                }
            }
            oris::langgraph::StreamEvent::GraphEnd { .. } => {
                println!("  GraphEnd");
            }
            oris::langgraph::StreamEvent::Error { error } => {
                eprintln!("  Error: {}", error);
            }
            _ => {}
        }
    }

    Ok(())
}
