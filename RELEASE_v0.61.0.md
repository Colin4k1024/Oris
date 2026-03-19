# oris-runtime v0.61.0

## Summary

Implements A2A Economic Lifecycle endpoints ([EVOMAP-154][P1], issue #334) covering service registration, discovery, bid submission, deterministic bid evaluation, and dispute rule querying, with 16 new integration tests.

## Changes

### New Endpoints

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/a2a/service/register` | Register a node service offering (idempotent) |
| `GET`  | `/a2a/service/list` | List services with category/status/owner/query filters |
| `GET`  | `/a2a/service/:id` | Retrieve a specific service by ID |
| `POST` | `/a2a/bid/submit` | Submit a bid for a service contract (idempotent) |
| `GET`  | `/a2a/bid/:id` | Retrieve a specific bid by ID |
| `POST` | `/a2a/bid/evaluate` | Deterministically evaluate competing bids (strategy: highest_bid|lowest_bid) |
| `GET`  | `/a2a/dispute/rule` | Query canonical ruleset or retrieve a specific stored dispute rule |

### Bid Evaluation

Deterministic settlement logic with two versioned strategies:
- `highest_bid` (default) — selects the open bid with the highest `amount`; ties broken by earliest submission
- `lowest_bid` — selects the open bid with the lowest `amount`; same tie-break

Result carries `schema_version: "v1"` for auditability.

### Dispute Rule Querying

`GET /a2a/dispute/rule` now:
- Without `?dispute_id` → returns the canonical ruleset definition (4 decisions + aliases)
- With `?dispute_id=<id>` → retrieves the specific stored rule record

## Tests Added (16)

### Service
- `a2a_service_register_returns_service`
- `a2a_service_register_missing_title_rejected`
- `a2a_service_get_returns_service`
- `a2a_service_get_unknown_returns_404`
- `a2a_service_list_returns_services`
- `a2a_service_list_category_filter`

### Bid
- `a2a_bid_submit_returns_bid`
- `a2a_bid_get_returns_bid`
- `a2a_bid_get_unknown_returns_404`
- `a2a_bid_evaluate_returns_winner`
- `a2a_bid_evaluate_lowest_bid_strategy`
- `a2a_bid_evaluate_non_owner_forbidden`
- `a2a_bid_evaluate_invalid_strategy_rejected`
- `a2a_bid_evaluate_no_open_bids_returns_no_open_bids`

### Dispute Rule
- `a2a_dispute_rule_get_returns_ruleset`
- `a2a_dispute_rule_get_by_id_returns_stored_rule`

## Validation

- `cargo fmt --all -- --check` ✅
- `cargo test -p oris-runtime --features "sqlite-persistence,execution-server,a2a-production" a2a_service_` → 6/6 ✅
- `cargo test -p oris-runtime --features "sqlite-persistence,execution-server,a2a-production" a2a_bid_` → 8/8 ✅
- `cargo test -p oris-runtime --features "sqlite-persistence,execution-server,a2a-production" a2a_dispute_rule` → 2/2 ✅
- `cargo build --all --release --all-features` ✅
- `cargo test --release --all-features` → 0 failures ✅
- `cargo publish --dry-run` ✅
