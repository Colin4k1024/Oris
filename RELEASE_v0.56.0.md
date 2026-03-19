# Release Notes — oris-runtime v0.56.0

## Summary

Implements the A2A task lifecycle semantics (EVOMAP-149 / issue #329) and fixes
a pre-existing test failure caused by the lifecycle route missing under the
`a2a-production` feature set.

## Changes

### Bug fix — lifecycle route available under `a2a-production`

The `/v1/evolution/a2a/tasks/:task_id/lifecycle` route was previously registered
only in `with_evolution_routes`, which is a no-op when `a2a-production` is the
active feature without `full-evolution-experimental`. This caused the existing
test `evolution_a2a_task_complete_endpoint_maps_terminal_state_and_clears_claimability`
to panic with 404. The route is now also registered in `with_a2a_routes` under
a `#[cfg(not(feature = "full-evolution-experimental"))]` guard to avoid
duplicate-route panics when `full-evolution-experimental` is also active.

### Tests — A2A task lifecycle semantics (17 new tests)

All new tests are guarded by
`#[cfg(all(feature = "agent-contract-experimental", feature = "evolution-network-experimental"))]`
and are reachable under the `a2a-production` composite flag.

| Endpoint | Tests added |
|---|---|
| `POST /a2a/task/submit` | 4 |
| `GET /a2a/task/list` | 3 |
| `GET /a2a/task/:id` | 2 |
| `GET /a2a/task/my` | 2 |
| `GET /a2a/task/eligible-count` | 2 |
| `POST /a2a/task/release` | 2 |
| `POST /a2a/ask` | 2 |
| Full lifecycle (submit→detail→eligible→release) | 1 |

**Total new tests: 18** (17 `a2a_task_*` + 1 cross-endpoint lifecycle test)

## Validation

- `cargo fmt --all -- --check` — clean
- `cargo test -p oris-runtime --features "sqlite-persistence,execution-server,a2a-production" a2a_task_` — 21/21 pass (3 pre-existing + 18 new)
- `cargo build --all --release --all-features` — clean
- `cargo test --release --all-features` — 0 failures

## Breaking changes

None. No public API changes. One route (`/v1/evolution/a2a/tasks/:task_id/lifecycle`)
is now additionally exposed under `a2a-production` (without
`full-evolution-experimental`); the `full-evolution-experimental` path is
unchanged.
