# Oris ExperienceBundleV1 reference

The authoritative wire schema is `spec/experience/experience-bundle-v1.schema.json` in the Oris repository.

- `Gene`: reusable procedure, applicability, negative boundaries, steps, tool and permission requirements, safety constraints, validation contract, provenance, scope, and lifecycle.
- `Capsule`: immutable evidence for one real application, including environment and task hashes, validation results, artifact references, and redaction state.
- `UsageReceipt`: adoption or rejection by an Agent, applied steps, outcome, failure reason, test evidence, token/cost/latency metrics, and task context.

Portable MCP wire names are `oris_experience_search`, `oris_experience_get`, `oris_experience_propose`, `oris_experience_begin_use`, `oris_experience_record_outcome`, `oris_experience_promote`, and `oris_experience_revoke`. Ordinary Agent identities receive only the first five. Governance identities receive the final two. The server still accepts the historic dotted names on direct `tools/call` requests for one compatibility cycle, but does not advertise them because Grok and other strict clients reject dots in tool names.

OpenClaw projection mapping:

- Oris `candidate` → Skill Workshop `pending`
- Oris `quarantined` → Skill Workshop `quarantined`
- Oris `stable` → eligible for an `applied` workspace Skill after operator approval
- Oris `revoked` → remove or disable the local projection and retain the evidence record

OpenClaw must use `skill_workshop` for generated skill changes. Oris remains authoritative for evidence, versions, revocations, and cross-Agent usage statistics.
