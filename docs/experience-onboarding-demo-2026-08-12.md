# Oris upstream experience onboarding demo — 2026-08-12

## Verdict

The deterministic local onboarding scenario passed through the real Oris STDIO MCP server:

```text
Claude identity proposes verified candidate
  → Grok identity searches, begins use, validates, and records evidence
  → OpenCode identity independently reuses and records evidence
  → Oris promotes the Gene to local stable
  → Oris renders an installable portable Agent Skill
```

Final state:

| Lifecycle | Verified successes | Failures | Distinct contexts | Skill installable |
|---|---:|---:|---:|---|
| `stable` | 3 | 0 | 3 | yes |

## Command

```bash
python3 scripts/demo_experience_onboarding.py
```

The script starts the actual `target/debug/oris-experience-mcp` process for each Agent identity and shares one disposable SQLite database at `target/experience-onboarding-demo/experience.db`.

Generated local evidence:

- `target/experience-onboarding-demo/session.log`
- `target/experience-onboarding-demo/summary.json`
- `target/experience-onboarding-demo/experience.db`

## Assertions exercised

1. An ordinary Agent sees exactly the five non-governance MCP tools.
2. A proposal containing terminal validation evidence remains a local candidate.
3. Grok and OpenCode identities retrieve the same Gene with applicability boundaries intact.
4. Each adoption calls `begin_use` before recording the terminal outcome.
5. Every successful receipt contains test evidence and an evidence Capsule.
6. Three successes across three contexts with no failure promote the Gene to `stable`.
7. The stable Gene produces an installable Agent Skill projection.

## Animated evidence

The README animation is rendered from the successful `session.log`. The renderer refuses to produce a GIF unless the final log line is `RESULT: PASS`.

```bash
python3 scripts/render_experience_onboarding_gif.py
```

The renderer requires Pillow. Output: `docs/assets/oris-experience-onboarding.gif` (960×540, 15 frames, approximately 346 KiB).

## Regression verification

The final verification run completed with:

```text
demo scenario                         PASS
Python compile check                  PASS
oris-experience-repo unit tests       43 passed
Ed25519 integration tests             13 passed
Rust doc tests                         0 failed
```

The first unit-test attempt ran inside a restricted filesystem sandbox. Two `mockito` tests could not bind a loopback port and failed with `Operation not permitted`; the complete suite passed when rerun with local loopback access. This was an execution-environment restriction, not a product failure.

## Evidence boundary

This test validates the protocol, store, lifecycle, evidence, cross-identity sharing, and Skill projection in a deterministic and repeatable way. The three identities are protocol clients and do not invoke language models.

Real Claude Code and Grok CLI processes previously completed a model-level cross-Agent scenario, documented in [Claude Code ↔ Grok real experience evolution E2E](agent-experience-e2e-2026-08-11.md). That report proves real Agent interoperability but does not yet prove a general success-rate, latency, or Token improvement. A pre-registered hard-task benchmark remains required before making that claim.
