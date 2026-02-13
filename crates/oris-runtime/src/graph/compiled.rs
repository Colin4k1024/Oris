use std::collections::{HashMap, HashSet};
use std::pin::Pin;
use std::sync::Arc;

use async_stream::stream;
use futures::Stream;

use crate::kernel::{Event, EventStore};

use super::{
    edge::{Edge, END, START},
    error::GraphError,
    execution::{
        durability::DurabilityMode, scheduler::NodeScheduler, superstep::SuperStepExecutor,
    },
    interrupts::{
        set_interrupt_context, Interrupt, InterruptContext, InvokeResult, StateOrCommand,
    },
    trace::TraceEvent,
    node::Node,
    persistence::{
        checkpointer::CheckpointerBox,
        config::{CheckpointConfig, RunnableConfig},
        snapshot::StateSnapshot,
        store::StoreBox,
    },
    state::{State, StateUpdate},
    step_result::GraphStepOnceResult,
    streaming::{
        chunk::StreamChunk,
        metadata::{MessageChunk, MessageMetadata},
        mode::StreamMode,
    },
};

/// CompiledGraph - an executable graph ready for execution
///
/// This is created by calling `compile()` on a `StateGraph`.
/// It provides methods to execute the graph with initial state.
pub struct CompiledGraph<S: State> {
    nodes: HashMap<String, Arc<dyn Node<S>>>,
    adjacency: HashMap<String, Vec<Edge<S>>>,
    checkpointer: Option<CheckpointerBox<S>>,
    store: Option<StoreBox>,
    /// Optional kernel EventStore for event-first execution (2.0 pilot).
    event_store: Option<Arc<dyn EventStore>>,
}

impl<S: State + 'static> CompiledGraph<S> {
    /// Get a reference to the nodes (for subgraph persistence propagation)
    pub(crate) fn nodes(&self) -> &HashMap<String, Arc<dyn Node<S>>> {
        &self.nodes
    }

    /// Get a reference to the adjacency list (for subgraph persistence propagation)
    pub(crate) fn adjacency(&self) -> &HashMap<String, Vec<Edge<S>>> {
        &self.adjacency
    }

    /// Get a reference to the checkpointer (for subgraph persistence propagation)
    pub(crate) fn checkpointer(&self) -> &Option<CheckpointerBox<S>> {
        &self.checkpointer
    }

    /// Get a reference to the store (for subgraph persistence propagation)
    pub(crate) fn store(&self) -> &Option<StoreBox> {
        &self.store
    }
}

impl<S: State + 'static> CompiledGraph<S> {
    // Methods moved above after struct definition
}

impl<S: State + 'static> CompiledGraph<S> {
    /// Create a new CompiledGraph (internal use only)
    ///
    /// Use `StateGraph::compile()` to create a CompiledGraph.
    pub(crate) fn new(
        nodes: HashMap<String, Arc<dyn Node<S>>>,
        adjacency: HashMap<String, Vec<Edge<S>>>,
    ) -> Result<Self, GraphError> {
        Ok(Self {
            nodes,
            adjacency,
            checkpointer: None,
            store: None,
            event_store: None,
        })
    }

    /// Create a new CompiledGraph with checkpointer and store
    pub(crate) fn with_persistence(
        nodes: HashMap<String, Arc<dyn Node<S>>>,
        adjacency: HashMap<String, Vec<Edge<S>>>,
        checkpointer: Option<CheckpointerBox<S>>,
        store: Option<StoreBox>,
    ) -> Result<Self, GraphError> {
        Ok(Self {
            nodes,
            adjacency,
            checkpointer,
            store,
            event_store: None,
        })
    }

    /// Attach a kernel EventStore for event-first execution (2.0 pilot).
    /// When set, the interrupt execution path will append kernel events
    /// (StateUpdated, Interrupted, Resumed, Completed) to this store.
    pub fn with_event_store(self, event_store: Arc<dyn EventStore>) -> Self {
        Self {
            event_store: Some(event_store),
            ..self
        }
    }

    /// Invoke the graph with initial state
    ///
    /// Executes the graph from START to END, returning the final state.
    ///
    /// # Arguments
    ///
    /// * `initial_state` - The initial state to start execution with
    ///
    /// # Example
    ///
    /// ```ignore
    /// use oris_runtime::graph::{CompiledGraph, MessagesState};
    /// let graph: CompiledGraph<MessagesState> = // ... create graph
    /// let initial_state = MessagesState::new();
    /// let final_state = graph.invoke(initial_state).await.unwrap();
    /// ```
    pub async fn invoke(&self, initial_state: S) -> Result<S, GraphError> {
        let mut current_state = initial_state;
        let mut current_node = START.to_string();
        let mut visited = HashSet::new();
        let max_iterations = 1000; // Prevent infinite loops
        let mut iterations = 0;

        loop {
            if iterations >= max_iterations {
                return Err(GraphError::ExecutionError(
                    "Maximum iterations reached. Possible infinite loop.".to_string(),
                ));
            }
            iterations += 1;

            // If we've reached END, return the final state
            if current_node == END {
                return Ok(current_state);
            }

            // Check for cycles (simple detection)
            if visited.contains(&current_node) {
                // Allow revisiting nodes, but track it
                log::warn!("Revisiting node: {}", current_node);
            } else {
                visited.insert(current_node.clone());
            }

            // Get edges from current node
            let edges = self
                .adjacency
                .get(&current_node)
                .ok_or_else(|| {
                    GraphError::ExecutionError(format!(
                        "No edges found from node: {}",
                        current_node
                    ))
                })?
                .clone();

            // If we're at START, execute the first node
            if current_node == START {
                if edges.is_empty() {
                    return Err(GraphError::ExecutionError(
                        "No edges from START".to_string(),
                    ));
                }

                // Get the first edge (for START, there should typically be one)
                let edge = &edges[0];
                let next_node = edge.get_target(&current_state).await?;
                current_node = next_node;
                continue;
            }

            // Execute the current node
            let node = self
                .nodes
                .get(&current_node)
                .ok_or_else(|| GraphError::NodeNotFound(current_node.clone()))?;

            // Use invoke for basic invoke method (no config/store available)
            let update = node.invoke(&current_state).await?;

            // Merge the update into the current state
            current_state = self.merge_state_update(&current_state, &update)?;

            // Determine next node based on edges
            if edges.is_empty() {
                return Err(GraphError::ExecutionError(format!(
                    "No edges from node: {}",
                    current_node
                )));
            }

            // For regular edges, take the first one
            // For conditional edges, evaluate the condition
            let edge = &edges[0];
            let next_node = edge.get_target(&current_state).await?;

            // If next node is END, we're done
            if next_node == END {
                return Ok(current_state);
            }

            current_node = next_node;
        }
    }

    /// Merge a state update into the current state
    ///
    /// This handles the merging logic for different state types.
    /// For MessagesState, we use specialized logic.
    /// For other state types, we use the State trait's merge method.
    fn merge_state_update(&self, state: &S, update: &StateUpdate) -> Result<S, GraphError> {
        // Try to handle MessagesState specially
        // For other state types, we'll need to serialize/deserialize
        // This is a limitation of the current design - in a full implementation,
        // we'd have a StateUpdate trait or similar

        // Convert state to JSON to check if it's MessagesState-like
        let state_json = serde_json::to_value(state).map_err(GraphError::SerializationError)?;

        // If state has "messages" field, treat it as MessagesState
        if state_json.get("messages").is_some() {
            // This is a workaround - we need to handle MessagesState specially
            // In practice, we'd use specialization or a different approach
            return self.merge_messages_state_update(state, update);
        }

        // For other state types, create a new state from the update and merge
        // This requires the state to be serializable/deserializable
        let update_json = serde_json::to_value(update).map_err(GraphError::SerializationError)?;

        // Try to deserialize update as state and merge
        // This is a simplified approach
        let update_state: S = serde_json::from_value(update_json.clone()).map_err(|_| {
            GraphError::StateMergeError("Cannot deserialize update as state".to_string())
        })?;

        Ok(state.merge(&update_state))
    }

    /// Merge update for MessagesState (specialized handling)
    fn merge_messages_state_update(
        &self,
        state: &S,
        update: &StateUpdate,
    ) -> Result<S, GraphError> {
        use super::state::{apply_update_to_messages_state, MessagesState};

        // Try to convert state to MessagesState
        let state_json = serde_json::to_value(state).map_err(GraphError::SerializationError)?;

        let messages_state: MessagesState = if let Some(messages_value) = state_json.get("messages")
        {
            if let Ok(messages) = serde_json::from_value::<Vec<crate::schemas::messages::Message>>(
                messages_value.clone(),
            ) {
                MessagesState::with_messages(messages)
            } else {
                MessagesState::new()
            }
        } else {
            MessagesState::new()
        };

        // Apply update
        let updated_state = apply_update_to_messages_state(&messages_state, update);

        // Convert back to S
        let updated_json =
            serde_json::to_value(&updated_state).map_err(GraphError::SerializationError)?;
        serde_json::from_value(updated_json).map_err(GraphError::SerializationError)
    }

    /// Execute a single graph step from (current_state, current_node).
    /// Does not append events; used by GraphStepFnAdapter for kernel-driven execution.
    ///
    /// Currently requires exactly one outgoing edge from START and from each node;
    /// multi-exit / conditional routing is not supported and will return an error.
    pub async fn step_once(
        &self,
        current_state: &S,
        current_node: &str,
        config: Option<&RunnableConfig>,
    ) -> Result<GraphStepOnceResult<S>, GraphError> {
        let store = self.store.clone();

        let node_to_run = if current_node.is_empty() || current_node == START {
            let edges = self.adjacency.get(START).ok_or_else(|| {
                GraphError::ExecutionError("No edges from START".to_string())
            })?;
            if edges.is_empty() {
                return Err(GraphError::ExecutionError("No edges from START".to_string()));
            }
            if edges.len() != 1 {
                return Err(GraphError::ExecutionError(
                    "step_once requires exactly one outgoing edge from START".to_string(),
                ));
            }
            let edge = &edges[0];
            edge.get_target(current_state).await?
        } else if current_node == END {
            return Ok(GraphStepOnceResult::Complete {
                state: current_state.clone(),
            });
        } else {
            current_node.to_string()
        };

        if node_to_run == END {
            return Ok(GraphStepOnceResult::Complete {
                state: current_state.clone(),
            });
        }

        let edges = self.adjacency.get(&node_to_run).ok_or_else(|| {
            GraphError::ExecutionError(format!("No edges from node: {}", node_to_run))
        })?;

        let node = self.nodes.get(&node_to_run).ok_or_else(|| {
            GraphError::NodeNotFound(node_to_run.clone())
        })?;

        let update_result = node
            .invoke_with_context(current_state, config, store)
            .await;

        match update_result {
            Ok(update) => {
                let new_state = self.merge_state_update(current_state, &update)?;
                if edges.is_empty() {
                    return Ok(GraphStepOnceResult::Complete { state: new_state });
                }
                if edges.len() != 1 {
                    return Err(GraphError::ExecutionError(
                        "step_once requires exactly one outgoing edge per node".to_string(),
                    ));
                }
                let edge = &edges[0];
                let next_node = edge.get_target(&new_state).await?;
                if next_node == END {
                    Ok(GraphStepOnceResult::Complete { state: new_state })
                } else {
                    Ok(GraphStepOnceResult::Emit {
                        executed_node: node_to_run.clone(),
                        new_state,
                        next_node,
                    })
                }
            }
            Err(GraphError::InterruptError(interrupt_err)) => {
                let value = interrupt_err.value().clone();
                Ok(GraphStepOnceResult::Interrupt {
                    state: current_state.clone(),
                    value,
                })
            }
            Err(e) => Err(e),
        }
    }

    /// Stream the graph execution, yielding events as they occur
    ///
    /// This method executes the graph and yields events for each node execution,
    /// allowing you to monitor the graph's progress in real-time.
    ///
    /// # Arguments
    ///
    /// * `initial_state` - The initial state to start execution with
    ///
    /// # Returns
    ///
    /// A stream of `StreamEvent` values representing the execution progress
    pub fn stream<'a>(
        &'a self,
        initial_state: S,
    ) -> Pin<Box<dyn Stream<Item = StreamEvent<S>> + Send + 'a>> {
        self.stream_with_options(initial_state, StreamOptions::default())
    }

    /// Stream with options (modes and subgraphs)
    pub fn stream_with_options<'a>(
        &'a self,
        initial_state: S,
        options: StreamOptions,
    ) -> Pin<Box<dyn Stream<Item = StreamEvent<S>> + Send + 'a>> {
        self.stream_internal(initial_state, options.stream_modes, options.subgraphs)
    }

    /// Internal stream method with optional stream modes and subgraphs support
    fn stream_internal<'a>(
        &'a self,
        initial_state: S,
        stream_modes: Option<Vec<StreamMode>>,
        subgraphs: bool,
    ) -> Pin<Box<dyn Stream<Item = StreamEvent<S>> + Send + 'a>> {
        let nodes = self.nodes.clone();
        let adjacency = self.adjacency.clone();

        Box::pin(stream! {
            let mut current_state = initial_state;
            let mut current_node = START.to_string();
            let mut visited = HashSet::new();
            let max_iterations = 1000;
            let mut iterations = 0;

            loop {
                if iterations >= max_iterations {
                    yield StreamEvent::Error {
                        error: std::sync::Arc::new(GraphError::ExecutionError(
                            "Maximum iterations reached. Possible infinite loop.".to_string(),
                        )),
                    };
                    return;
                }
                iterations += 1;

                // If we've reached END, yield final event and return
                if current_node == END {
                    yield StreamEvent::GraphEnd {
                        final_state: current_state,
                    };
                    return;
                }

                // Track visited nodes
                if !visited.contains(&current_node) {
                    visited.insert(current_node.clone());
                }

                // Get edges from current node
                let edges = match adjacency.get(&current_node) {
                    Some(edges) => edges.clone(),
                    None => {
                        yield StreamEvent::Error {
                            error: std::sync::Arc::new(GraphError::ExecutionError(format!(
                                "No edges found from node: {}",
                                current_node
                            ))),
                        };
                        return;
                    }
                };

                // If we're at START, move to first node
                if current_node == START {
                    if edges.is_empty() {
                            yield StreamEvent::Error {
                                error: std::sync::Arc::new(GraphError::ExecutionError(
                                    "No edges from START".to_string(),
                                )),
                            };
                        return;
                    }

                    let edge = &edges[0];
                    match edge.get_target(&current_state).await {
                        Ok(next_node) => {
                            current_node = next_node;
                            continue;
                        }
                        Err(e) => {
                            yield StreamEvent::Error {
                                error: std::sync::Arc::new(e),
                            };
                            return;
                        }
                    }
                }

                // Yield node start event
                yield StreamEvent::NodeStart {
                    node: current_node.clone(),
                    state: current_state.clone(),
                    path: Vec::new(), // Empty path for top-level nodes
                };

                // Execute the current node
                let node = match nodes.get(&current_node) {
                    Some(node) => node.clone(),
                    None => {
                        yield StreamEvent::Error {
                            error: std::sync::Arc::new(GraphError::NodeNotFound(current_node.clone())),
                        };
                        return;
                    }
                };

                // Check if this is a subgraph node and subgraphs streaming is enabled
                let is_subgraph = subgraphs && node.get_subgraph().is_some();

                // Check if we need to stream LLM tokens
                let needs_message_streaming = stream_modes.as_ref()
                    .map(|modes| modes.contains(&StreamMode::Messages))
                    .unwrap_or(false);

                let update = if is_subgraph {
                    // This is a subgraph node - stream its execution
                    let subgraph = node.get_subgraph().unwrap();
                    let subgraph_path = vec![current_node.clone()]; // Path prefix for subgraph events

                    // Create stream options for subgraph
                    let subgraph_options = StreamOptions {
                        stream_modes: stream_modes.clone(),
                        subgraphs: subgraphs, // Recursively enable subgraphs
                    };

                    // Stream subgraph execution
                    use futures::StreamExt;
                    let mut subgraph_stream = subgraph.stream_with_options(current_state.clone(), subgraph_options);
                    let mut final_state = current_state.clone();

                    while let Some(sub_event) = subgraph_stream.next().await {
                        match sub_event {
                            StreamEvent::NodeStart { node: sub_node, state, path: sub_path, .. } => {
                                // Build full path: [parent_node, ...sub_path, sub_node]
                                let mut full_path = subgraph_path.clone();
                                full_path.extend(sub_path);
                                full_path.push(sub_node.clone());

                                yield StreamEvent::NodeStart {
                                    node: sub_node,
                                    state,
                                    path: full_path,
                                };
                            }
                            StreamEvent::NodeEnd { node: sub_node, state, update: sub_update, path: sub_path, .. } => {
                                // Build full path
                                let mut full_path = subgraph_path.clone();
                                full_path.extend(sub_path);
                                full_path.push(sub_node.clone());

                                yield StreamEvent::NodeEnd {
                                    node: sub_node,
                                    state: state.clone(),
                                    update: sub_update,
                                    path: full_path,
                                };

                                // Update final state
                                final_state = state;
                            }
                            StreamEvent::MessageChunk { node: sub_node, chunk, metadata, path: sub_path, .. } => {
                                // Build full path
                                let mut full_path = subgraph_path.clone();
                                full_path.extend(sub_path);
                                full_path.push(sub_node.clone());

                                yield StreamEvent::MessageChunk {
                                    node: sub_node,
                                    chunk,
                                    metadata,
                                    path: full_path,
                                };
                            }
                            StreamEvent::CustomData { node: sub_node, data, path: sub_path, .. } => {
                                // Build full path
                                let mut full_path = subgraph_path.clone();
                                full_path.extend(sub_path);
                                full_path.push(sub_node.clone());

                                yield StreamEvent::CustomData {
                                    node: sub_node,
                                    data,
                                    path: full_path,
                                };
                            }
                            StreamEvent::GraphEnd { final_state: sub_final_state } => {
                                // Subgraph completed - use its final state
                                final_state = sub_final_state;
                            }
                            StreamEvent::Error { error } => {
                                yield StreamEvent::Error { error };
                                return;
                            }
                        }
                    }

                    // Convert final state to update
                    let state_json = match serde_json::to_value(&final_state) {
                        Ok(json) => json,
                        Err(e) => {
                            yield StreamEvent::Error {
                                error: std::sync::Arc::new(GraphError::SerializationError(e)),
                            };
                            return;
                        }
                    };

                    let mut update = HashMap::new();
                    if let serde_json::Value::Object(map) = state_json {
                        for (key, value) in map {
                            update.insert(key, value);
                        }
                    }
                    update
                } else if needs_message_streaming {
                    // Try to get LLM from node for streaming
                    if let Some(llm) = node.get_llm() {
                        // Convert state to messages
                        let state_json = match serde_json::to_value(&current_state) {
                            Ok(json) => json,
                            Err(e) => {
                                yield StreamEvent::Error {
                                    error: std::sync::Arc::new(GraphError::SerializationError(e)),
                                };
                                return;
                            }
                        };

                        let messages: Vec<crate::schemas::messages::Message> = if let Some(messages_value) = state_json.get("messages") {
                            match serde_json::from_value(messages_value.clone()) {
                                Ok(msgs) => msgs,
                                Err(e) => {
                                    yield StreamEvent::Error {
                                    error: std::sync::Arc::new(GraphError::SerializationError(e)),
                                };
                                    return;
                                }
                            }
                        } else {
                            vec![crate::schemas::messages::Message::new_human_message("")]
                        };

                        // Stream LLM tokens
                        let mut stream_result = match llm.stream(&messages).await {
                            Ok(stream) => stream,
                            Err(e) => {
                                yield StreamEvent::Error {
                                    error: std::sync::Arc::new(GraphError::LLMError(e.to_string())),
                                };
                                return;
                            }
                        };

                        use futures::StreamExt;
                        let mut full_content = String::new();
                        let metadata = MessageMetadata::new(current_node.clone());

                        while let Some(chunk_result) = stream_result.next().await {
                            match chunk_result {
                                Ok(stream_data) => {
                                    full_content.push_str(&stream_data.content);

                                    // Yield message chunk event
                                    yield StreamEvent::MessageChunk {
                                        node: current_node.clone(),
                                        chunk: stream_data,
                                        metadata: metadata.clone(),
                                        path: Vec::new(), // Empty path for top-level nodes
                                    };
                                }
                                Err(e) => {
                                    yield StreamEvent::Error {
                                        error: std::sync::Arc::new(GraphError::LLMError(e.to_string())),
                                    };
                                    return;
                                }
                            }
                        }

                        // Create state update with full content
                        let ai_message = crate::schemas::messages::Message::new_ai_message(&full_content);
                        let mut update = HashMap::new();
                        match serde_json::to_value(vec![ai_message]) {
                            Ok(msg_value) => {
                                update.insert("messages".to_string(), msg_value);
                            }
                            Err(e) => {
                                yield StreamEvent::Error {
                                    error: std::sync::Arc::new(GraphError::SerializationError(e)),
                                };
                                return;
                            }
                        }
                        update
                    } else {
                        // Not an LLM node, use invoke_with_context
                        // Note: stream_internal doesn't have config/store, so pass None
                        match node.invoke_with_context(&current_state, None, None).await {
                            Ok(update) => update,
                            Err(e) => {
                                yield StreamEvent::Error {
                                    error: std::sync::Arc::new(e),
                                };
                                return;
                            }
                        }
                    }
                } else {
                    // No message streaming needed, use invoke_with_context
                    // Note: stream_internal doesn't have config/store, so pass None
                    match node.invoke_with_context(&current_state, None, None).await {
                        Ok(update) => update,
                        Err(e) => {
                            yield StreamEvent::Error {
                                error: std::sync::Arc::new(e),
                            };
                            return;
                        }
                    }
                };

                // Merge the update into the current state
                current_state = match self.merge_state_update(&current_state, &update) {
                    Ok(new_state) => new_state,
                    Err(e) => {
                        yield StreamEvent::Error {
                                    error: std::sync::Arc::new(e),
                                };
                        return;
                    }
                };

                // Yield node end event
                yield StreamEvent::NodeEnd {
                    node: current_node.clone(),
                    state: current_state.clone(),
                    update: update.clone(),
                    path: Vec::new(), // Empty path for top-level nodes
                };

                // Determine next node based on edges
                if edges.is_empty() {
                    yield StreamEvent::Error {
                        error: std::sync::Arc::new(GraphError::ExecutionError(format!(
                            "No edges from node: {}",
                            current_node
                        ))),
                    };
                    return;
                }

                // Get next node
                let edge = &edges[0];
                let next_node = match edge.get_target(&current_state).await {
                    Ok(node) => node,
                    Err(e) => {
                        yield StreamEvent::Error {
                                    error: std::sync::Arc::new(e),
                                };
                        return;
                    }
                };

                // If next node is END, yield final event
                if next_node == END {
                    yield StreamEvent::GraphEnd {
                        final_state: current_state,
                    };
                    return;
                }

                current_node = next_node;
            }
        })
    }

    /// Stream the graph execution with a specific stream mode
    ///
    /// This method executes the graph and yields chunks based on the specified stream mode.
    ///
    /// # Arguments
    ///
    /// * `initial_state` - The initial state to start execution with
    /// * `mode` - The stream mode (values, updates, messages, custom, or debug)
    ///
    /// # Returns
    ///
    /// A stream of `StreamChunk` values filtered by the stream mode
    pub fn stream_with_mode<'a>(
        &'a self,
        initial_state: S,
        mode: StreamMode,
    ) -> Pin<Box<dyn Stream<Item = StreamChunk<S>> + Send + 'a>> {
        // Pass stream modes to enable LLM streaming if messages mode is requested
        let stream_modes = if mode == StreamMode::Messages {
            Some(vec![StreamMode::Messages])
        } else {
            None
        };
        let event_stream = self.stream_internal(initial_state, stream_modes, false);
        Box::pin(stream! {
            use futures::StreamExt;
            let mut event_stream = event_stream;

            while let Some(event) = event_stream.next().await {
                if let Some(chunk) = Self::convert_event_to_chunk(&event, mode) {
                    yield chunk;
                }
            }
        })
    }

    /// Stream the graph execution with multiple stream modes
    ///
    /// This method executes the graph and yields chunks for all specified stream modes.
    ///
    /// # Arguments
    ///
    /// * `initial_state` - The initial state to start execution with
    /// * `modes` - A vector of stream modes to enable
    ///
    /// # Returns
    ///
    /// A stream of `(StreamMode, StreamChunk)` tuples, one for each enabled mode
    pub fn stream_with_modes<'a>(
        &'a self,
        initial_state: S,
        modes: Vec<StreamMode>,
    ) -> Pin<Box<dyn Stream<Item = (StreamMode, StreamChunk<S>)> + Send + 'a>> {
        // Pass stream modes to enable LLM streaming if messages mode is requested
        let stream_modes = if modes.contains(&StreamMode::Messages) {
            Some(modes.clone())
        } else {
            None
        };
        let event_stream = self.stream_internal(initial_state, stream_modes, false);
        Box::pin(stream! {
            use futures::StreamExt;
            let mut event_stream = event_stream;

            while let Some(event) = event_stream.next().await {
                for mode in &modes {
                    if let Some(chunk) = Self::convert_event_to_chunk(&event, *mode) {
                        yield (*mode, chunk);
                    }
                }
            }
        })
    }

    /// Convert a StreamEvent to a StreamChunk based on the stream mode
    fn convert_event_to_chunk(event: &StreamEvent<S>, mode: StreamMode) -> Option<StreamChunk<S>> {
        use super::streaming::metadata::DebugInfo;

        match (event, mode) {
            // Values mode: extract full state from NodeEnd
            (StreamEvent::NodeEnd { state, .. }, StreamMode::Values) => Some(StreamChunk::Values {
                state: state.clone(),
            }),
            (StreamEvent::GraphEnd { final_state }, StreamMode::Values) => {
                Some(StreamChunk::Values {
                    state: final_state.clone(),
                })
            }

            // Updates mode: extract update from NodeEnd
            (StreamEvent::NodeEnd { node, update, .. }, StreamMode::Updates) => {
                Some(StreamChunk::Updates {
                    node: node.clone(),
                    update: update.clone(),
                })
            }

            // Messages mode: extract from MessageChunk event
            (
                StreamEvent::MessageChunk {
                    chunk, metadata, ..
                },
                StreamMode::Messages,
            ) => Some(StreamChunk::Messages {
                chunk: MessageChunk::new(chunk.clone(), metadata.clone()),
            }),

            // Custom mode: extract from CustomData event
            (StreamEvent::CustomData { node, data, .. }, StreamMode::Custom) => {
                Some(StreamChunk::Custom {
                    node: node.clone(),
                    data: data.clone(),
                })
            }

            // Debug mode: convert all events to debug info
            (_, StreamMode::Debug) => {
                let debug_info = match event {
                    StreamEvent::NodeStart { node, .. } => {
                        DebugInfo::with_node("NodeStart", node.clone())
                    }
                    StreamEvent::NodeEnd {
                        node,
                        state,
                        update,
                        ..
                    } => {
                        let mut info = DebugInfo::with_node("NodeEnd", node.clone());
                        info =
                            info.with_info("state".to_string(), serde_json::to_value(state).ok()?);
                        info = info
                            .with_info("update".to_string(), serde_json::to_value(update).ok()?);
                        info
                    }
                    StreamEvent::GraphEnd { final_state } => {
                        let mut info = DebugInfo::new("GraphEnd");
                        info = info.with_info(
                            "final_state".to_string(),
                            serde_json::to_value(final_state).ok()?,
                        );
                        info
                    }
                    StreamEvent::Error { error } => DebugInfo::new("Error")
                        .with_info("error".to_string(), serde_json::json!(error.to_string())),
                    StreamEvent::MessageChunk {
                        node,
                        chunk,
                        metadata,
                        ..
                    } => {
                        let mut info = DebugInfo::with_node("MessageChunk", node.clone());
                        info =
                            info.with_info("chunk".to_string(), serde_json::to_value(chunk).ok()?);
                        info = info.with_info(
                            "metadata".to_string(),
                            serde_json::to_value(metadata).ok()?,
                        );
                        info
                    }
                    StreamEvent::CustomData { node, data, .. } => {
                        let mut info = DebugInfo::with_node("CustomData", node.clone());
                        info = info.with_info("data".to_string(), data.clone());
                        info
                    }
                };
                Some(StreamChunk::Debug { info: debug_info })
            }

            // Other combinations don't match
            _ => None,
        }
    }

    /// Invoke the graph with initial state and config (with persistence support)
    ///
    /// This method supports checkpointing and can resume from a checkpoint
    /// if checkpoint_id is provided in the config.
    ///
    /// By default, this uses super-step execution for parallel node execution.
    ///
    /// This method also supports interrupts. If an interrupt occurs, it returns
    /// an `InvokeResult` with interrupt information. Use `invoke_with_config_interrupt`
    /// to get the full result including interrupt information.
    ///
    /// # Arguments
    ///
    /// * `initial_state` - The initial state, or `None` to resume from a checkpoint
    ///   (requires `checkpoint_id` in config)
    /// * `config` - The runnable configuration (thread_id, checkpoint_id, etc.)
    ///
    /// # Time-Travel Support
    ///
    /// Pass `None` as `initial_state` along with a `checkpoint_id` in config to
    /// resume execution from a historical checkpoint. This creates a new fork in
    /// the execution history.
    pub async fn invoke_with_config(
        &self,
        initial_state: Option<S>,
        config: &RunnableConfig,
    ) -> Result<S, GraphError> {
        let state_or_command = match initial_state {
            Some(state) => StateOrCommand::State(state),
            None => {
                // None input - load from checkpoint (by checkpoint_id or latest for crash recovery)
                let checkpoint_config = CheckpointConfig::from_config(config)?;
                let checkpointer = self.checkpointer.as_ref().ok_or_else(|| {
                    GraphError::ExecutionError(
                        "Checkpointer is required to resume from checkpoint".to_string(),
                    )
                })?;
                let snapshot = checkpointer
                    .get(
                        &checkpoint_config.thread_id,
                        checkpoint_config.checkpoint_id.as_deref(),
                    )
                    .await
                    .map_err(|e| {
                        GraphError::ExecutionError(format!("Failed to load checkpoint: {}", e))
                    })?;
                let snapshot = snapshot.ok_or_else(|| {
                    GraphError::ExecutionError(format!(
                        "No checkpoint found for thread_id: {}",
                        checkpoint_config.thread_id
                    ))
                })?;
                StateOrCommand::State(snapshot.values)
            }
        };
        let result = self
            .invoke_with_config_interrupt(state_or_command, config)
            .await?;
        Ok(result.state)
    }

    /// Invoke the graph with initial state and config, supporting interrupts
    ///
    /// This method supports checkpointing, resuming from checkpoints, and interrupts.
    /// It returns an `InvokeResult` that includes interrupt information if an interrupt occurred.
    ///
    /// # Arguments
    ///
    /// * `initial_state` - The initial state or Command to resume from
    /// * `config` - The runnable configuration (thread_id, checkpoint_id, etc.)
    ///
    /// # Returns
    ///
    /// An `InvokeResult` containing the final state and any interrupt information
    pub async fn invoke_with_config_interrupt(
        &self,
        initial_state: StateOrCommand<S>,
        config: &RunnableConfig,
    ) -> Result<InvokeResult<S>, GraphError> {
        let checkpoint_config = CheckpointConfig::from_config(config)?;
        let thread_id = &checkpoint_config.thread_id;

        // Check if checkpointer is available (required for interrupts)
        let checkpointer = self.checkpointer.as_ref().ok_or_else(|| {
            GraphError::ExecutionError("Checkpointer is required for interrupt support".to_string())
        })?;

        // Handle Command input or regular state
        let (current_state, resume_values, parent_config) = match initial_state {
            StateOrCommand::State(state) => {
                // Regular state input
                // Check if we should load from checkpoint (time-travel)
                let (state, parent) = if let Some(checkpoint_id) = &checkpoint_config.checkpoint_id
                {
                    let snapshot = checkpointer
                        .get(thread_id, Some(checkpoint_id))
                        .await
                        .map_err(|e| {
                            GraphError::ExecutionError(format!("Failed to load checkpoint: {}", e))
                        })?;

                    let snapshot = snapshot.ok_or_else(|| {
                        GraphError::ExecutionError(format!(
                            "Checkpoint not found: {}",
                            checkpoint_id
                        ))
                    })?;

                    // Record parent config for fork tracking
                    let parent = Some(snapshot.config.clone());
                    (snapshot.values, parent)
                } else {
                    (state, None)
                };
                (state, Vec::new(), parent)
            }
            StateOrCommand::Command(cmd) => {
                // Command input - resume from checkpoint
                // Get the latest checkpoint
                let snapshot = checkpointer.get(thread_id, None).await.map_err(|e| {
                    GraphError::ExecutionError(format!("Failed to load checkpoint: {}", e))
                })?;

                let snapshot = snapshot.ok_or_else(|| {
                    GraphError::ExecutionError(format!(
                        "No checkpoint found for thread: {}",
                        thread_id
                    ))
                })?;

                // Extract resume values from command
                let resume_values = if let Some(resume_value) = cmd.resume_value() {
                    vec![resume_value.clone()]
                } else {
                    Vec::new()
                };

                // Record parent config for fork tracking
                let parent = Some(snapshot.config.clone());
                (snapshot.values, resume_values, parent)
            }
        };

        // Build trace: push ResumeReceived when resuming with values (before moving resume_values)
        let mut trace = Vec::new();
        for v in &resume_values {
            trace.push(TraceEvent::ResumeReceived { value: v.clone() });
        }

        // Event-first (2.0): append Resumed events when resuming
        if let Some(es) = &self.event_store {
            for v in &resume_values {
                es.append(thread_id, &[Event::Resumed { value: v.clone() }])
                    .map_err(|e| GraphError::ExecutionError(e.to_string()))?;
            }
        }

        // Set up interrupt context with resume values
        let interrupt_ctx = if resume_values.is_empty() {
            InterruptContext::new()
        } else {
            InterruptContext::with_resume_values(resume_values)
        };

        // Update checkpoint_config with parent if we're resuming from a checkpoint
        let mut checkpoint_config = checkpoint_config.clone();
        if let Some(ref _parent) = parent_config {
            checkpoint_config.checkpoint_id = None; // Clear checkpoint_id to create new fork
                                                    // Store parent in the checkpoint when saving
        }

        // Create RunnableConfig from checkpoint_config for nodes
        let mut runnable_config =
            RunnableConfig::with_thread_id(checkpoint_config.thread_id.clone());
        if let Some(checkpoint_id) = &checkpoint_config.checkpoint_id {
            runnable_config.configurable.insert(
                "checkpoint_id".to_string(),
                serde_json::json!(checkpoint_id),
            );
        }

        // Execute with interrupt context
        let result = set_interrupt_context(interrupt_ctx, async {
            self.execute_with_interrupt_support(
                current_state,
                &checkpoint_config,
                parent_config.as_ref(),
                Some(&runnable_config),
                self.store.clone(),
                &mut trace,
                self.event_store.as_ref(),
                &checkpoint_config.thread_id,
            )
            .await
        })
        .await;

        result
    }

    /// Execute graph with interrupt support
    ///
    /// Internal method that executes the graph and handles interrupts.
    /// Fills `trace` with StepCompleted, InterruptReached events.
    async fn execute_with_interrupt_support(
        &self,
        initial_state: S,
        checkpoint_config: &CheckpointConfig,
        parent_config: Option<&CheckpointConfig>,
        config: Option<&RunnableConfig>,
        store: Option<StoreBox>,
        trace: &mut Vec<TraceEvent>,
        event_store: Option<&Arc<dyn EventStore>>,
        run_id: &String,
    ) -> Result<InvokeResult<S>, GraphError> {
        let mut current_state = initial_state;
        let mut current_node = START.to_string();
        let mut visited = HashSet::new();
        let max_iterations = 1000;
        let mut iterations = 0;

        loop {
            if iterations >= max_iterations {
                return Err(GraphError::ExecutionError(
                    "Maximum iterations reached. Possible infinite loop.".to_string(),
                ));
            }
            iterations += 1;

            if current_node == END {
                if let Some(es) = event_store {
                    es.append(run_id, &[Event::Completed])
                        .map_err(|e| GraphError::ExecutionError(e.to_string()))?;
                }
                return Ok(InvokeResult::new_with_trace(
                    current_state,
                    std::mem::take(trace),
                ));
            }

            if !visited.contains(&current_node) {
                visited.insert(current_node.clone());
            }

            let edges = self
                .adjacency
                .get(&current_node)
                .ok_or_else(|| {
                    GraphError::ExecutionError(format!(
                        "No edges found from node: {}",
                        current_node
                    ))
                })?
                .clone();

            if current_node == START {
                if edges.is_empty() {
                    return Err(GraphError::ExecutionError(
                        "No edges from START".to_string(),
                    ));
                }
                let edge = &edges[0];
                let next_node = edge.get_target(&current_state).await?;
                current_node = next_node;
                continue;
            }

            // Execute the current node
            let node = self
                .nodes
                .get(&current_node)
                .ok_or_else(|| GraphError::NodeNotFound(current_node.clone()))?;

            // Event-first (2.0): append ActionRequested before node execution (node step as action)
            let action_id = if event_store.is_some() {
                Some(format!("{}-{}", run_id, current_node))
            } else {
                None
            };
            if let (Some(es), Some(ref aid)) = (event_store, &action_id) {
                let payload = serde_json::to_value(&current_state)
                    .map_err(|e| GraphError::ExecutionError(e.to_string()))?;
                es.append(
                    run_id,
                    &[Event::ActionRequested {
                        action_id: aid.clone(),
                        payload,
                    }],
                )
                .map_err(|e| GraphError::ExecutionError(e.to_string()))?;
            }

            // Execute node and handle interrupts
            // Use invoke_with_context to support config and store
            let update_result = node
                .invoke_with_context(&current_state, config, store.clone())
                .await;

            match update_result {
                Ok(update) => {
                    // Event-first (2.0): append ActionSucceeded after node success
                    if let (Some(es), Some(ref aid)) = (event_store, &action_id) {
                        let output = serde_json::to_value(&update)
                            .map_err(|e| GraphError::ExecutionError(e.to_string()))?;
                        es.append(
                            run_id,
                            &[Event::ActionSucceeded {
                                action_id: aid.clone(),
                                output,
                            }],
                        )
                        .map_err(|e| GraphError::ExecutionError(e.to_string()))?;
                    }
                    // Node executed successfully, merge state
                    trace.push(TraceEvent::StepCompleted {
                        node: current_node.clone(),
                    });
                    current_state = self.merge_state_update(&current_state, &update)?;
                    // Event-first (2.0): append StateUpdated after each node
                    if let Some(es) = event_store {
                        let payload = serde_json::to_value(&current_state)
                            .map_err(|e| GraphError::ExecutionError(e.to_string()))?;
                        es.append(
                            run_id,
                            &[Event::StateUpdated {
                                step_id: Some(current_node.clone()),
                                payload,
                            }],
                        )
                        .map_err(|e| GraphError::ExecutionError(e.to_string()))?;
                    }
                }
                Err(GraphError::InterruptError(interrupt_err)) => {
                    // Interrupt occurred - save checkpoint and return
                    let interrupt_value = interrupt_err.value().clone();
                    trace.push(TraceEvent::InterruptReached {
                        value: interrupt_value.clone(),
                    });
                    // Event-first (2.0): pair ActionRequested with ActionFailed(Interrupt) then Interrupted
                    if let (Some(es), Some(ref aid)) = (event_store, &action_id) {
                        es.append(
                            run_id,
                            &[
                                Event::ActionFailed {
                                    action_id: aid.clone(),
                                    error: "Interrupt".to_string(),
                                },
                                Event::Interrupted {
                                    value: interrupt_value.clone(),
                                },
                            ],
                        )
                        .map_err(|e| GraphError::ExecutionError(e.to_string()))?;
                    } else if let Some(es) = event_store {
                        es.append(
                            run_id,
                            &[Event::Interrupted {
                                value: interrupt_value.clone(),
                            }],
                        )
                        .map_err(|e| GraphError::ExecutionError(e.to_string()))?;
                    }
                    let interrupt = Interrupt::new(interrupt_value);

                    // Save checkpoint at interrupt point
                    // Note: checkpointer should always be available when using interrupt support
                    // (checked in invoke_with_config_interrupt)
                    let mut snapshot = if let Some(parent) = parent_config {
                        // Create snapshot with parent config for fork tracking
                        StateSnapshot::with_parent(
                            current_state.clone(),
                            vec![current_node.clone()],
                            checkpoint_config.clone(),
                            parent.clone(),
                        )
                    } else {
                        StateSnapshot::new(
                            current_state.clone(),
                            vec![current_node.clone()],
                            checkpoint_config.clone(),
                        )
                    };
                    if let Some(es) = event_store {
                        let seq = es
                            .head(run_id)
                            .map_err(|e| GraphError::ExecutionError(e.to_string()))?;
                        snapshot = snapshot.with_at_seq(seq);
                    }
                    if let Some(checkpointer) = &self.checkpointer {
                        checkpointer
                            .put(checkpoint_config.thread_id.as_str(), &snapshot)
                            .await
                            .map_err(|e| {
                                GraphError::ExecutionError(format!(
                                    "Failed to save checkpoint: {}",
                                    e
                                ))
                            })?;
                    }

                    return Ok(InvokeResult::with_interrupt_and_trace(
                        current_state,
                        vec![interrupt],
                        std::mem::take(trace),
                    ));
                }
                Err(e) => {
                    // Event-first (2.0): append ActionFailed on node execution error
                    if let (Some(es), Some(ref aid)) = (event_store, &action_id) {
                        let _ = es.append(
                            run_id,
                            &[Event::ActionFailed {
                                action_id: aid.clone(),
                                error: e.to_string(),
                            }],
                        );
                    }
                    return Err(e);
                }
            }

            // Determine next node
            if edges.is_empty() {
                return Err(GraphError::ExecutionError(format!(
                    "No edges from node: {}",
                    current_node
                )));
            }

            let edge = &edges[0];
            let next_node = edge.get_target(&current_state).await?;

            if next_node == END {
                if let Some(es) = event_store {
                    es.append(run_id, &[Event::Completed])
                        .map_err(|e| GraphError::ExecutionError(e.to_string()))?;
                }
                return Ok(InvokeResult::new_with_trace(
                    current_state,
                    std::mem::take(trace),
                ));
            }

            current_node = next_node;
        }
    }

    /// Invoke the graph with initial state, config, and durability mode
    ///
    /// This method supports checkpointing, resuming from checkpoints, and
    /// configurable durability modes (exit/async/sync).
    ///
    /// Uses super-step execution model for parallel node execution.
    ///
    /// # Arguments
    ///
    /// * `initial_state` - The initial state, or `None` to resume from a checkpoint
    ///   (requires `checkpoint_id` in config)
    /// * `config` - The runnable configuration (thread_id, checkpoint_id, etc.)
    /// * `durability_mode` - The durability mode for checkpoint saving
    pub async fn invoke_with_config_and_mode(
        &self,
        initial_state: Option<S>,
        config: &RunnableConfig,
        durability_mode: DurabilityMode,
    ) -> Result<S, GraphError> {
        let checkpoint_config = CheckpointConfig::from_config(config)?;
        let thread_id = &checkpoint_config.thread_id;

        // Determine current state and parent_config: from input, checkpoint (by id or latest), or error
        let (current_state, parent_config) = match initial_state {
            Some(state) => {
                // If checkpoint_id is provided, it takes precedence (time-travel)
                if let Some(checkpoint_id) = &checkpoint_config.checkpoint_id {
                    if let Some(checkpointer) = &self.checkpointer {
                        let snapshot = checkpointer
                            .get(thread_id, Some(checkpoint_id))
                            .await
                            .map_err(|e| {
                                GraphError::ExecutionError(format!(
                                    "Failed to load checkpoint: {}",
                                    e
                                ))
                            })?;

                        let snapshot = snapshot.ok_or_else(|| {
                            GraphError::ExecutionError(format!(
                                "Checkpoint not found: {}",
                                checkpoint_id
                            ))
                        })?;
                        (snapshot.values, Some(checkpoint_config.clone()))
                    } else {
                        return Err(GraphError::ExecutionError(
                            "Checkpointer not configured but checkpoint_id provided".to_string(),
                        ));
                    }
                } else {
                    (state, None)
                }
            }
            None => {
                // Resume from checkpoint: either by checkpoint_id or from latest (crash recovery)
                let checkpointer = self.checkpointer.as_ref().ok_or_else(|| {
                    GraphError::ExecutionError(
                        "Checkpointer is required to resume from checkpoint".to_string(),
                    )
                })?;

                let snapshot = if let Some(checkpoint_id) = &checkpoint_config.checkpoint_id {
                    checkpointer
                        .get(thread_id, Some(checkpoint_id))
                        .await
                        .map_err(|e| {
                            GraphError::ExecutionError(format!(
                                "Failed to load checkpoint: {}",
                                e
                            ))
                        })?
                        .ok_or_else(|| {
                            GraphError::ExecutionError(format!(
                                "Checkpoint not found: {}",
                                checkpoint_id
                            ))
                        })?
                } else {
                    // Resume from latest checkpoint (e.g. after process crash)
                    checkpointer
                        .get(thread_id, None)
                        .await
                        .map_err(|e| {
                            GraphError::ExecutionError(format!(
                                "Failed to load latest checkpoint: {}",
                                e
                            ))
                        })?
                        .ok_or_else(|| {
                            GraphError::ExecutionError(format!(
                                "No checkpoint found for thread_id: {}",
                                thread_id
                            ))
                        })?
                };

                (snapshot.values.clone(), Some(snapshot.config.clone()))
            }
        };

        // Use super-step executor for parallel execution
        let scheduler = NodeScheduler::new(self.adjacency.clone());
        let executor = SuperStepExecutor::new(
            self.nodes.clone(),
            scheduler,
            self.checkpointer.clone(),
            durability_mode,
        );

        // Create new checkpoint config without checkpoint_id for new fork
        let mut new_checkpoint_config = checkpoint_config.clone();
        new_checkpoint_config.checkpoint_id = None;

        // Pass config and store to executor for nodes to access
        executor
            .execute(
                current_state,
                &new_checkpoint_config,
                parent_config.as_ref(),
                Some(config),       // Pass config to nodes
                self.store.clone(), // Pass store to nodes
            )
            .await
    }

    /// Stream the graph execution with config and stream mode
    ///
    /// This method supports checkpointing, resuming from checkpoints, and
    /// configurable stream modes.
    ///
    /// # Arguments
    ///
    /// * `initial_state` - The initial state to start execution with
    /// * `config` - The runnable configuration (thread_id, checkpoint_id, etc.)
    /// * `mode` - The stream mode (values, updates, messages, custom, or debug)
    ///
    /// # Returns
    ///
    /// A stream of `StreamChunk` values filtered by the stream mode
    pub fn astream_with_config_and_mode<'a>(
        &'a self,
        initial_state: S,
        config: &RunnableConfig,
        mode: StreamMode,
    ) -> Pin<Box<dyn Stream<Item = StreamChunk<S>> + Send + 'a>> {
        let checkpoint_config = match CheckpointConfig::from_config(config) {
            Ok(cfg) => cfg,
            Err(e) => {
                return Box::pin(stream! {
                    yield StreamChunk::Debug {
                        info: super::streaming::metadata::DebugInfo::new("Error")
                            .with_info("error".to_string(), serde_json::json!(e.to_string())),
                    };
                });
            }
        };
        let _thread_id = &checkpoint_config.thread_id;

        // If checkpoint_id is provided, load state from checkpoint
        let current_state = if let Some(_checkpoint_id) = &checkpoint_config.checkpoint_id {
            if let Some(_checkpointer) = &self.checkpointer {
                // For now, we'll use the initial state
                // In a full implementation, we'd load from checkpoint
                initial_state
            } else {
                initial_state
            }
        } else {
            initial_state
        };

        // Pass stream modes to enable LLM streaming if messages mode is requested
        let stream_modes = if mode == StreamMode::Messages {
            Some(vec![StreamMode::Messages])
        } else {
            None
        };
        let event_stream = self.stream_internal(current_state, stream_modes, false);

        Box::pin(stream! {
            use futures::StreamExt;
            let mut event_stream = event_stream;

            while let Some(event) = event_stream.next().await {
                if let Some(chunk) = Self::convert_event_to_chunk(&event, mode) {
                    yield chunk;
                }
            }
        })
    }

    /// Get the current state for a thread
    ///
    /// Returns the latest checkpoint for the given thread_id.
    pub async fn get_state(&self, config: &RunnableConfig) -> Result<StateSnapshot<S>, GraphError> {
        let checkpoint_config = CheckpointConfig::from_config(config)?;
        let thread_id = &checkpoint_config.thread_id;

        let checkpointer = self
            .checkpointer
            .as_ref()
            .ok_or_else(|| GraphError::ExecutionError("Checkpointer not configured".to_string()))?;

        let snapshot = checkpointer
            .get(thread_id, checkpoint_config.checkpoint_id.as_deref())
            .await
            .map_err(|e| GraphError::ExecutionError(format!("Failed to get state: {}", e)))?;

        snapshot.ok_or_else(|| {
            GraphError::ExecutionError(format!("No state found for thread: {}", thread_id))
        })
    }

    /// Get the state history for a thread
    ///
    /// Returns all checkpoints for the given thread_id in chronological order.
    pub async fn get_state_history(
        &self,
        config: &RunnableConfig,
    ) -> Result<Vec<StateSnapshot<S>>, GraphError> {
        let checkpoint_config = CheckpointConfig::from_config(config)?;
        let thread_id = &checkpoint_config.thread_id;

        let checkpointer = self
            .checkpointer
            .as_ref()
            .ok_or_else(|| GraphError::ExecutionError("Checkpointer not configured".to_string()))?;

        checkpointer
            .list(thread_id, None)
            .await
            .map_err(|e| GraphError::ExecutionError(format!("Failed to get state history: {}", e)))
    }

    /// Update the state for a thread
    ///
    /// This creates a new checkpoint with updated state values.
    /// The new checkpoint will be associated with the same thread, but will have
    /// a new checkpoint_id and will record the original checkpoint as its parent
    /// (for fork tracking in time-travel scenarios).
    ///
    /// # Arguments
    ///
    /// * `config` - The runnable configuration pointing to the checkpoint to update
    /// * `values` - State updates to apply
    /// * `as_node` - Optional node name to record in metadata
    ///
    /// # Returns
    ///
    /// A new `StateSnapshot` with the updated state and new checkpoint_id
    pub async fn update_state(
        &self,
        config: &RunnableConfig,
        values: &StateUpdate,
        as_node: Option<&str>,
    ) -> Result<StateSnapshot<S>, GraphError> {
        // Get current state
        let original_snapshot = self.get_state(config).await?;

        // Apply updates to state
        let updated_values = self.merge_state_update(&original_snapshot.values, values)?;

        // Create new checkpoint config (new checkpoint_id will be generated)
        let mut new_config = CheckpointConfig::new(original_snapshot.thread_id());
        new_config.checkpoint_ns = original_snapshot.config.checkpoint_ns.clone();

        // Create new snapshot with parent config for fork tracking
        let mut new_snapshot = StateSnapshot::with_parent(
            updated_values,
            original_snapshot.next.clone(),
            new_config,
            original_snapshot.config.clone(), // Parent config
        );

        // Update metadata
        if let Some(node) = as_node {
            new_snapshot
                .metadata
                .insert("as_node".to_string(), serde_json::json!(node));
        }

        // Save updated checkpoint
        let checkpointer = self
            .checkpointer
            .as_ref()
            .ok_or_else(|| GraphError::ExecutionError("Checkpointer not configured".to_string()))?;

        let checkpoint_id = checkpointer
            .put(new_snapshot.thread_id(), &new_snapshot)
            .await
            .map_err(|e| GraphError::ExecutionError(format!("Failed to update state: {}", e)))?;

        // Update snapshot with new checkpoint_id
        new_snapshot.config.checkpoint_id = Some(checkpoint_id);

        Ok(new_snapshot)
    }
}

/// Stream options for controlling streaming behavior
#[derive(Clone, Debug, Default)]
pub struct StreamOptions {
    /// Stream modes to enable
    pub stream_modes: Option<Vec<StreamMode>>,
    /// Whether to include subgraph events in the stream
    pub subgraphs: bool,
}

impl StreamOptions {
    /// Create default stream options
    pub fn new() -> Self {
        Self::default()
    }

    /// Create stream options with subgraphs enabled
    pub fn with_subgraphs(mut self, subgraphs: bool) -> Self {
        self.subgraphs = subgraphs;
        self
    }

    /// Create stream options with stream modes
    pub fn with_modes(mut self, stream_modes: Vec<StreamMode>) -> Self {
        self.stream_modes = Some(stream_modes);
        self
    }
}

/// Stream event type - represents different types of events during graph execution
#[derive(Clone, Debug)]
pub enum StreamEvent<S: State> {
    /// A node is about to be executed
    NodeStart {
        node: String,
        state: S,
        /// Path for subgraph nodes (e.g., ["parent_node:uuid", "subgraph_node"])
        path: Vec<String>,
    },
    /// A node has completed execution
    NodeEnd {
        node: String,
        state: S,
        update: StateUpdate,
        /// Path for subgraph nodes
        path: Vec<String>,
    },
    /// The graph has completed execution
    GraphEnd { final_state: S },
    /// An error occurred during execution
    Error { error: std::sync::Arc<GraphError> },
    /// A message chunk from an LLM node (for messages stream mode)
    MessageChunk {
        node: String,
        chunk: crate::schemas::StreamData,
        metadata: MessageMetadata,
        /// Path for subgraph nodes
        path: Vec<String>,
    },
    /// Custom data from a node (for custom stream mode)
    CustomData {
        node: String,
        data: serde_json::Value,
        /// Path for subgraph nodes
        path: Vec<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{function_node, state::MessagesState, StateGraph, END, START};
    use futures::StreamExt;

    #[tokio::test]
    async fn test_invoke_simple_graph() {
        let mut graph = StateGraph::<MessagesState>::new();

        graph
            .add_node(
                "node1",
                function_node("node1", |_state| async move {
                    let mut update = HashMap::new();
                    update.insert(
                        "messages".to_string(),
                        serde_json::to_value(vec![
                            crate::schemas::messages::Message::new_ai_message("Hello"),
                        ])?,
                    );
                    Ok(update)
                }),
            )
            .unwrap();

        graph.add_edge(START, "node1");
        graph.add_edge("node1", END);

        let compiled = graph.compile().unwrap();
        let initial_state = MessagesState::new();
        let result = compiled.invoke(initial_state).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_stream_simple_graph() {
        let mut graph = StateGraph::<MessagesState>::new();

        graph
            .add_node(
                "node1",
                function_node("node1", |_state| async move {
                    let mut update = HashMap::new();
                    update.insert(
                        "messages".to_string(),
                        serde_json::to_value(vec![
                            crate::schemas::messages::Message::new_ai_message("Hello"),
                        ])?,
                    );
                    Ok(update)
                }),
            )
            .unwrap();

        graph.add_edge(START, "node1");
        graph.add_edge("node1", END);

        let compiled = graph.compile().unwrap();
        let initial_state = MessagesState::new();
        let mut stream = compiled.stream(initial_state);

        let mut events = Vec::new();
        while let Some(event) = stream.next().await {
            events.push(event);
        }

        // Should have at least NodeStart, NodeEnd, and GraphEnd events
        assert!(events.len() >= 3);
    }
}
