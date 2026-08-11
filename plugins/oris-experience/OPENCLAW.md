# OpenClaw integration

OpenClaw can install this directory as a compatible Codex/Claude bundle and load the shared `skills/oris-experience` Skill. Configure the Oris MCP server separately if the compatible-bundle runtime does not activate `.mcp.json`.

Project candidates must be projected through Skill Workshop, not written directly into workspace skills:

1. Convert an Oris candidate into a `skill_workshop create` proposal with evidence references.
2. Keep it pending while Oris lifecycle is `candidate`.
3. Map safety quarantine to `skill_workshop quarantine`.
4. Apply only after Oris reports `stable` and an operator approves the workspace projection.
5. If Oris revokes the Gene, disable or remove the projection while retaining the Oris evidence record.

Recommended OpenClaw setting: `skills.workshop.approvalPolicy: "pending"`.
