# Plugin Reference (0.1.x)

Reference layout for an **external** Oris graph node plugin. Use this crate as a template for packaging your own plugins.

## Contract

- **Plugin type**: `plugin_reference/delay`
- **State type**: `MessagesState`
- **Config schema**: `{ "message": string, "delay_ms"?: number }` (default `delay_ms`: 100)
- **Minimum oris-runtime**: `0.1.x` (same major.minor as host app)

## Usage (host application)

Add this crate as a path dependency (or publish and depend on the published crate), then:

```rust
use oris_runtime::graph::{MessagesState, NodePluginRegistry, StateGraph, END, START};
use plugin_reference::register_all;

let mut registry = NodePluginRegistry::<MessagesState>::new();
register_all(&mut registry)?;

let mut graph = StateGraph::<MessagesState>::new();
graph.add_plugin_node(
    "delayed-step",
    plugin_reference::DELAY_NODE_PLUGIN_TYPE,
    serde_json::json!({ "message": "Hello from plugin_reference", "delay_ms": 50 }),
    &registry,
)?;
```

## Layout

- `src/lib.rs`: Plugin implementation and `register_all` helper.
- `Cargo.toml`: Depends on `oris-runtime` (path or version) with no required features for the graph plugin API.
- This README: Plugin type, config schema, compatibility.

See [Plugin Authoring](../../docs/plugin-authoring.md) for the full 0.1.x contract and compatibility rules.
