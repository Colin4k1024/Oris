use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    agent::Agent, chain::Chain, language_models::llm::LLM, prompt::PromptArgs,
    schemas::messages::Message,
};

use super::{
    compiled::CompiledGraph,
    error::LangGraphError,
    persistence::{config::RunnableConfig, store::StoreBox},
    state::State,
    StateUpdate,
};

mod subgraph;
pub use subgraph::{SubgraphNode, SubgraphNodeWithTransform};

/// Trait for nodes in a LangGraph
///
/// Nodes are the building blocks of a graph. They take state as input
/// and return state updates.
#[async_trait]
pub trait Node<S: State>: Send + Sync {
    /// Invoke the node with the current state
    ///
    /// Returns a state update that will be merged into the current state.
    async fn invoke(&self, state: &S) -> Result<StateUpdate, LangGraphError>;

    /// Invoke the node with state, config, and store
    ///
    /// This method allows nodes to access configuration and store for
    /// long-term memory and cross-thread data access.
    ///
    /// # Arguments
    ///
    /// * `state` - The current state
    /// * `config` - Optional configuration (thread_id, user_id, etc.)
    /// * `store` - Optional store for cross-thread storage
    ///
    /// # Default Implementation
    ///
    /// The default implementation calls `invoke(state)` to maintain
    /// backward compatibility. Nodes that need config or store should
    /// override this method.
    async fn invoke_with_context(
        &self,
        state: &S,
        _config: Option<&RunnableConfig>,
        _store: Option<StoreBox>,
    ) -> Result<StateUpdate, LangGraphError> {
        // Default implementation calls invoke for backward compatibility
        self.invoke(state).await
    }

    /// Get the LLM from this node if it contains one
    ///
    /// Returns None if this node is not an LLM node.
    /// This is used for streaming LLM tokens.
    fn get_llm(&self) -> Option<Arc<dyn LLM>> {
        None
    }

    /// Get the subgraph from this node if it contains one
    ///
    /// Returns None if this node is not a subgraph node.
    /// This is used for subgraph streaming.
    fn get_subgraph(&self) -> Option<Arc<CompiledGraph<S>>> {
        None
    }
}

/// Function node - wraps an async function
///
/// This is the simplest type of node, wrapping a function that takes
/// state and returns a state update.
///
/// Supports multiple function signatures:
/// - `Fn(&S) -> Fut` - state only
/// - `Fn(&S, &RunnableConfig) -> Fut` - state and config
/// - `Fn(&S, &RunnableConfig, &dyn Store) -> Fut` - state, config, and store
pub struct FunctionNode<S: State> {
    name: String,
    func_state_only: Option<
        Arc<
            dyn Fn(
                    &S,
                ) -> std::pin::Pin<
                    Box<
                        dyn std::future::Future<Output = Result<StateUpdate, LangGraphError>>
                            + Send,
                    >,
                > + Send
                + Sync,
        >,
    >,
    func_with_config: Option<
        Arc<
            dyn Fn(
                    &S,
                    &RunnableConfig,
                ) -> std::pin::Pin<
                    Box<
                        dyn std::future::Future<Output = Result<StateUpdate, LangGraphError>>
                            + Send,
                    >,
                > + Send
                + Sync,
        >,
    >,
    func_with_config_store: Option<
        Arc<
            dyn Fn(
                    &S,
                    &RunnableConfig,
                    StoreBox,
                ) -> std::pin::Pin<
                    Box<
                        dyn std::future::Future<Output = Result<StateUpdate, LangGraphError>>
                            + Send,
                    >,
                > + Send
                + Sync,
        >,
    >,
}

impl<S: State> FunctionNode<S> {
    /// Create a new function node with state only
    pub fn new<F, Fut>(name: String, func: F) -> Self
    where
        F: Fn(&S) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<StateUpdate, LangGraphError>> + Send + 'static,
    {
        Self {
            name,
            func_state_only: Some(Arc::new(move |state| Box::pin(func(state)))),
            func_with_config: None,
            func_with_config_store: None,
        }
    }

    /// Create a new function node with state and config
    pub fn with_config<F, Fut>(name: String, func: F) -> Self
    where
        F: Fn(&S, &RunnableConfig) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<StateUpdate, LangGraphError>> + Send + 'static,
    {
        Self {
            name,
            func_state_only: None,
            func_with_config: Some(Arc::new(move |state, config| Box::pin(func(state, config)))),
            func_with_config_store: None,
        }
    }

    /// Create a new function node with state, config, and store
    pub fn with_config_store<F, Fut>(name: String, func: F) -> Self
    where
        F: Fn(&S, &RunnableConfig, StoreBox) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<StateUpdate, LangGraphError>> + Send + 'static,
    {
        Self {
            name,
            func_state_only: None,
            func_with_config: None,
            func_with_config_store: Some(Arc::new(move |state, config, store| {
                Box::pin(func(state, config, store))
            })),
        }
    }

    /// Get the name of the node
    pub fn name(&self) -> &str {
        &self.name
    }
}

#[async_trait]
impl<S: State> Node<S> for FunctionNode<S> {
    async fn invoke(&self, state: &S) -> Result<StateUpdate, LangGraphError> {
        if let Some(ref func) = self.func_state_only {
            func(state).await
        } else {
            // If node requires config/store, invoke_with_context should be used
            Err(LangGraphError::ExecutionError(
                "Node requires config or store, use invoke_with_context".to_string(),
            ))
        }
    }

    async fn invoke_with_context(
        &self,
        state: &S,
        config: Option<&RunnableConfig>,
        store: Option<StoreBox>,
    ) -> Result<StateUpdate, LangGraphError> {
        // Try func_with_config_store first (most specific)
        if let Some(ref func) = self.func_with_config_store {
            if let (Some(config), Some(store)) = (config, store) {
                return func(state, config, store.clone()).await;
            } else {
                return Err(LangGraphError::ExecutionError(
                    "Node requires both config and store".to_string(),
                ));
            }
        }

        // Try func_with_config
        if let Some(ref func) = self.func_with_config {
            if let Some(config) = config {
                return func(state, config).await;
            } else {
                return Err(LangGraphError::ExecutionError(
                    "Node requires config".to_string(),
                ));
            }
        }

        // Fall back to func_state_only
        if let Some(ref func) = self.func_state_only {
            func(state).await
        } else {
            Err(LangGraphError::ExecutionError(
                "No valid function signature found".to_string(),
            ))
        }
    }
}

/// Chain node - wraps a Chain trait object
///
/// This node executes a Chain and adds the result to the state.
pub struct ChainNode {
    chain: Arc<dyn Chain>,
    input_key: String,
    output_key: String,
}

impl ChainNode {
    /// Create a new chain node
    ///
    /// # Arguments
    ///
    /// * `chain` - The chain to execute
    /// * `input_key` - The key in the state to use as input (default: "input")
    /// * `output_key` - The key in the state to store the output (default: "output")
    pub fn new(
        chain: Arc<dyn Chain>,
        input_key: Option<String>,
        output_key: Option<String>,
    ) -> Self {
        Self {
            chain,
            input_key: input_key.unwrap_or_else(|| "input".to_string()),
            output_key: output_key.unwrap_or_else(|| "output".to_string()),
        }
    }
}

#[async_trait]
impl<S: State> Node<S> for ChainNode {
    async fn invoke(&self, state: &S) -> Result<StateUpdate, LangGraphError> {
        // Convert state to PromptArgs
        // For MessagesState, we extract the last message as input
        let state_json = serde_json::to_value(state).map_err(LangGraphError::SerializationError)?;

        let mut prompt_args = PromptArgs::new();

        // Try to extract input from state
        if let Some(input_value) = state_json.get(&self.input_key) {
            prompt_args.insert(self.input_key.clone(), input_value.clone());
        } else if let Some(messages) = state_json.get("messages") {
            // For MessagesState, use the last message as input
            if let Some(msg_array) = messages.as_array() {
                if let Some(last_msg) = msg_array.last() {
                    if let Some(content) = last_msg.get("content") {
                        prompt_args.insert(self.input_key.clone(), content.clone());
                    }
                }
            }
        }

        // Execute the chain
        let result = self.chain.call(prompt_args).await?;

        // Create state update
        let mut update = HashMap::new();
        update.insert(
            self.output_key.clone(),
            serde_json::to_value(result.generation)?,
        );

        Ok(update)
    }
}

/// LLM node - wraps an LLM trait object
///
/// This node executes an LLM and adds the result as a message to the state.
pub struct LLMNode {
    llm: Arc<dyn LLM>,
}

impl LLMNode {
    /// Create a new LLM node
    pub fn new(llm: Arc<dyn LLM>) -> Self {
        Self { llm }
    }
}

#[async_trait]
impl<S: State> Node<S> for LLMNode {
    // invoke_with_context uses default implementation (calls invoke)
    // LLM nodes don't typically need config or store

    async fn invoke(&self, state: &S) -> Result<StateUpdate, LangGraphError> {
        // Convert state to messages
        let state_json = serde_json::to_value(state).map_err(LangGraphError::SerializationError)?;

        let messages: Vec<Message> = if let Some(messages_value) = state_json.get("messages") {
            serde_json::from_value(messages_value.clone())
                .map_err(LangGraphError::SerializationError)?
        } else {
            // If no messages, create a default human message
            vec![Message::new_human_message("")]
        };

        // Generate response
        let result = self.llm.generate(&messages).await?;

        // Create state update with new AI message
        let ai_message = Message::new_ai_message(&result.generation);
        let mut update = HashMap::new();
        update.insert(
            "messages".to_string(),
            serde_json::to_value(vec![ai_message])?,
        );

        Ok(update)
    }

    fn get_llm(&self) -> Option<Arc<dyn LLM>> {
        Some(self.llm.clone())
    }
}

/// Agent node - wraps an Agent trait object
///
/// This node executes an Agent and adds the result to the state.
pub struct AgentNode {
    agent: Arc<dyn Agent>,
}

impl AgentNode {
    /// Create a new agent node
    pub fn new(agent: Arc<dyn Agent>) -> Self {
        Self { agent }
    }
}

#[async_trait]
impl<S: State> Node<S> for AgentNode {
    // invoke_with_context uses default implementation (calls invoke)
    // Agent nodes can be extended later if needed

    async fn invoke(&self, state: &S) -> Result<StateUpdate, LangGraphError> {
        // Convert state to PromptArgs
        let state_json = serde_json::to_value(state).map_err(LangGraphError::SerializationError)?;

        let mut prompt_args = PromptArgs::new();

        // Extract input from state
        if let Some(input_value) = state_json.get("input") {
            prompt_args.insert("input".to_string(), input_value.clone());
        } else if let Some(messages) = state_json.get("messages") {
            // For MessagesState, use the last message as input
            if let Some(msg_array) = messages.as_array() {
                if let Some(last_msg) = msg_array.last() {
                    if let Some(content) = last_msg.get("content") {
                        prompt_args.insert("input".to_string(), content.clone());
                    }
                }
            }
        }

        // Execute the agent
        // Note: This is a simplified version. Full agent execution
        // would require handling intermediate steps, tools, etc.
        // For now, we'll use the agent's plan method
        let intermediate_steps = vec![];
        let event = self.agent.plan(&intermediate_steps, prompt_args).await?;

        // Create state update based on agent event
        let mut update = HashMap::new();

        match event {
            crate::schemas::agent::AgentEvent::Finish(finish) => {
                update.insert(
                    "output".to_string(),
                    serde_json::Value::String(finish.output.clone()),
                );
            }
            crate::schemas::agent::AgentEvent::Action(actions) => {
                if let Some(action) = actions.first() {
                    update.insert("action".to_string(), serde_json::to_value(action)?);
                }
            }
        }

        Ok(update)
    }
}

/// Helper function to create a simple function node from a closure
///
/// Supports function signatures:
/// - `Fn(&S) -> Fut` - state only
pub fn function_node<S: State, F, Fut>(name: impl Into<String>, func: F) -> FunctionNode<S>
where
    F: Fn(&S) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<StateUpdate, LangGraphError>> + Send + 'static,
{
    FunctionNode::new(name.into(), func)
}

/// Helper function to create a function node with config
///
/// Supports function signatures:
/// - `Fn(&S, &RunnableConfig) -> Fut` - state and config
pub fn function_node_with_config<S: State, F, Fut>(
    name: impl Into<String>,
    func: F,
) -> FunctionNode<S>
where
    F: Fn(&S, &RunnableConfig) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<StateUpdate, LangGraphError>> + Send + 'static,
{
    FunctionNode::with_config(name.into(), func)
}

/// Helper function to create a function node with config and store
///
/// Supports function signatures:
/// - `Fn(&S, &RunnableConfig, StoreBox) -> Fut` - state, config, and store (owned Arc)
pub fn function_node_with_store<S: State, F, Fut>(
    name: impl Into<String>,
    func: F,
) -> FunctionNode<S>
where
    F: Fn(&S, &RunnableConfig, StoreBox) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<StateUpdate, LangGraphError>> + Send + 'static,
{
    FunctionNode::with_config_store(name.into(), func)
}

#[cfg(test)]
mod tests_subgraph;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::langgraph::state::MessagesState;

    #[tokio::test]
    async fn test_function_node() {
        let node = function_node("test_node", |state: &MessagesState| async move {
            let mut update = HashMap::new();
            update.insert(
                "messages".to_string(),
                serde_json::to_value(vec![Message::new_ai_message("Hello from node")])?,
            );
            Ok(update)
        });

        let state = MessagesState::new();
        let result = node.invoke(&state).await;
        assert!(result.is_ok());
    }
}
