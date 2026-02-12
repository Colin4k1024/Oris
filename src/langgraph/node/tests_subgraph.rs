#[cfg(test)]
mod subgraph_tests {
    use crate::langgraph::{
        function_node, state::MessagesState, StateGraph, SubgraphNode, END, START,
    };
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_subgraph_node_invoke() {
        // Create a simple subgraph
        let mut subgraph = StateGraph::<MessagesState>::new();
        let node = function_node("test_node", |_state: &MessagesState| async move {
            use crate::langgraph::error::LangGraphError;
            let mut update = HashMap::new();
            update.insert(
                "messages".to_string(),
                serde_json::to_value(vec![crate::schemas::messages::Message::new_ai_message(
                    "Test",
                )])
                .map_err(|e| LangGraphError::SerializationError(e))?,
            );
            Ok(update)
        });
        subgraph.add_node("test_node", node).unwrap();
        subgraph.add_edge(START, "test_node");
        subgraph.add_edge("test_node", END);

        let compiled_subgraph = subgraph.compile().unwrap();
        let subgraph_node = SubgraphNode::new("subgraph", compiled_subgraph);

        let state = MessagesState::new();
        let result = subgraph_node.invoke(&state).await.unwrap();
        assert!(result.contains_key("messages"));
    }

    #[tokio::test]
    async fn test_add_subgraph() {
        // Create subgraph
        let mut subgraph = StateGraph::<MessagesState>::new();
        let node = function_node("sub_node", |_state: &MessagesState| async move {
            Ok(HashMap::new())
        });
        subgraph.add_node("sub_node", node).unwrap();
        subgraph.add_edge(START, "sub_node");
        subgraph.add_edge("sub_node", END);
        let compiled_subgraph = subgraph.compile().unwrap();

        // Add to parent
        let mut parent = StateGraph::<MessagesState>::new();
        parent
            .add_subgraph("subgraph_node", compiled_subgraph)
            .unwrap();
        parent.add_edge(START, "subgraph_node");
        parent.add_edge("subgraph_node", END);

        let compiled = parent.compile().unwrap();
        let state = MessagesState::new();
        let result = compiled.invoke(state).await;
        assert!(result.is_ok());
    }
}
