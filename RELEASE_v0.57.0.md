# Release: oris-runtime v0.57.0

## Summary

Adds 13 contract tests covering the A2A Asset Discovery API endpoints (issue #330).

## Changes

- Added `a2a_assets_search_requires_sender_id` — verifies 400 when `sender_id` is absent
- Added `a2a_assets_search_empty_sender_id_rejected` — verifies 400 when `sender_id` is empty
- Added `a2a_assets_search_requires_handshake` — verifies 403 when no prior handshake
- Added `a2a_assets_search_returns_results_shape` — 200 with `mode`, `results`, `total`, `limit`, `offset`, `idempotent`
- Added `a2a_assets_search_mode_field_is_search` — `mode == "search"` for the search endpoint
- Added `a2a_assets_ranked_returns_ranked_mode` — `mode == "ranked"`, results array present
- Added `a2a_assets_explore_returns_explore_mode` — `mode == "explore"`
- Added `a2a_assets_recommended_returns_recommended_mode` — `mode == "recommended"`
- Added `a2a_assets_trending_returns_trending_mode` — `mode == "trending"`
- Added `a2a_assets_categories_returns_categories_shape` — `categories` array + `total_categories` + `idempotent`
- Added `a2a_assets_search_pagination_limit_respected` — `limit=1` returns at most one result
- Added `a2a_assets_ranked_deterministic_for_same_inputs` — two identical ranked requests return identical results
- Added `a2a_assets_search_idempotent_flag_is_true` — all six discovery endpoints advertise `idempotent: true`

## Validation

- `cargo fmt --all -- --check` ✓
- `cargo test -p oris-runtime --features "sqlite-persistence,execution-server,a2a-production" a2a_assets_` → 13 passed
- `cargo build --all --release --all-features` ✓
- `cargo test --release --all-features` → all passed
