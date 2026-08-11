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

The HTTP server exposes the same JSON-RPC handler at `POST /mcp`. Implemented methods include `initialize`, `ping`, `tools/list`, `tools/call`, `resources/list`, `resources/templates/list`, and `resources/read`. Ordinary callers see search/get/propose/begin-use/record-outcome; governance callers additionally see promote/revoke.

Advertised tool names use the portable underscore form (`oris_experience_search`, `oris_experience_get`, `oris_experience_propose`, `oris_experience_begin_use`, `oris_experience_record_outcome`, `oris_experience_promote`, and `oris_experience_revoke`). Direct calls using the original dotted names remain accepted for one compatibility cycle. The portable form is required because strict Agent clients reject dots in MCP tool names.

Resources use `oris://genes/{id}` and `oris://capsules/{id}`.

## Agent packages

[`plugins/oris-experience`](../plugins/oris-experience) is the shared package:

- Codex loads its `.codex-plugin` manifest, Skill, and MCP server.
- Claude Code loads the same Skill and `.mcp.json`; hooks persist begun/recorded use state and mark an unclosed task inconclusive without fabricating success.
- OpenClaw loads the compatible bundle Skill. Projected skills must move through Skill Workshop: Oris candidate → pending, quarantined → quarantined, stable → eligible for operator-approved apply.
