# Use Oris with a coding agent

Oris adds evidence-backed procedural memory to an existing coding agent. It does not replace Claude Code, Codex, OpenCode, Grok, or an in-house agent, and it does not modify model weights.

When connected, an agent can:

1. search for a verified procedure before a recurring engineering task;
2. inspect applicability and negative boundaries before adopting it;
3. run the repository's own validation after applying it;
4. record the real outcome so the procedure improves, degrades, or is quarantined;
5. propose a new local candidate only after a task has terminal evidence.

## Current source-checkout setup

The one-command `oris connect` experience is an accepted product direction, but is not shipped yet. The current version uses the MCP binary from this repository.

Build it once:

```bash
cd /path/to/Oris
cargo build -p oris-experience-repo --bin oris-experience-mcp
```

Choose one absolute database path and use it for every local agent:

```text
/absolute/path/to/.oris/experience_repo.db
```

Different database paths create isolated experience stores.

Ordinary agents should receive only:

```text
ORIS_MCP_SCOPES=experience:read,experience:write
```

Do not grant `experience:govern` to an ordinary coding agent.

## Claude Code

```bash
claude mcp add \
  --env ORIS_EXPERIENCE_DB=/absolute/path/to/.oris/experience_repo.db \
  --env ORIS_AGENT_ID=claude-code \
  --env ORIS_MCP_SCOPES=experience:read,experience:write \
  --transport stdio \
  --scope project \
  oris-experience \
  -- /absolute/path/to/Oris/target/debug/oris-experience-mcp
```

Verify with `claude mcp list` and `/mcp` inside Claude Code. Copy `plugins/oris-experience/skills/oris-experience` into the target project's `.claude/skills/` directory when the plugin package is not installed directly.

## OpenCode

For OpenCode V1, add this project configuration to `opencode.json`:

```json
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "oris-experience": {
      "type": "local",
      "command": ["/absolute/path/to/Oris/target/debug/oris-experience-mcp"],
      "enabled": true,
      "environment": {
        "ORIS_EXPERIENCE_DB": "/absolute/path/to/.oris/experience_repo.db",
        "ORIS_AGENT_ID": "opencode",
        "ORIS_MCP_SCOPES": "experience:read,experience:write"
      }
    }
  }
}
```

Use `opencode mcp list` to verify the connection. OpenCode V2 nests servers under `mcp.servers` and uses `disabled` instead of `enabled`.

## Grok CLI

```bash
grok mcp add \
  --scope project \
  oris-experience \
  -e ORIS_EXPERIENCE_DB=/absolute/path/to/.oris/experience_repo.db \
  -e ORIS_AGENT_ID=grok-cli \
  -e ORIS_MCP_SCOPES=experience:read,experience:write \
  -- /absolute/path/to/Oris/target/debug/oris-experience-mcp
```

Verify with:

```bash
grok mcp doctor oris-experience
```

For controlled Oris benchmarks, start Grok with `--no-evolution --no-memory` so another memory system does not confound the result. This is optional for normal use.

## Codex and compatible Agent Skill runtimes

The shared package is in `plugins/oris-experience`. It contains the portable Skill, MCP metadata, Claude hooks, and the MCP start script. When the runtime does not install the complete package, connect the same STDIO binary and load `skills/oris-experience/SKILL.md` through the runtime's normal Skill mechanism.

## In-house agents

Integrate these lifecycle points through MCP:

| Agent lifecycle | Oris tool |
|---|---|
| Before a recurring engineering task | `oris_experience_search` |
| After deciding to adopt a Gene | `oris_experience_begin_use` |
| After native validation reaches a terminal result | `oris_experience_record_outcome` |
| After a novel procedure has terminal evidence | `oris_experience_propose` |

The agent must not execute a result rejected by structural filters, and must not treat semantic similarity as permission to bypass its sandbox, approvals, or tests.

## Verify the control-plane loop locally

Run the deterministic MCP scenario:

```bash
python3 scripts/demo_experience_onboarding.py
```

It uses the real MCP binary and a disposable database under `target/experience-onboarding-demo`. The three client identities are deterministic protocol clients, not model invocations. For real Claude Code and Grok CLI evidence, see [the model-level E2E report](agent-experience-e2e-2026-08-11.md).
