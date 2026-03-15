# oris-intake v0.2.0 - CI/CD Webhook Integration

GitHub CI failure events now flow automatically into the Oris intake pipeline via `GithubIntakeSource`, enabling zero-touch Detect for `check_run` and `workflow_run` webhook events.

## What's in this release

- **`GithubIntakeSource`** — concrete `IntakeSource` implementation that processes raw GitHub webhook JSON for both `check_run` (CI check suite failures) and `workflow_run` events; auto-dispatches on payload shape when no explicit event type is supplied
- **`GithubCheckRunEvent`** — typed deserialization struct for GitHub `check_run` webhook payloads including `id`, `name`, `head_sha`, `conclusion`, and `output` (title + summary)
- **`from_github_check_run()`** — converts a parsed `GithubCheckRunEvent` into an `IntakeEvent` with severity, signals (`check_run_conclusion:*`, `commit_sha:*`, `output_title:*`), and timestamp
- Signal dedup ≥ 95% — same-root-cause events are deduplicated; existing `Deduplicator` validated to 19/20 (95%) hit rate in unit test
- Priority label ordering validated: Critical > High > Medium > Low > Info; Critical scores ≥ 75
- 8 new integration tests in `crates/oris-intake/tests/webhook_integration.rs` covering all 4 acceptance criteria

## Validation

- `cargo fmt --all -- --check` ✓
- `cargo test -p oris-intake` — 25/25 pass (17 unit + 8 integration tests)

## Links

- Crate: https://crates.io/crates/oris-intake
- Repo: https://github.com/Colin4k1024/Oris
