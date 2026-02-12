use std::collections::{HashMap, HashSet};

use crate::langgraph::{
    edge::{Edge, EdgeType, END, START},
    error::LangGraphError,
    state::State,
};

/// Node scheduler for identifying nodes that can be executed in parallel
///
/// Analyzes the graph structure to determine which nodes are ready to execute
/// in the current super-step.
pub struct NodeScheduler<S: State> {
    pub(crate) adjacency: HashMap<String, Vec<Edge<S>>>,
    reverse_adjacency: HashMap<String, Vec<String>>,
}

impl<S: State> NodeScheduler<S> {
    /// Create a new node scheduler
    pub fn new(adjacency: HashMap<String, Vec<Edge<S>>>) -> Self {
        // Build reverse adjacency for dependency analysis
        let mut reverse_adjacency: HashMap<String, Vec<String>> = HashMap::new();

        for (from, edges) in &adjacency {
            for edge in edges {
                match &edge.edge_type {
                    EdgeType::Regular { to } => {
                        reverse_adjacency
                            .entry(to.clone())
                            .or_insert_with(Vec::new)
                            .push(from.clone());
                    }
                    EdgeType::Conditional { mapping, .. } => {
                        for target in mapping.values() {
                            reverse_adjacency
                                .entry(target.clone())
                                .or_insert_with(Vec::new)
                                .push(from.clone());
                        }
                    }
                }
            }
        }

        Self {
            adjacency,
            reverse_adjacency,
        }
    }

    /// Get nodes that are ready to execute in the current super-step
    ///
    /// A node is ready if:
    /// - It's the START node (first super-step)
    /// - All its predecessor nodes have been executed
    /// - It's not the END node
    pub async fn get_ready_nodes(
        &self,
        executed_nodes: &HashSet<String>,
        current_state: &S,
    ) -> Result<Vec<String>, LangGraphError> {
        let mut ready_nodes = Vec::new();

        // If no nodes have been executed, start with nodes from START
        if executed_nodes.is_empty() {
            if let Some(start_edges) = self.adjacency.get(START) {
                for edge in start_edges {
                    let target = edge.get_target(current_state).await?;
                    if target != END && !ready_nodes.contains(&target) {
                        ready_nodes.push(target);
                    }
                }
            }
            return Ok(ready_nodes);
        }

        // Find nodes whose all predecessors have been executed
        for (node, predecessors) in &self.reverse_adjacency {
            if node == START || node == END {
                continue;
            }

            // Check if all predecessors have been executed
            let all_predecessors_executed = predecessors
                .iter()
                .all(|pred| pred == START || executed_nodes.contains(pred));

            if all_predecessors_executed && !executed_nodes.contains(node) {
                ready_nodes.push(node.clone());
            }
        }

        Ok(ready_nodes)
    }

    /// Get the next nodes to execute after a super-step
    ///
    /// This determines which nodes should be executed in the next super-step
    /// based on the edges from the currently executing nodes.
    pub async fn get_next_nodes(
        &self,
        current_nodes: &[String],
        state: &S,
    ) -> Result<Vec<String>, LangGraphError> {
        let mut next_nodes = HashSet::new();

        for node in current_nodes {
            if let Some(edges) = self.adjacency.get(node) {
                for edge in edges {
                    let target = edge.get_target(state).await?;
                    if target != END {
                        next_nodes.insert(target);
                    }
                }
            }
        }

        Ok(next_nodes.into_iter().collect())
    }

    /// Check if execution is complete (reached END)
    pub async fn is_complete(
        &self,
        current_nodes: &[String],
        state: &S,
    ) -> Result<bool, LangGraphError> {
        for node in current_nodes {
            if let Some(edges) = self.adjacency.get(node) {
                for edge in edges {
                    let target = edge.get_target(state).await?;
                    if target == END {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::langgraph::state::MessagesState;

    #[tokio::test]
    async fn test_get_ready_nodes() {
        let mut adjacency = HashMap::new();
        adjacency.insert(START.to_string(), vec![Edge::new(START, "node1")]);
        adjacency.insert("node1".to_string(), vec![Edge::new("node1", "node2")]);

        let scheduler = NodeScheduler::<MessagesState>::new(adjacency);
        let executed = HashSet::new();
        let state = MessagesState::new();

        let ready = scheduler.get_ready_nodes(&executed, &state).await.unwrap();
        assert_eq!(ready, vec!["node1"]);
    }
}
