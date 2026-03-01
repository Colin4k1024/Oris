# Plugin Authoring and Compatibility (0.1.x)

This document defines the contract for third-party graph node plugins in Oris 0.1.x: how to author, package, and what to expect across runtime upgrades.

## Overview

The runtime plugin system is **in-process only**. Plugins are Rust libraries that implement the `NodePlugin<S>` trait and are registered with `NodePluginRegistry<S>` at application startup. There is no out-of-process or sandboxed execution; plugin code runs in the same process as the runtime.

## Authoring Contract (0.1.x)

### Implement the plugin trait

- Depend on `oris-runtime` with a version compatible with the application (see [Compatibility](#api-compatibility-across-runtime-upgrades)).
- Implement [`oris_runtime::graph::NodePlugin<S>`](https://docs.rs/oris-runtime/latest/oris_runtime/graph/trait.NodePlugin.html) for the state type `S` your nodes use (e.g. `MessagesState`).
- Provide a **stable plugin type** string via `plugin_type()`. Use a unique, namespaced identifier (e.g. `my_org/my_plugin_name`) to avoid clashes.
- In `create_node(name, config)`, validate `config` and return a node implementing `Node<S>`. Prefer typed config via `typed_node_plugin` and `serde` for validation.

### Package layout (reference)

A packaged plugin is a Rust crate that:

1. Declares `oris-runtime` as a dependency (same major.minor as the host app for 0.1.x).
2. Exposes one or more types or constructor functions that produce `Arc<dyn NodePlugin<S>>` (or a type implementing `NodePlugin<S>`).
3. Documents the plugin type string and the JSON schema (or shape) of the config payload.

Example layout:

```
my-oris-plugin/
  Cargo.toml     # [dependencies] oris-runtime = "0.1"
  src/lib.rs     # pub fn my_plugin() -> impl NodePlugin<MessagesState> + ...
  README.md      # Plugin type, config schema, compatibility
```

See the [plugin reference example](../examples/plugin_reference/README.md) in this repository for a concrete crate that follows this contract.

### Registry metadata / capability descriptors (0.1.x)

The runtime does not require a separate registry manifest or capability file. The host application discovers plugins by linking the crate and calling `registry.register_plugin(...)`. Optional metadata you may document for your plugin:

- **Plugin type**: the string returned by `plugin_type()`.
- **Config schema**: JSON shape or schema for the `config` passed to `create_node`.
- **State type**: e.g. `MessagesState`; must match the registry’s state type.
- **Minimum oris-runtime**: e.g. `0.1.3`, for compatibility claims.

If you ship a registry or catalog later, these fields can be used as capability descriptors.

## API Compatibility Across Runtime Upgrades

- **0.1.x**: Patch and minor bumps are intended to be backward compatible for the plugin API. Existing `NodePlugin` and `NodePluginRegistry` usage should continue to work; new methods may be added.
- **Breaking changes** to the plugin trait or registry will be accompanied by a major version bump (e.g. 0.2.0). Plan to pin the host app and plugins to the same major.minor when you need stability.
- We do not guarantee stability across different minor versions (e.g. 0.1 vs 0.2) without a migration path documented in release notes.

## Safety Boundaries (In-Process, Unsandboxed)

- Plugins run **in the same process** as the Oris runtime and the application. There is no sandboxing or isolation.
- Plugin code can access the same memory, filesystem, and network as the host. Ensure you only register plugins from trusted sources or from dependencies you control.
- No separate capability or permission model is enforced; the host is responsible for which plugins it registers.
- For multi-tenant or untrusted plugin scenarios, a future version may introduce a different extension mechanism (e.g. sandboxed or out-of-process). The current 0.1.x contract does not cover that.

## Summary

| Topic | 0.1.x contract |
|-------|----------------|
| Packaging | Rust crate; link and register at startup. |
| Discovery | By registration only; no central registry required. |
| Compatibility | Same major.minor or follow release notes. |
| Safety | In-process, unsandboxed; trust the plugin source. |
