# Release: oris-runtime v0.58.0

## Summary

Adds 13 contract tests covering A2A asset detail and governance semantics for issue #331.

## Changes

- Added `a2a_asset_detail_requires_sender_id` to verify GET detail rejects missing `sender_id`
- Added `a2a_asset_detail_unknown_asset_returns_404` to verify unknown assets return 404
- Added `a2a_asset_detail_returns_full_shape` to verify detail response includes asset and governance summaries
- Added `a2a_asset_detail_verify_records_verification` to verify `/verify` records a verification result
- Added `a2a_asset_detail_verify_idempotent_on_repeat` to verify repeated identical verify submissions are idempotent
- Added `a2a_asset_detail_verify_rejects_invalid_status` to verify invalid verification status returns 400
- Added `a2a_asset_detail_vote_records_vote` to verify `/vote` records governance votes
- Added `a2a_asset_detail_vote_idempotent_on_repeat` to verify repeated identical vote submissions are idempotent
- Added `a2a_asset_detail_vote_rejects_invalid_vote_value` to verify invalid vote values return 400
- Added `a2a_asset_detail_audit_trail_reflects_governance_events` to verify audit trail shows verify and vote events
- Added `a2a_asset_detail_reviews_returns_reviews_shape` to verify reviews list shape and contents
- Added `a2a_asset_detail_governance_worker_role_rejected_on_verify` to verify worker-role callers are forbidden on verify
- Added `a2a_asset_detail_governance_worker_role_rejected_on_vote` to verify worker-role callers are forbidden on vote

## Validation

- `cargo fmt --all -- --check` ✓
- `cargo test -p oris-runtime --features "sqlite-persistence,execution-server,a2a-production" a2a_asset_detail_` → 13 passed
- `cargo build --all --release --all-features` ✓
- `cargo test --release --all-features` ✓
