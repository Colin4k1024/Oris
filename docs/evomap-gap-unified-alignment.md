# EvoMap Gap Unified Alignment (2026-03-09)

## Sync baseline

- Local branch: `main`
- Remote sync: `git pull` => already up to date
- Snapshot commit: `af2c536`
- Snapshot time: `2026-03-09 13:25:31 CST`
- GitHub source of truth: open `evomap-gap` issues in `Colin4k1024/Oris`

## Already aligned baseline (implemented)

These compatibility foundations are already present in code:

- `/a2a` compatibility router family and EvoMap aliases:
  - `crates/oris-runtime/src/execution_server/api_handlers.rs` (`with_a2a_routes`, lines ~2271+)
- Stable-vs-experimental route boundary:
  - `crates/oris-runtime/src/execution_server/api_handlers.rs` (`with_evolution_routes`, lines ~2335+)
  - `crates/oris-runtime/Cargo.toml` (`a2a-production` feature)
- `gep-a2a@1.0.0` envelope parsing and compatibility bridging:
  - `crates/oris-runtime/src/execution_server/api_handlers.rs` (`GEP_A2A_PROTOCOL_*`, `parse_gep_envelope_or_plain`)
- Orchestrator runtime client targeting `/a2a/*`:
  - `crates/oris-orchestrator/src/runtime_client.rs` (`handshake/fetch/publish/claim/complete/heartbeat`)

## Unified mapping: code gaps -> remote GitHub issues

| Issue | Scope | Endpoint family | Current code status | Alignment action |
| --- | --- | --- | --- | --- |
| [#148](https://github.com/Colin4k1024/Oris/issues/148) | Protocol core semantics | `/a2a/validate`, `/a2a/report`, `/a2a/decision`, `/a2a/revoke`, `/a2a/policy/model-tiers` | Not routed in `with_a2a_routes` | Add endpoints + deterministic error/idempotency contract |
| [#149](https://github.com/Colin4k1024/Oris/issues/149) | Task lifecycle semantics | `/a2a/task/*`, `/a2a/ask`, swarm/submit/release lifecycle | Partially present (`task claim/complete`, `tasks claim/report`, `work claim/complete`) but missing list/detail/submit/release/my/eligible-count/ask | Expand from claim/complete API into full lifecycle state machine |
| [#150](https://github.com/Colin4k1024/Oris/issues/150) | Asset discovery semantics | `/a2a/assets/search|ranked|explore|recommended|trending|categories` | Not routed | Implement discovery query/filter/ranking + deterministic pagination |
| [#151](https://github.com/Colin4k1024/Oris/issues/151) | Asset detail + governance semantics | `/a2a/assets/:id/*` (`verify/audit/vote/reviews/...`) | Not routed | Implement asset detail, governance writes, auth-sensitive behavior |
| [#152](https://github.com/Colin4k1024/Oris/issues/152) | Council workflow | `/a2a/council/*` | Not routed | Add proposal/vote/execute/session semantics |
| [#153](https://github.com/Colin4k1024/Oris/issues/153) | Project workflow | `/a2a/project/*` + suggestions/list | Not routed | Add project state machine + deterministic listing |
| [#154](https://github.com/Colin4k1024/Oris/issues/154) | Service/bid/dispute rule semantics | `/a2a/service/*`, `/a2a/bid/*`, `/a2a/dispute/rule` | Not routed; current dispute endpoints are limited to `/a2a/dispute/open|evidence|resolve` | Add economic lifecycle semantics and settlement rules |
| [#155](https://github.com/Colin4k1024/Oris/issues/155) | Cross-cutting audit/auth/contract/E2E parity | All new semantic endpoints | Existing tests mainly cover current compatibility routes, not the new semantic set | Extend audit target mapping, auth matrix, contract/E2E tests |

## Data-layer consistency risk (must be tracked with #155 acceptance)

- SQLite side has EvoMap-alignment schema migrations (`v12`, `v13`) in:
  - `crates/oris-execution-runtime/src/sqlite_runtime_repository.rs`
- Postgres side still has many `TODO` methods for bounty/swarm/worker/recipe/organism/session/dispute:
  - `crates/oris-execution-runtime/src/postgres_runtime_repository.rs` (section around lines ~1741+)

Without closing Postgres parity, semantic endpoint behavior will diverge by backend.

## Recommended execution order

1. `#148` protocol core semantics
2. `#149` task lifecycle semantics
3. `#150` + `#151` asset discovery/detail semantics
4. `#152` + `#153` governance/project workflows
5. `#154` economic service/bid/dispute semantics
6. `#155` cross-cutting hardening and end-to-end closure

## Notes

- This file supersedes the old "March 5 delta" as the active alignment tracker.
- Keep this file and the issue bodies synchronized whenever endpoint scope changes.
