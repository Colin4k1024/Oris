# Oris experience control plane

Oris stores governed procedural memory. It does not train or modify a base model. A reusable procedure is a `Gene`; one real application is an immutable `Capsule`; adoption or rejection by an Agent is a `UsageReceipt`.

The authoritative exchange contract is [`ExperienceBundleV1`](../spec/experience/experience-bundle-v1.schema.json). Rust, Python, TypeScript, and Go models round-trip the same [golden fixture](../spec/experience/golden/experience-bundle-v1.json).

## Lifecycle

- A proposal is always local `candidate`, even if its payload asks for an elevated state or shared scope.
- A candidate becomes local `stable` after at least three evidence-backed successes across at least two task-context hashes with no failures.
- Two consecutive ordinary failures demote a stable Gene to candidate.
- A safety failure immediately quarantines the Gene.
- Team/network promotion and revocation require an API key with `experience:govern`.

Success receipts with no test evidence are rejected. Raw conversations, credentials, user preferences, and unverified summaries are not valid Genes.

## REST v1

- `GET /v1/experience-assets` — structural filtering plus hybrid retrieval.
- `POST /v1/experience-assets` — propose an `ExperienceBundleV1`.
- `GET /v1/experience-assets/{id}` — get a complete versioned bundle.
- `GET /v1/experience-assets/{id}/skill` — render one portable AgentSkills package; candidate output is review-only and stable output is installable.
- `POST /v1/experience-assets/{id}/use` — begin a traceable use.
- `POST /v1/experience-assets/{id}/outcomes` — record receipt and optional Capsule.
- `POST /v1/experience-assets/{id}/promote` — governance-only publication.
- `POST /v1/experience-assets/{id}/revoke` — governance-only revoke/quarantine.

`/experience` remains for one compatibility cycle. It accepts a historic direct genestore Gene or exactly one OEN Gene asset. Multi-asset, Capsule, and event payloads are rejected rather than silently dropping fields.

## MCP

Run STDIO with:

```sh
cargo run -p oris-experience-repo --bin oris-experience-mcp
```

The HTTP server exposes the same JSON-RPC handler at `POST /mcp`. Implemented methods include `initialize`, `ping`, `tools/list`, `tools/call`, `resources/list`, `resources/templates/list`, `resources/read`, `prompts/list`, and `prompts/get`. Tool discovery is scope-trimmed: read callers see search/get/Skill projection; write callers see propose/begin-use/record-outcome and may manage only their own public key; governance callers additionally see promote/revoke; explicitly authorized administrators see API-key lifecycle and public-key registry operations.

Advertised tool names use the portable underscore form; the complete ordered list is published in [`plugins/oris-experience/capabilities.json`](../plugins/oris-experience/capabilities.json) and verified against the server constants by tests. Direct calls using the original dotted names remain accepted for one compatibility cycle. The portable form is required because strict Agent clients reject dots in MCP tool names. `experience:admin` is separate from `experience:govern` and is never granted by a packaged host default.

Resources use `oris://genes/{id}` and `oris://capsules/{id}`. Static resources `oris://capabilities` and `oris://instructions` let a host inspect its granted surface and the safe lifecycle at runtime. User-controlled prompts cover reuse, contribution, and governance; the governance prompt is hidden without `experience:govern`.

The historical `/experience` endpoint and `/health` probe remain HTTP-only compatibility and operational surfaces. Every current governed repository operation is available through MCP; a transport health probe is deliberately not a model-callable tool.

## Agent packages

[`plugins/oris-experience`](../plugins/oris-experience) is the shared package:

- Codex loads its `.codex-plugin` manifest, Skill, and MCP server.
- Claude Code loads the same Skill and `.mcp.json`; hooks persist begun/recorded use state and mark an unclosed task inconclusive without fabricating success.
- OpenCode uses the native MCP configuration and tool/session hook in `adapters/opencode`.
- Grok Build can load the Claude-compatible plugin directly or use the explicit native config in `adapters/grok`.
- OpenClaw loads the compatible bundle Skill. Projected skills must move through Skill Workshop: Oris candidate → pending, quarantined → quarantined, stable → eligible for operator-approved apply.

The authoritative cross-host operation list is [`plugins/oris-experience/capabilities.json`](../plugins/oris-experience/capabilities.json). Tests compare it to the server's Rust tool constants so an adapter cannot silently omit an Oris operation.
