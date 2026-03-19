# oris-runtime v0.60.0

## Summary

Implements the A2A Project Workflow endpoints ([EVOMAP-153][P1], issue #333) with 11 new integration tests covering create, detail, state transitions, per-project suggestions, and list operations.

## Changes

### New Endpoints

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/a2a/project/create` | Create a project (idempotent by proposer+title+summary+tags) |
| `GET`  | `/a2a/project/:id` | Retrieve a project by ID |
| `POST` | `/a2a/project/:id/state` | Transition lifecycle state (active/paused/completed) |
| `GET`  | `/a2a/project/:id/suggestions` | Get suggestions scoped to a specific project |
| `GET`  | `/a2a/project/list` | List projects with optional status/owner_id/query/limit/offset filters |

### Data Model

- `EvomapProjectRecord` gains a new `lifecycle_state: Option<String>` field, orthogonal to the existing status machine (Proposed/Claimed/InProgress/…).
- `evomap_project_json()` helper now serializes `lifecycle_state`.
- `evomap_project_propose` initializes records with `lifecycle_state: Some("active")`.

### Tests Added (11)

- `a2a_project_create_returns_project`
- `a2a_project_create_missing_title_rejected`
- `a2a_project_get_returns_project`
- `a2a_project_get_unknown_returns_404`
- `a2a_project_state_active`
- `a2a_project_state_paused`
- `a2a_project_state_completed`
- `a2a_project_state_invalid_rejected`
- `a2a_project_list_get_returns_list`
- `a2a_project_list_get_status_filter`
- `a2a_project_id_suggestions_returns_suggestions`

## Validation

- `cargo fmt --all -- --check` ✅
- `cargo test -p oris-runtime --features "sqlite-persistence,execution-server,a2a-production" a2a_project_ -- --nocapture` → 11/11 ✅
- `cargo build --all --release --all-features` ✅
- `cargo test --release --all-features` → 0 failures ✅
- `cargo publish --dry-run` ✅
