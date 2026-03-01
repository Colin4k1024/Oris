//! Reference implementation of an external Oris graph node plugin (0.1.x).
//!
//! This crate demonstrates the packaged plugin layout: implement [NodePlugin],
//! expose a constructor, and document plugin type + config schema. See
//! [plugin-authoring](https://github.com/Colin4k1024/Oris/blob/main/docs/plugin-authoring.md).

use std::sync::Arc;

use oris_runtime::graph::{
    function_node, messages_state_update, typed_node_plugin, GraphError, MessagesState, NodePlugin,
    NodePluginRegistry,
};
use oris_runtime::schemas::messages::Message;
use serde::Deserialize;

/// Plugin type string for the delay node. Use this when adding the node via [NodePluginRegistry].
pub const DELAY_NODE_PLUGIN_TYPE: &str = "plugin_reference/delay";

/// Config for the delay node plugin.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct DelayNodeConfig {
    /// Message to append after the delay.
    pub message: String,
    /// Delay in milliseconds (simulated).
    #[serde(default = "default_delay_ms")]
    pub delay_ms: u64,
}

fn default_delay_ms() -> u64 {
    100
}

/// Builds a [NodePlugin] for the delay node. Register with:
/// `registry.register_plugin(plugin_reference::delay_node_plugin())?`
pub fn delay_node_plugin() -> impl NodePlugin<MessagesState> + 'static {
    typed_node_plugin(DELAY_NODE_PLUGIN_TYPE, |name, config: DelayNodeConfig| {
        let message = config.message;
        let delay_ms = config.delay_ms;
        Ok(Arc::new(function_node(
            name.to_string(),
            move |_state: &MessagesState| {
                let msg = message.clone();
                async move {
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
                    Ok(messages_state_update(vec![Message::new_ai_message(&msg)]))
                }
            },
        )))
    })
}

/// Registers all plugin_reference plugins into the given registry.
/// Use from the host app: `plugin_reference::register_all(&mut registry)?`
pub fn register_all(registry: &mut NodePluginRegistry<MessagesState>) -> Result<(), GraphError> {
    registry.register_plugin(delay_node_plugin())?;
    Ok(())
}
