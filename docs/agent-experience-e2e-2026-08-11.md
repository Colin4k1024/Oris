# Claude Code ↔ Grok real experience evolution E2E — 2026-08-11

## Verdict

The governed functional loop passed with real Claude Code and Grok CLI processes:

`Claude fix → Gene/Capsule/Receipt → Grok search/use/validate/receipt → Claude independent reuse → stable Gene`

The final persisted state was 3 verified successes, 0 failures, and 3 distinct task contexts. This proves cross-Agent experience capture, retrieval, adoption, evidence recording, and lifecycle promotion work end to end. It does not yet prove a performance or task-success improvement over an Agent without Oris.

## Environment

- Claude Code 2.1.205, authenticated through OAuth.
- Grok CLI 1.0.0 (56999f7), authenticated locally.
- Oris MCP over STDIO, backed by an isolated SQLite database at `/tmp/oris-agent-e2e/experience.db`.
- Grok native evolution and memory were disabled so the experiment measured Oris rather than another memory system.
- Every task ran in an isolated project under `/tmp/oris-agent-e2e`.

## Scenario and evidence

The reusable procedure was intentionally expressed across different implementations:

1. Claude repaired a Python line-delimited JSON-RPC server whose readiness `print` polluted stdout. It ran `python3 -m unittest -v`, preserved the diagnostic on stderr, and proposed `software.protocol.stdio-json.keep-diagnostics-off-protocol-stream` with a Capsule and succeeded UsageReceipt.
2. Grok repaired a structurally different stdio bridge. It retrieved the Claude Gene with score 0.6094, inspected all applicability and negative boundaries, called `begin_use`, applied all five relevant steps, reran the test successfully, and recorded `receipt-grok-reuse-003`.
3. Claude repaired a third JSONL worker where `logging.basicConfig(stream=sys.stdout)` was the source of contamination. It retrieved the same Gene with score 0.671, called `begin_use`, changed the logging sink to stderr, ran both validation checks, and recorded `receipt-claude-reuse-001`.

Persisted receipts and contexts:

| Agent | Run | Context | Result |
|---|---|---|---|
| Claude Code | `claude-seed-stdio-json-run` | `sha256:claude-seed-stdio-json` | succeeded |
| Grok CLI | `grok-reuse-003` | `sha256:grok-reuse-stdio-json` | succeeded |
| Claude Code | `claude-reuse-001` | `sha256:claude-reuse-jsonl-worker` | succeeded |

Final state:

| Lifecycle | Successes | Failures | Distinct contexts |
|---|---:|---:|---:|
| stable | 3 | 0 | 3 |

## Real interoperability failures found and fixed

The first attempts did not pass. They exposed three issues that protocol-only unit tests had missed:

1. Claude Code rejected a search result because MCP `structuredContent` was an array. Oris now always returns the object envelope `{ "data": ... }` while retaining text content.
2. Grok rejected every advertised dotted MCP tool name such as `oris.experience.search`. Oris now advertises portable underscore names such as `oris_experience_search`; direct dotted `tools/call` names remain accepted for one compatibility cycle.
3. Grok initially received zero matches because the Gene required abstract capabilities (`test-runner`, `code-search`, `editor`) while Grok reported concrete tools (`run_command`, `read_file`, `apply_patch`). Search now normalizes common Codex, Claude Code, and Grok tool names to portable capabilities before the hard filter.

Grok also first sent the unknown receipt field `actual_applied_step_ids`. Oris rejected it instead of silently dropping it; Grok corrected the field to `applied_step_ids` and the second submission succeeded.

## Baseline and efficiency result

The no-Oris Grok baseline also solved the deliberately small bug:

| Run | Passed | Agent turns | Reported total tokens |
|---|---|---:|---:|
| Grok, Oris disabled | yes | 8 | 432,025 |
| Grok, successful Oris reuse | yes | 10 | 580,686 |

The token totals include very large local skill-cache reads, so they are noisy, but this run provides no evidence of an efficiency gain. Oris added MCP and evidence overhead on an easy task that the baseline already solved. The correct conclusion is therefore:

- functional cross-Agent self-evolution: passed;
- improved success rate, latency, or token efficiency: not demonstrated by this one easy scenario.

Claude reported USD cost directly. The seed task cost $1.0614 and the third reuse task cost $0.4839. Including failed compatibility probes and canaries, total Claude cost for this debugging experiment was approximately $2.3097. Grok did not report a monetary cost.

## Follow-up benchmark required for “make agent better”

Run a pre-registered batch of harder tasks where the reusable root cause is not obvious from one assertion. Use the same model, permissions, task variants, and maximum turns with Oris off and on. Compare validation pass rate first, then time-to-validation, non-cache tokens, cost, false executable suggestions, and harmful reuse. Keep simple known-pattern tasks as an overhead control rather than the primary improvement set.
