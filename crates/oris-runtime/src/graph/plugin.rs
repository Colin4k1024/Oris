use std::{collections::HashMap, marker::PhantomData, sync::Arc};

use serde::de::DeserializeOwned;
use serde_json::Value;

use super::{error::GraphError, node::Node, state::State};

/// Runtime plugin interface for constructing custom graph node types from configuration.
///
/// A plugin is responsible for validating its configuration and returning a concrete
/// node implementation for the requested graph state type.
pub trait NodePlugin<S: State>: Send + Sync {
    /// Stable plugin type identifier used for registration and lookup.
    fn plugin_type(&self) -> &str;

    /// Create a node instance for the provided graph node name and configuration payload.
    fn create_node(&self, name: &str, config: &Value) -> Result<Arc<dyn Node<S>>, GraphError>;
}

/// Registry for runtime-resolved node plugins.
///
/// This allows applications to register custom node factories up front, then
/// construct graph nodes later from a runtime payload (`plugin_type` + config).
pub struct NodePluginRegistry<S: State> {
    plugins: HashMap<String, Arc<dyn NodePlugin<S>>>,
}

impl<S: State> NodePluginRegistry<S> {
    /// Create an empty plugin registry.
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
        }
    }

    /// Register a plugin by value.
    ///
    /// Returns an error if the same `plugin_type` is already registered.
    pub fn register_plugin<P>(&mut self, plugin: P) -> Result<&mut Self, GraphError>
    where
        P: NodePlugin<S> + 'static,
    {
        self.register_plugin_arc(Arc::new(plugin))
    }

    /// Register a shared plugin implementation.
    ///
    /// Returns an error if the same `plugin_type` is already registered.
    pub fn register_plugin_arc(
        &mut self,
        plugin: Arc<dyn NodePlugin<S>>,
    ) -> Result<&mut Self, GraphError> {
        let plugin_type = plugin.plugin_type().to_string();
        if self.plugins.contains_key(&plugin_type) {
            return Err(GraphError::CompilationError(format!(
                "Plugin type '{}' is already registered",
                plugin_type
            )));
        }
        self.plugins.insert(plugin_type, plugin);
        Ok(self)
    }

    /// Return true when a plugin type is registered.
    pub fn contains(&self, plugin_type: &str) -> bool {
        self.plugins.contains_key(plugin_type)
    }

    /// Return registered plugin types in stable order.
    pub fn plugin_types(&self) -> Vec<String> {
        let mut plugin_types = self.plugins.keys().cloned().collect::<Vec<_>>();
        plugin_types.sort();
        plugin_types
    }

    /// Build a node from a plugin registration and runtime configuration.
    pub fn create_node(
        &self,
        name: &str,
        plugin_type: &str,
        config: &Value,
    ) -> Result<Arc<dyn Node<S>>, GraphError> {
        let plugin = self.plugins.get(plugin_type).ok_or_else(|| {
            GraphError::CompilationError(format!("Plugin type '{}' is not registered", plugin_type))
        })?;
        plugin.create_node(name, config)
    }
}

impl<S: State> Default for NodePluginRegistry<S> {
    fn default() -> Self {
        Self::new()
    }
}

/// Typed plugin adapter that deserializes config into a concrete Rust type before building the node.
///
/// This is the recommended helper for runtime type safety: config is validated via `serde`
/// before any node instance is created.
pub struct TypedNodePlugin<S: State, C, F>
where
    C: DeserializeOwned + Send + Sync + 'static,
    F: Fn(&str, C) -> Result<Arc<dyn Node<S>>, GraphError> + Send + Sync + 'static,
{
    plugin_type: String,
    factory: F,
    _state: PhantomData<S>,
    _config: PhantomData<C>,
}

impl<S: State, C, F> TypedNodePlugin<S, C, F>
where
    C: DeserializeOwned + Send + Sync + 'static,
    F: Fn(&str, C) -> Result<Arc<dyn Node<S>>, GraphError> + Send + Sync + 'static,
{
    /// Create a new typed plugin adapter.
    pub fn new(plugin_type: impl Into<String>, factory: F) -> Self {
        Self {
            plugin_type: plugin_type.into(),
            factory,
            _state: PhantomData,
            _config: PhantomData,
        }
    }
}

impl<S: State, C, F> NodePlugin<S> for TypedNodePlugin<S, C, F>
where
    C: DeserializeOwned + Send + Sync + 'static,
    F: Fn(&str, C) -> Result<Arc<dyn Node<S>>, GraphError> + Send + Sync + 'static,
{
    fn plugin_type(&self) -> &str {
        &self.plugin_type
    }

    fn create_node(&self, name: &str, config: &Value) -> Result<Arc<dyn Node<S>>, GraphError> {
        let typed_config: C = serde_json::from_value(config.clone()).map_err(|e| {
            GraphError::CompilationError(format!(
                "Invalid config for plugin '{}': {}",
                self.plugin_type, e
            ))
        })?;
        (self.factory)(name, typed_config)
    }
}

/// Convenience constructor for [TypedNodePlugin].
pub fn typed_node_plugin<S: State, C, F>(
    plugin_type: impl Into<String>,
    factory: F,
) -> TypedNodePlugin<S, C, F>
where
    C: DeserializeOwned + Send + Sync + 'static,
    F: Fn(&str, C) -> Result<Arc<dyn Node<S>>, GraphError> + Send + Sync + 'static,
{
    TypedNodePlugin::new(plugin_type, factory)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        graph::{
            function_node, messages_state_update, CompiledGraph, MessagesState, StateGraph, END,
            START,
        },
        schemas::messages::Message,
    };
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    struct EchoConfig {
        prefix: String,
    }

    fn build_registry() -> NodePluginRegistry<MessagesState> {
        let mut registry = NodePluginRegistry::new();
        registry
            .register_plugin(typed_node_plugin("echo", |name, config: EchoConfig| {
                let prefix = config.prefix;
                Ok(Arc::new(function_node(
                    name.to_string(),
                    move |_state: &MessagesState| {
                        let content = format!("{} from plugin", prefix);
                        async move {
                            Ok(messages_state_update(vec![Message::new_ai_message(
                                &content,
                            )]))
                        }
                    },
                )))
            }))
            .expect("register plugin");
        registry
    }

    async fn build_plugin_graph() -> CompiledGraph<MessagesState> {
        let registry = build_registry();
        let mut graph = StateGraph::<MessagesState>::new();
        graph
            .add_plugin_node(
                "echo-node",
                "echo",
                serde_json::json!({"prefix": "hello"}),
                &registry,
            )
            .expect("plugin node");
        graph.add_edge(START, "echo-node");
        graph.add_edge("echo-node", END);
        graph.compile().expect("compile")
    }

    #[test]
    fn registry_rejects_duplicate_plugin_types() {
        let mut registry = NodePluginRegistry::<MessagesState>::new();
        registry
            .register_plugin(typed_node_plugin("echo", |_name, _config: EchoConfig| {
                Ok(Arc::new(function_node(
                    "echo",
                    |_state: &MessagesState| async move { Ok(messages_state_update(Vec::new())) },
                )))
            }))
            .expect("first register");

        let err = match registry.register_plugin(typed_node_plugin(
            "echo",
            |_name, _config: EchoConfig| {
                Ok(Arc::new(function_node(
                    "echo-2",
                    |_state: &MessagesState| async move { Ok(messages_state_update(Vec::new())) },
                )))
            },
        )) {
            Ok(_) => panic!("duplicate register should fail"),
            Err(err) => err,
        };

        assert!(matches!(err, GraphError::CompilationError(_)));
    }

    #[test]
    fn registry_validates_typed_config_at_runtime() {
        let registry = build_registry();
        let err =
            match registry.create_node("broken", "echo", &serde_json::json!({"unknown": true})) {
                Ok(_) => panic!("invalid config should fail"),
                Err(err) => err,
            };

        assert!(matches!(err, GraphError::CompilationError(_)));
    }

    #[tokio::test]
    async fn graph_can_execute_runtime_registered_plugin_nodes() {
        let graph = build_plugin_graph().await;
        let result = graph.invoke(MessagesState::new()).await.expect("invoke");

        assert_eq!(result.messages.len(), 1);
        assert_eq!(result.messages[0].content, "hello from plugin");
    }
}
