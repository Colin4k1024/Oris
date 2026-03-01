use std::sync::Arc;

use oris_runtime::graph::{
    function_node, messages_state_update, typed_node_plugin, MessagesState, NodePluginRegistry,
    StateGraph, END, START,
};
use oris_runtime::schemas::messages::Message;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct PrefixConfig {
    prefix: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut registry = NodePluginRegistry::<MessagesState>::new();
    registry.register_plugin(typed_node_plugin(
        "prefix_message",
        |name, config: PrefixConfig| {
            let prefix = config.prefix;
            Ok(Arc::new(function_node(
                name.to_string(),
                move |_state: &MessagesState| {
                    let content = format!("{} from runtime plugin", prefix);
                    async move {
                        Ok(messages_state_update(vec![Message::new_ai_message(
                            &content,
                        )]))
                    }
                },
            )))
        },
    ))?;

    let mut graph = StateGraph::<MessagesState>::new();
    graph.add_plugin_node(
        "plugin-step",
        "prefix_message",
        serde_json::json!({ "prefix": "hello" }),
        &registry,
    )?;
    graph.add_edge(START, "plugin-step");
    graph.add_edge("plugin-step", END);

    let result = graph.compile()?.invoke(MessagesState::new()).await?;
    println!("{}", result.messages[0].content);
    Ok(())
}
