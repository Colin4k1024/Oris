# MCP Bootstrap (Slice 1)

This document defines the first MCP integration slice shipped for Oris runtime.

## Scope in this slice

- Compile-time gate: `mcp-experimental` feature.
- Runtime gate: `ORIS_MCP_BOOTSTRAP_ENABLED`.
- Bootstrap status endpoint: `GET /v1/mcp/bootstrap`.
- Capability discovery endpoint: `GET /v1/mcp/capabilities` (enabled only when bootstrap gate is on).
- Capability registry includes at least one Oris-backed tool mapping.

This slice intentionally does **not** claim full MCP protocol support.

## Bootstrap configuration

`ExecutionApiState` reads MCP bootstrap config through:

- `McpBootstrapConfig::from_env()`
- `ExecutionApiState::with_mcp_bootstrap(...)`
- `ExecutionApiState::with_mcp_bootstrap_from_env()`

Supported environment variables:

- `ORIS_MCP_BOOTSTRAP_ENABLED` (`true|false`, `1|0`, `yes|no`, `on|off`)
- `ORIS_MCP_TRANSPORT` (`http` or `stdio`)
- `ORIS_MCP_SERVER_NAME`
- `ORIS_MCP_SERVER_VERSION`

## Capability mapping contract (current)

Current mapping model links MCP tool metadata to existing Oris HTTP routes:

- `tool_name`
- `description`
- `oris_http_method`
- `oris_route`
- `contract_version`
- `input_schema`

The default registry includes `oris.runtime.jobs.run` mapped to `POST /v1/jobs/run`.

## Next slices (not in this release)

1. MCP JSON-RPC transport/session lifecycle (beyond bootstrap metadata).
2. MCP tool invocation bridge (dispatch MCP tool calls to Oris runtime endpoints).
3. Auth/session identity bridging between MCP principal and Oris API auth model.
4. Registry generation from runtime API contract artifacts for stricter schema parity.
5. End-to-end conformance tests against selected MCP client/server interoperability suites.
