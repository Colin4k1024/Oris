# Oris Experience agent integrations

This package exposes one governed Experience Repository contract to Claude Code,
Codex, OpenCode, and Grok Build. The canonical machine-readable
surface is [`capabilities.json`](capabilities.json); host adapters must not
rename, omit, or reinterpret those operations.

## Permission profiles

`ORIS_MCP_SCOPES` controls discovery as well as invocation:

- `experience:read` exposes search, get, Skill projection, resources, and the reuse prompt.
- `experience:write` exposes propose, begin-use, record-outcome, and the contribution prompt.
- `experience:govern` additionally exposes promote, revoke, and the governance prompt.
- `experience:write` also lets an agent register, rotate, or revoke only its own
  Ed25519 verification key.
- `experience:admin` exposes API-key lifecycle administration and the active
  public-key registry. Newly created or rotated secrets are returned once.

The packaged default is `experience:read,experience:write`. Governance remains
available but is not granted implicitly. An authorized operator can opt in with:

```sh
ORIS_MCP_SCOPES=experience:read,experience:write,experience:govern
```

Repository administrators can add `experience:admin`; this is intentionally
separate from governance and is never present in a packaged host default.

Every host keeps its native sandbox and approval policy. Oris never turns a
retrieved Gene into authority to bypass either one.

## Host entry points

| Host | Native entry point | Lifecycle coverage |
|---|---|---|
| Claude Code | `.claude-plugin/plugin.json`, `.mcp.json`, `hooks/hooks.json` | Hook records begun and completed uses; unfinished uses become pending/inconclusive |
| Codex | `.codex-plugin/plugin.json` or `adapters/codex/config.toml` | Shared Skill requires explicit evidence-backed outcome recording |
| OpenCode | `adapters/opencode/opencode.json` plus `oris-experience.js` | Native tool/session hooks track unfinished uses |
| Grok Build | Claude-compatible plugin or `adapters/grok/config.toml` | Loads the same Skill/MCP/hooks; native config is provided for explicit installs |

The configuration templates assume they are used from an Oris checkout. For an
installed binary, replace the wrapper path with `oris-experience-mcp` or set
`ORIS_EXPERIENCE_MCP_BIN`.

## Complete lifecycle

1. Search with actual tool, environment, project, and tenant constraints.
2. Read the complete Gene or Skill projection and reject an incompatible result.
3. Call `oris_experience_begin_use` before applying any step.
4. Preserve host permissions and validate in the caller environment.
5. Call `oris_experience_record_outcome` with the real result and evidence.
6. Propose only reusable, redacted procedures after a terminal result.
7. Restrict promotion, revocation, and quarantine to governance principals.
8. Restrict API-key issuance, rotation, revocation, and registry listing to
   explicit administrators; agents may mutate only their own public key.

The MCP resources `oris://capabilities` and `oris://instructions` let any
standards-compliant host inspect this contract at runtime.

The historical `/experience` compatibility endpoint and `/health` probe remain
HTTP-only compatibility/operations surfaces. Every current governed repository
operation is available through MCP; transport health checks are not agent tools.
