use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;

use super::Node;
use crate::langgraph::{
    compiled::CompiledGraph,
    error::LangGraphError,
    persistence::{config::RunnableConfig, store::StoreBox},
    state::State,
    StateUpdate,
};

/// Subgraph node - wraps a CompiledGraph as a node
///
/// This allows a graph to be used as a node in another graph.
/// The subgraph and parent graph share the same state type.
///
/// # Example
///
/// ```rust,no_run
/// use oris::langgraph::{StateGraph, MessagesState, CompiledGraph};
///
/// // Create a subgraph
/// let mut subgraph = StateGraph::<MessagesState>::new();
/// // ... add nodes to subgraph
/// let compiled_subgraph = subgraph.compile()?;
///
/// // Use as a node in parent graph
/// let mut parent = StateGraph::<MessagesState>::new();
/// parent.add_subgraph("subgraph_node", compiled_subgraph)?;
/// ```
pub struct SubgraphNode<S: State + 'static> {
    subgraph: Arc<CompiledGraph<S>>,
    name: String,
}

impl<S: State + 'static> SubgraphNode<S> {
    /// Create a new subgraph node
    pub fn new(name: impl Into<String>, subgraph: CompiledGraph<S>) -> Self {
        Self {
            subgraph: Arc::new(subgraph),
            name: name.into(),
        }
    }

    /// Get the name of the subgraph node
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get a reference to the subgraph
    pub fn subgraph(&self) -> &CompiledGraph<S> {
        &self.subgraph
    }
}

#[async_trait]
impl<S: State + 'static> Node<S> for SubgraphNode<S> {
    async fn invoke(&self, state: &S) -> Result<StateUpdate, LangGraphError> {
        // Execute the subgraph with the current state
        let final_state = self.subgraph.invoke(state.clone()).await?;

        // Convert final state to state update
        // For shared state, we merge the final state back
        let state_json =
            serde_json::to_value(final_state).map_err(LangGraphError::SerializationError)?;

        let mut update = HashMap::new();
        if let serde_json::Value::Object(map) = state_json {
            for (key, value) in map {
                update.insert(key, value);
            }
        }

        Ok(update)
    }

    async fn invoke_with_context(
        &self,
        state: &S,
        config: Option<&RunnableConfig>,
        _store: Option<StoreBox>,
    ) -> Result<StateUpdate, LangGraphError> {
        // Execute the subgraph with config and store
        // Note: subgraph will inherit checkpointer from parent if provided
        let final_state = if let Some(config) = config {
            self.subgraph
                .invoke_with_config(Some(state.clone()), config)
                .await?
        } else {
            self.subgraph.invoke(state.clone()).await?
        };

        // Convert final state to state update
        let state_json =
            serde_json::to_value(final_state).map_err(LangGraphError::SerializationError)?;

        let mut update = HashMap::new();
        if let serde_json::Value::Object(map) = state_json {
            for (key, value) in map {
                update.insert(key, value);
            }
        }

        Ok(update)
    }

    fn get_subgraph(&self) -> Option<Arc<CompiledGraph<S>>> {
        Some(self.subgraph.clone())
    }
}

/// Subgraph node with state transformation
///
/// This allows a subgraph with a different state type to be used
/// as a node in a parent graph. State transformation functions are
/// provided to convert between parent and subgraph state types.
///
/// # Example
///
/// ```rust,no_run
/// // Parent state
/// struct ParentState { foo: String }
///
/// // Subgraph state
/// struct SubState { bar: String }
///
/// // Create subgraph with SubState
/// let mut subgraph = StateGraph::<SubState>::new();
/// // ... add nodes
/// let compiled_subgraph = subgraph.compile()?;
///
/// // Add to parent with transformation
/// let mut parent = StateGraph::<ParentState>::new();
/// parent.add_subgraph_with_transform(
///     "subgraph_node",
///     compiled_subgraph,
///     |parent_state| -> Result<SubState, _> {
///         Ok(SubState { bar: parent_state.foo.clone() })
///     },
///     |sub_state| -> Result<StateUpdate, _> {
///         let mut update = HashMap::new();
///         update.insert("foo".to_string(), serde_json::to_value(sub_state.bar.clone())?);
///         Ok(update)
///     },
/// )?;
/// ```
pub struct SubgraphNodeWithTransform<ParentState: State + 'static, SubState: State + 'static> {
    subgraph: Arc<CompiledGraph<SubState>>,
    name: String,
    transform_in: Arc<dyn Fn(&ParentState) -> Result<SubState, LangGraphError> + Send + Sync>,
    transform_out: Arc<dyn Fn(&SubState) -> Result<StateUpdate, LangGraphError> + Send + Sync>,
}

impl<ParentState: State + 'static, SubState: State + 'static>
    SubgraphNodeWithTransform<ParentState, SubState>
{
    /// Create a new subgraph node with transformation
    pub fn new(
        name: impl Into<String>,
        subgraph: CompiledGraph<SubState>,
        transform_in: impl Fn(&ParentState) -> Result<SubState, LangGraphError> + Send + Sync + 'static,
        transform_out: impl Fn(&SubState) -> Result<StateUpdate, LangGraphError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            subgraph: Arc::new(subgraph),
            name: name.into(),
            transform_in: Arc::new(transform_in),
            transform_out: Arc::new(transform_out),
        }
    }

    /// Get the name of the subgraph node
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get a reference to the subgraph
    pub fn subgraph(&self) -> &CompiledGraph<SubState> {
        &self.subgraph
    }
}

#[async_trait]
impl<ParentState: State + 'static, SubState: State + 'static> Node<ParentState>
    for SubgraphNodeWithTransform<ParentState, SubState>
{
    async fn invoke(&self, state: &ParentState) -> Result<StateUpdate, LangGraphError> {
        // Transform parent state to subgraph state
        let sub_state = (self.transform_in)(state)?;

        // Execute the subgraph
        let final_sub_state = self.subgraph.invoke(sub_state).await?;

        // Transform subgraph state back to parent state update
        (self.transform_out)(&final_sub_state)
    }

    async fn invoke_with_context(
        &self,
        state: &ParentState,
        config: Option<&RunnableConfig>,
        _store: Option<StoreBox>,
    ) -> Result<StateUpdate, LangGraphError> {
        // Transform parent state to subgraph state
        let sub_state = (self.transform_in)(state)?;

        // Execute the subgraph with config
        let final_sub_state = if let Some(config) = config {
            self.subgraph
                .invoke_with_config(Some(sub_state), config)
                .await?
        } else {
            self.subgraph.invoke(sub_state).await?
        };

        // Transform subgraph state back to parent state update
        (self.transform_out)(&final_sub_state)
    }

    // Note: get_subgraph is not implemented for SubgraphNodeWithTransform
    // because it has a different state type. Streaming would need special handling.
}
