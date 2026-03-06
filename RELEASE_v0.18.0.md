# v0.18.0 - Deterministic scheduler lease hardening for A2A flows

`oris-runtime` v0.18.0 ships deterministic lease-ownership hardening for compatibility `/a2a` claim/report/complete flows.

## What's in this release

- Enforced active-lease ownership checks for `/a2a/tasks/report` and `/a2a/task/complete` write paths:
  - report/complete now require an active claim held by the caller.
  - stale or expired leases are rejected deterministically.
  - writes from non-owner claimers are rejected.
- Tightened SQLite lease touch semantics so heartbeat-style lease extension only applies to the active owner.
- Added deterministic ordering tiebreaker (`session_id`) for equal enqueue timestamps when listing/claiming compatibility tasks.
- Added regression coverage for:
  - report without active claim rejection,
  - complete with expired lease rejection,
  - deterministic claim ordering with enqueue-time ties,
  - strict owner-only lease touch behavior.

## Validation

- cargo fmt --all
- cargo test -p oris-execution-runtime --features sqlite-persistence claim_a2a_compat_task_uses_session_id_tiebreaker_for_equal_enqueue_time
- cargo test -p oris-execution-runtime --features sqlite-persistence touch_a2a_compat_task_lease_requires_active_owner
- cargo test -p oris-runtime --all-features evolution_a2a_tasks_report_rejects_running_without_active_claim
- cargo test -p oris-runtime --all-features evolution_a2a_task_complete_rejects_expired_claim_lease
- cargo test -p oris-runtime --all-features evolution_a2a_compat_distribute_and_report_map_to_session_flow
- cargo test -p oris-runtime --all-features metrics_endpoint_exposes_a2a_compat_metrics
- cargo fmt --all -- --check
- cargo build --verbose --all --release --all-features
- cargo test --release --all-features
- cargo publish -p oris-runtime --all-features --dry-run --registry crates-io
- cargo publish -p oris-runtime --all-features --registry crates-io

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris
