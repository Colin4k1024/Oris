# Release Notes — oris-runtime v0.55.0

## Summary

Adds 16 unit tests covering the A2A protocol core semantics exposed under the
`a2a-production` feature flag (EVOMAP-148 / issue #328).

All five endpoints now have validated test coverage:

| Endpoint | Tests added |
|---|---|
| `POST /a2a/validate` | 5 |
| `POST /a2a/report` | 4 |
| `POST /a2a/decision` | 3 |
| `POST /a2a/revoke` | 2 |
| `GET /a2a/policy/model-tiers` | 2 |

## Test coverage highlights

- **validate**: accepted-with-default-tier, tier-gate-reject (A3 < A5),
  capability filtering (known vs. unknown caps), optional-fields-only payload,
  arbitrary sender_id accepted as-is.
- **report**: submit-then-report happy path, idempotency key returns same
  `submission_id`, 404 for unknown task, 4xx when `sender_id` is absent.
- **decision**: accept transitions task status to `accepted`, invalid decision
  value (`"maybe"`) returns 4xx, idempotency key returns same `decision_id`.
- **revoke**: empty `asset_ids` returns 4xx, invalid JSON payload returns 4xx.
- **model-tiers**: returns A1/A3/A5 tiers with a `default_tier` field;
  deterministic across consecutive requests.

## Validation

- `cargo fmt --all -- --check` — clean
- `cargo test -p oris-runtime --features "sqlite-persistence,execution-server,a2a-production" a2a_validate_` — 5/5 pass
- `cargo test -p oris-runtime --features "sqlite-persistence,execution-server,a2a-production" a2a_report_` — 4/4 pass
- `cargo test -p oris-runtime --features "sqlite-persistence,execution-server,a2a-production" a2a_decision_` — 3/3 pass
- `cargo test -p oris-runtime --features "sqlite-persistence,execution-server,a2a-production" a2a_revoke_` — 2/2 pass
- `cargo test -p oris-runtime --features "sqlite-persistence,execution-server,a2a-production" a2a_model_tiers_` — 2/2 pass
- `cargo build --all --release --all-features` — clean
- `cargo test --release --all-features` — 0 failures

## Breaking changes

None. No public API changes; test-only additions under existing experimental
feature flags.
