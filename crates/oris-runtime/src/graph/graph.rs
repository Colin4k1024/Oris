use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::{
    compiled::CompiledGraph,
    edge::{Edge, EdgeType, END, START},
    error::GraphError,
    node::{Node, SubgraphNode, SubgraphNodeWithTransform},
    persistence::{checkpointer::CheckpointerBox, store::StoreBox},
    plugin::NodePluginRegistry,
    state::{State, StateUpdate},
};

/// StateGraph - a builder for creating stateful graphs
///
/// This is the main entry point for creating LangGraph workflows.
/// Similar to Python's StateGraph, it allows you to add nodes and edges
/// to build a graph, then compile it for execution.
///
/// # Example
///
/// ```rust,no_run
/// use oris_runtime::graph::{StateGraph, MessagesState, function_node, END, START};
///
/// let mut graph = StateGraph::<MessagesState>::new();
/// graph.add_node("node1", function_node("node1", |_state| async move {
///     Ok(std::collections::HashMap::new())
/// })).unwrap();
/// graph.add_edge(START, "node1");
/// graph.add_edge("node1", END);
/// let compiled = graph.compile().unwrap();
/// ```
pub struct StateGraph<S: State> {
    nodes: HashMap<String, Arc<dyn Node<S>>>,
    edges: Vec<Edge<S>>,
}

impl<S: State + 'static> StateGraph<S> {
    /// Create a new empty StateGraph
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            edges: Vec::new(),
        }
    }

    /// Add a node to the graph
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the node (must be unique)
    /// * `node` - The node implementation
    ///
    /// # Errors
    ///
    /// Returns an error if a node with the same name already exists
    pub fn add_node<N: Node<S> + 'static>(
        &mut self,
        name: impl Into<String>,
        node: N,
    ) -> Result<&mut Self, GraphError> {
        self.add_shared_node(name, Arc::new(node))
    }

    /// Add a pre-built shared node instance to the graph.
    ///
    /// This is mainly used by runtime plugin registries that construct nodes
    /// dynamically and return trait objects.
    pub fn add_shared_node(
        &mut self,
        name: impl Into<String>,
        node: Arc<dyn Node<S>>,
    ) -> Result<&mut Self, GraphError> {
        let name = name.into();
        self.validate_new_node_name(&name)?;
        self.nodes.insert(name, node);
        Ok(self)
    }

    /// Add a node by resolving a registered runtime plugin and config payload.
    ///
    /// The plugin is responsible for validating the payload and constructing a
    /// concrete node implementation.
    pub fn add_plugin_node(
        &mut self,
        name: impl Into<String>,
        plugin_type: &str,
        config: impl Into<serde_json::Value>,
        registry: &NodePluginRegistry<S>,
    ) -> Result<&mut Self, GraphError> {
        let name = name.into();
        let config = config.into();
        let node = registry.create_node(&name, plugin_type, &config)?;
        self.add_shared_node(name, node)
    }

    /// Add a subgraph as a node (shared state type)
    ///
    /// This allows a compiled graph to be used as a node in this graph.
    /// The subgraph and parent graph must share the same state type.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the node (must be unique)
    /// * `subgraph` - The compiled subgraph to use as a node
    ///
    /// # Errors
    ///
    /// Returns an error if a node with the same name already exists
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use oris_runtime::graph::{StateGraph, MessagesState};
    ///
    /// // Create a subgraph
    /// let mut subgraph = StateGraph::<MessagesState>::new();
    /// let compiled_subgraph = subgraph.compile().unwrap();
    ///
    /// // Add to parent graph
    /// let mut parent = StateGraph::<MessagesState>::new();
    /// parent.add_subgraph("subgraph_node", compiled_subgraph).unwrap();
    /// ```
    pub fn add_subgraph(
        &mut self,
        name: impl Into<String>,
        subgraph: CompiledGraph<S>,
    ) -> Result<&mut Self, GraphError> {
        let node = SubgraphNode::new(name, subgraph);
        self.add_node(node.name().to_string(), node)
    }

    /// Add a subgraph as a node with state transformation
    ///
    /// This allows a compiled graph with a different state type to be used
    /// as a node in this graph. State transformation functions are provided
    /// to convert between parent and subgraph state types.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the node (must be unique)
    /// * `subgraph` - The compiled subgraph (with different state type)
    /// * `transform_in` - Function to convert parent state to subgraph state
    /// * `transform_out` - Function to convert subgraph state to parent state update
    ///
    /// # Errors
    ///
    /// Returns an error if a node with the same name already exists
    ///
    /// # Example
    ///
    /// See [SubgraphNodeWithTransform] for a runnable example with [MessagesState].
    pub fn add_subgraph_with_transform<SubState: State + 'static>(
        &mut self,
        name: impl Into<String>,
        subgraph: CompiledGraph<SubState>,
        transform_in: impl Fn(&S) -> Result<SubState, GraphError> + Send + Sync + 'static,
        transform_out: impl Fn(&SubState) -> Result<StateUpdate, GraphError> + Send + Sync + 'static,
    ) -> Result<&mut Self, GraphError> {
        let node = SubgraphNodeWithTransform::new(name, subgraph, transform_in, transform_out);
        self.add_node(node.name().to_string(), node)
    }

    /// Add a regular edge between two nodes
    ///
    /// # Arguments
    ///
    /// * `from` - The source node name (can be START)
    /// * `to` - The target node name (can be END)
    pub fn add_edge(&mut self, from: impl Into<String>, to: impl Into<String>) -> &mut Self {
        let edge = Edge::new(from, to);
        self.edges.push(edge);
        self
    }

    /// Add a conditional edge from a node
    ///
    /// # Arguments
    ///
    /// * `from` - The source node name
    /// * `condition` - A function that takes state and returns a condition result
    /// * `mapping` - A map from condition results to target node names
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use std::collections::HashMap;
    /// use oris_runtime::graph::{StateGraph, MessagesState};
    ///
    /// let mut graph = StateGraph::<MessagesState>::new();
    /// let mut mapping = HashMap::new();
    /// mapping.insert("yes".to_string(), "node_yes".to_string());
    /// mapping.insert("no".to_string(), "node_no".to_string());
    ///
    /// graph.add_conditional_edges("node1", |state| async move {
    ///     Ok("yes".to_string())
    /// }, mapping);
    /// ```
    pub fn add_conditional_edges<F, Fut>(
        &mut self,
        from: impl Into<String>,
        condition: F,
        mapping: HashMap<String, String>,
    ) -> &mut Self
    where
        F: Fn(&S) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<String, GraphError>> + Send + 'static,
    {
        let edge = Edge::conditional(from, condition, mapping);
        self.edges.push(edge);
        self
    }

    /// Compile the graph into an executable CompiledGraph
    ///
    /// This validates the graph structure and creates an optimized
    /// representation for execution.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - No path exists from START to END
    /// - Nodes are referenced in edges but not defined
    /// - Other graph validation errors
    pub fn compile(self) -> Result<CompiledGraph<S>, GraphError> {
        self.compile_with_persistence(None, None)
    }

    /// Compile the graph with checkpointer and store
    ///
    /// This allows the graph to persist state and support features like
    /// replay, time travel, and cross-thread storage.
    ///
    /// Subgraphs will automatically inherit the parent's checkpointer and store
    /// if they don't have their own.
    ///
    /// # Arguments
    ///
    /// * `checkpointer` - Optional checkpointer for saving state snapshots
    /// * `store` - Optional store for cross-thread storage
    pub fn compile_with_persistence(
        self,
        checkpointer: Option<CheckpointerBox<S>>,
        store: Option<StoreBox>,
    ) -> Result<CompiledGraph<S>, GraphError> {
        // Validate graph structure
        self.validate()?;

        // Build adjacency list for efficient traversal
        let adjacency = self.build_adjacency()?;

        // Take ownership of nodes so we don't borrow self while moving
        let nodes = self.nodes;
        let nodes =
            Self::propagate_persistence_to_subgraphs(nodes, checkpointer.as_ref(), store.as_ref())?;

        CompiledGraph::with_persistence(nodes, adjacency, checkpointer, store)
    }

    /// Propagate checkpointer and store to subgraphs
    ///
    /// This ensures that subgraphs inherit the parent's checkpointer and store
    /// if they don't have their own, as per Python LangGraph behavior.
    ///
    /// Note: Currently, persistence is handled at execution time via config.
    /// Subgraphs will automatically use the parent's checkpointer when invoked
    /// with config that includes checkpointer information.
    fn propagate_persistence_to_subgraphs(
        nodes: HashMap<String, Arc<dyn Node<S>>>,
        _checkpointer: Option<&CheckpointerBox<S>>,
        _store: Option<&StoreBox>,
    ) -> Result<HashMap<String, Arc<dyn Node<S>>>, GraphError> {
        // Note: Persistence propagation happens at execution time.
        // When a subgraph is invoked with config, it will use the parent's
        // checkpointer if available. This matches Python LangGraph behavior.
        Ok(nodes)
    }

    fn validate_new_node_name(&self, name: &str) -> Result<(), GraphError> {
        if self.nodes.contains_key(name) {
            return Err(GraphError::CompilationError(format!(
                "Node '{}' already exists",
                name
            )));
        }

        if name == START || name == END {
            return Err(GraphError::CompilationError(format!(
                "Cannot add node with reserved name '{}'",
                name
            )));
        }

        Ok(())
    }

    /// Validate the graph structure
    fn validate(&self) -> Result<(), GraphError> {
        // Check that all edges reference valid nodes
        for edge in &self.edges {
            // Check source node (unless it's START)
            if edge.from != START && !self.nodes.contains_key(&edge.from) {
                return Err(GraphError::InvalidEdge(
                    edge.from.clone(),
                    "source node not found".to_string(),
                ));
            }

            // Check target nodes
            match &edge.edge_type {
                EdgeType::Regular { to } => {
                    if *to != END && !self.nodes.contains_key(to) {
                        return Err(GraphError::InvalidEdge(
                            edge.from.clone(),
                            format!("target node '{}' not found", to),
                        ));
                    }
                }
                EdgeType::Conditional { mapping, .. } => {
                    for target in mapping.values() {
                        if *target != END && !self.nodes.contains_key(target) {
                            return Err(GraphError::InvalidEdge(
                                edge.from.clone(),
                                format!("conditional target node '{}' not found", target),
                            ));
                        }
                    }
                }
            }
        }

        // Check that there's a path from START to END
        if !self.has_path_to_end() {
            return Err(GraphError::NoPathToEnd);
        }

        Ok(())
    }

    /// Build adjacency list for graph traversal
    fn build_adjacency(&self) -> Result<HashMap<String, Vec<Edge<S>>>, GraphError> {
        let mut adjacency: HashMap<String, Vec<Edge<S>>> = HashMap::new();

        for edge in &self.edges {
            adjacency
                .entry(edge.from.clone())
                .or_insert_with(Vec::new)
                .push(edge.clone());
        }

        Ok(adjacency)
    }

    /// Check if there's a path from START to END using DFS
    fn has_path_to_end(&self) -> bool {
        let adjacency = match self.build_adjacency() {
            Ok(adj) => adj,
            Err(_) => return false,
        };

        let mut visited = HashSet::new();
        self.dfs(START, &adjacency, &mut visited)
    }

    /// Depth-first search to find path to END
    fn dfs(
        &self,
        node: &str,
        adjacency: &HashMap<String, Vec<Edge<S>>>,
        visited: &mut HashSet<String>,
    ) -> bool {
        if node == END {
            return true;
        }

        if visited.contains(node) {
            return false;
        }

        visited.insert(node.to_string());

        if let Some(edges) = adjacency.get(node) {
            for edge in edges {
                match &edge.edge_type {
                    EdgeType::Regular { to } => {
                        if self.dfs(to, adjacency, visited) {
                            return true;
                        }
                    }
                    EdgeType::Conditional { mapping, .. } => {
                        // For conditional edges, check all possible targets
                        for target in mapping.values() {
                            if self.dfs(target, adjacency, visited) {
                                return true;
                            }
                        }
                    }
                }
            }
        }

        false
    }
}

impl<S: State + 'static> Default for StateGraph<S> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{function_node, state::MessagesState};

    #[test]
    fn test_add_node() {
        let mut graph = StateGraph::<MessagesState>::new();
        let node = function_node("test", |_state| async move {
            Ok(std::collections::HashMap::new())
        });

        assert!(graph.add_node("test", node).is_ok());
        assert!(graph
            .add_node(
                "test",
                function_node("test2", |_state| async move {
                    Ok(std::collections::HashMap::new())
                })
            )
            .is_err()); // Duplicate node
    }

    #[test]
    fn test_add_edge() {
        let mut graph = StateGraph::<MessagesState>::new();
        graph
            .add_node(
                "node1",
                function_node("node1", |_state| async move {
                    Ok(std::collections::HashMap::new())
                }),
            )
            .unwrap();

        graph.add_edge(START, "node1");
        graph.add_edge("node1", END);

        assert!(graph.compile().is_ok());
    }

    #[test]
    fn test_validate() {
        let mut graph = StateGraph::<MessagesState>::new();
        graph.add_edge("nonexistent", "node1");

        assert!(graph.compile().is_err());
    }
}
