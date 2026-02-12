use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::langgraph::{
    edge::START,
    error::LangGraphError,
    node::Node,
    persistence::{
        checkpointer::CheckpointerBox,
        config::{CheckpointConfig, RunnableConfig},
        snapshot::StateSnapshot,
        store::StoreBox,
    },
    state::State,
};

use super::{
    durability::{save_checkpoint, DurabilityMode},
    parallel::{execute_nodes_parallel, merge_state_updates},
    scheduler::NodeScheduler,
};

/// Super-step executor
///
/// Executes the graph in super-steps, where each super-step can execute
/// multiple nodes in parallel.
pub struct SuperStepExecutor<S: State + 'static> {
    nodes: HashMap<String, Arc<dyn Node<S>>>,
    scheduler: NodeScheduler<S>,
    checkpointer: Option<CheckpointerBox<S>>,
    durability_mode: DurabilityMode,
}

impl<S: State + 'static> SuperStepExecutor<S> {
    /// Create a new super-step executor
    pub fn new(
        nodes: HashMap<String, Arc<dyn Node<S>>>,
        scheduler: NodeScheduler<S>,
        checkpointer: Option<CheckpointerBox<S>>,
        durability_mode: DurabilityMode,
    ) -> Self {
        Self {
            nodes,
            scheduler,
            checkpointer,
            durability_mode,
        }
    }

    /// Execute the graph using super-step model
    ///
    /// Returns the final state after all super-steps complete.
    ///
    /// # Arguments
    ///
    /// * `initial_state` - The initial state to start execution with
    /// * `checkpoint_config` - The checkpoint configuration
    /// * `parent_config` - Optional parent checkpoint config for fork tracking
    /// * `config` - Optional RunnableConfig for nodes to access
    /// * `store` - Optional Store for nodes to access
    pub async fn execute(
        &self,
        initial_state: S,
        checkpoint_config: &CheckpointConfig,
        parent_config: Option<&CheckpointConfig>,
        config: Option<&RunnableConfig>,
        store: Option<StoreBox>,
    ) -> Result<S, LangGraphError> {
        let mut current_state = initial_state;
        let mut executed_nodes = HashSet::new();
        let mut step = 0;
        let max_steps = 1000; // Prevent infinite loops

        // Save initial checkpoint (only for Sync mode, others will be saved later)
        if self.durability_mode == DurabilityMode::Sync {
            if let Some(checkpointer) = &self.checkpointer {
                let initial_snapshot = if let Some(parent) = parent_config {
                    // Create snapshot with parent config for fork tracking
                    StateSnapshot::with_parent(
                        current_state.clone(),
                        vec![START.to_string()],
                        checkpoint_config.clone(),
                        parent.clone(),
                    )
                } else {
                    StateSnapshot::new(
                        current_state.clone(),
                        vec![START.to_string()],
                        checkpoint_config.clone(),
                    )
                };
                save_checkpoint(Some(&checkpointer), &initial_snapshot, self.durability_mode)
                    .await?;
            }
        }

        loop {
            if step >= max_steps {
                return Err(LangGraphError::ExecutionError(
                    "Maximum super-steps reached. Possible infinite loop.".to_string(),
                ));
            }
            step += 1;

            // Get ready nodes for this super-step
            let ready_nodes = self
                .scheduler
                .get_ready_nodes(&executed_nodes, &current_state)
                .await?;

            if ready_nodes.is_empty() {
                // Check if we've reached END
                if self
                    .scheduler
                    .is_complete(
                        &executed_nodes.iter().cloned().collect::<Vec<_>>(),
                        &current_state,
                    )
                    .await?
                {
                    break;
                }
                return Err(LangGraphError::ExecutionError(
                    "No ready nodes but execution not complete".to_string(),
                ));
            }

            log::debug!("Super-step {}: Executing nodes: {:?}", step, ready_nodes);

            // Execute all ready nodes in parallel
            let updates = execute_nodes_parallel(
                &self.nodes,
                &ready_nodes,
                &current_state,
                config,        // Pass config to nodes
                store.clone(), // Pass store to nodes (clone for each call)
            )
            .await?;

            // Mark nodes as executed
            for node_name in &ready_nodes {
                executed_nodes.insert(node_name.clone());
            }

            // Merge all state updates
            current_state = merge_state_updates(&current_state, &updates)?;

            // Save checkpoint after super-step
            if let Some(checkpointer) = &self.checkpointer {
                let next_nodes = self
                    .scheduler
                    .get_next_nodes(&ready_nodes, &current_state)
                    .await?;

                let mut metadata = HashMap::new();
                metadata.insert("step".to_string(), serde_json::json!(step));
                metadata.insert("executed_nodes".to_string(), serde_json::json!(ready_nodes));

                let snapshot = if let Some(parent) = parent_config {
                    // Create snapshot with parent config for fork tracking
                    // Note: We need to preserve metadata, so we'll add it after creation
                    let mut snapshot = StateSnapshot::with_parent(
                        current_state.clone(),
                        next_nodes,
                        checkpoint_config.clone(),
                        parent.clone(),
                    );
                    snapshot.metadata.extend(metadata);
                    snapshot
                } else {
                    StateSnapshot::with_metadata(
                        current_state.clone(),
                        next_nodes,
                        checkpoint_config.clone(),
                        metadata,
                    )
                };

                save_checkpoint(Some(&checkpointer), &snapshot, self.durability_mode).await?;
            }

            // Check if we've reached END using scheduler's is_complete method
            if self
                .scheduler
                .is_complete(&ready_nodes, &current_state)
                .await?
            {
                break;
            }
        }

        // Save final checkpoint if using Exit mode
        if self.durability_mode == DurabilityMode::Exit {
            if let Some(checkpointer) = &self.checkpointer {
                let final_snapshot = if let Some(parent) = parent_config {
                    // Create snapshot with parent config for fork tracking
                    StateSnapshot::with_parent(
                        current_state.clone(),
                        vec![],
                        checkpoint_config.clone(),
                        parent.clone(),
                    )
                } else {
                    StateSnapshot::new(current_state.clone(), vec![], checkpoint_config.clone())
                };
                checkpointer
                    .put(checkpoint_config.thread_id.as_str(), &final_snapshot)
                    .await
                    .map_err(|e| {
                        LangGraphError::ExecutionError(format!(
                            "Failed to save final checkpoint: {}",
                            e
                        ))
                    })?;
            }
        }

        Ok(current_state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::langgraph::{function_node, state::MessagesState};

    #[tokio::test]
    async fn test_superstep_executor() {
        let mut nodes = HashMap::new();
        nodes.insert(
            "node1".to_string(),
            std::sync::Arc::new(function_node("node1", |_state| async move {
                let mut update = HashMap::new();
                update.insert(
                    "messages".to_string(),
                    serde_json::to_value(vec![crate::schemas::messages::Message::new_ai_message(
                        "Hello",
                    )])?,
                );
                Ok(update)
            })),
        );

        let mut adjacency = HashMap::new();
        adjacency.insert(
            crate::langgraph::edge::START.to_string(),
            vec![crate::langgraph::edge::Edge::new(
                crate::langgraph::edge::START,
                "node1",
            )],
        );
        adjacency.insert(
            "node1".to_string(),
            vec![crate::langgraph::edge::Edge::new(
                "node1",
                crate::langgraph::edge::END,
            )],
        );

        let scheduler = NodeScheduler::new(adjacency);
        let executor = SuperStepExecutor::new(nodes, scheduler, None, DurabilityMode::Exit);

        let config = CheckpointConfig::new("thread-1");
        let state = MessagesState::new();
        let result = executor.execute(state, &config).await;
        assert!(result.is_ok());
    }
}
