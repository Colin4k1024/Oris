use std::collections::HashMap;

use crate::langgraph::{
    error::LangGraphError,
    node::Node,
    persistence::{config::RunnableConfig, store::StoreBox},
    state::{State, StateUpdate},
};

/// Execute multiple nodes in parallel
///
/// Takes a list of node names and executes them concurrently,
/// returning their state updates.
///
/// Note: If an interrupt occurs in any node, the error is propagated
/// and execution stops. Interrupts in parallel nodes are not currently
/// supported (only the first interrupt will be caught).
pub async fn execute_nodes_parallel<S: State>(
    nodes: &HashMap<String, std::sync::Arc<dyn Node<S>>>,
    node_names: &[String],
    state: &S,
    config: Option<&RunnableConfig>,
    store: Option<StoreBox>,
) -> Result<Vec<(String, StateUpdate)>, LangGraphError> {
    // Create futures for all nodes
    let futures: Vec<_> = node_names
        .iter()
        .map(|node_name| {
            let node_opt = nodes.get(node_name).cloned();
            let node_name = node_name.clone();
            let state = state.clone();
            let store = store.clone();

            async move {
                let node =
                    node_opt.ok_or_else(|| LangGraphError::NodeNotFound(node_name.clone()))?;
                let update = node.invoke_with_context(&state, config, store).await?;
                Ok::<(String, StateUpdate), LangGraphError>((node_name, update))
            }
        })
        .collect();

    // Execute all nodes in parallel
    let results = futures::future::join_all(futures).await;

    // Collect results and handle errors
    // If an interrupt occurs, propagate it immediately
    let mut updates = Vec::new();
    for result in results {
        match result {
            Ok(update) => updates.push(update),
            Err(e) => {
                // Interrupt or other error - return immediately
                return Err(e);
            }
        }
    }

    Ok(updates)
}

/// Merge multiple state updates into a single state
///
/// When multiple nodes execute in parallel, their updates need to be merged.
pub fn merge_state_updates<S: State>(
    state: &S,
    updates: &[(String, StateUpdate)],
) -> Result<S, LangGraphError> {
    let mut current_state = state.clone();

    // Merge all updates sequentially
    // Note: The order of merging may matter for some state types
    for (node_name, update) in updates {
        log::debug!("Merging update from node: {}", node_name);
        current_state = merge_single_update(&current_state, update)?;
    }

    Ok(current_state)
}

/// Merge a single state update
fn merge_single_update<S: State>(state: &S, update: &StateUpdate) -> Result<S, LangGraphError> {
    // Try to handle MessagesState specially
    let state_json = serde_json::to_value(state).map_err(LangGraphError::SerializationError)?;

    if state_json.get("messages").is_some() {
        return merge_messages_state_update(state, update);
    }

    // For other state types, create a new state from the update and merge
    let update_json = serde_json::to_value(update).map_err(LangGraphError::SerializationError)?;

    let update_state: S = serde_json::from_value(update_json.clone()).map_err(|_| {
        LangGraphError::ExecutionError("Cannot deserialize update as state".to_string())
    })?;

    Ok(state.merge(&update_state))
}

/// Merge update for MessagesState (specialized handling)
fn merge_messages_state_update<S: State>(
    state: &S,
    update: &StateUpdate,
) -> Result<S, LangGraphError> {
    use crate::langgraph::state::{apply_update_to_messages_state, MessagesState};

    let state_json = serde_json::to_value(state).map_err(LangGraphError::SerializationError)?;

    let messages_state: MessagesState = if let Some(messages_value) = state_json.get("messages") {
        if let Ok(messages) =
            serde_json::from_value::<Vec<crate::schemas::messages::Message>>(messages_value.clone())
        {
            MessagesState::with_messages(messages)
        } else {
            MessagesState::new()
        }
    } else {
        MessagesState::new()
    };

    let updated_state = apply_update_to_messages_state(&messages_state, update);

    let updated_json =
        serde_json::to_value(&updated_state).map_err(LangGraphError::SerializationError)?;
    serde_json::from_value(updated_json).map_err(LangGraphError::SerializationError)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::langgraph::{function_node, state::MessagesState};

    #[tokio::test]
    async fn test_execute_nodes_parallel() {
        let mut nodes = HashMap::new();
        nodes.insert(
            "node1".to_string(),
            std::sync::Arc::new(function_node("node1", |_state| async move {
                let mut update = HashMap::new();
                update.insert(
                    "messages".to_string(),
                    serde_json::to_value(vec![crate::schemas::messages::Message::new_ai_message(
                        "Node1",
                    )])?,
                );
                Ok(update)
            })),
        );
        nodes.insert(
            "node2".to_string(),
            std::sync::Arc::new(function_node("node2", |_state| async move {
                let mut update = HashMap::new();
                update.insert(
                    "messages".to_string(),
                    serde_json::to_value(vec![crate::schemas::messages::Message::new_ai_message(
                        "Node2",
                    )])?,
                );
                Ok(update)
            })),
        );

        let state = MessagesState::new();
        let results =
            execute_nodes_parallel(&nodes, &["node1".to_string(), "node2".to_string()], &state)
                .await
                .unwrap();

        assert_eq!(results.len(), 2);
    }
}
