# oris-intake v0.4.0

## Summary

Adds an Axum HTTP webhook server for the Oris Intake system (Phase 2 P2-01).

## Changes

### New: `server` module (`oris-intake::server`, feature `webhook`)

- `WebhookServer` builder — create with `new(tx)`, configure with `with_github_secret()` / `with_gitlab_token()`, build router with `into_router()`
- `POST /webhooks/github` — parses GitHub Actions / check_run events; verifies `X-Hub-Signature-256` HMAC-SHA256 when `github_secret` is configured (returns 403 on failure)
- `POST /webhooks/gitlab` — parses GitLab CI pipeline events; verifies `X-Gitlab-Token` in constant time when `gitlab_token` is configured (returns 403 on failure)
- `POST /webhooks/prometheus` — parses Alertmanager v4 payloads via `PrometheusIntakeSource`
- `POST /webhooks/sentry` — parses Sentry issue-alert payloads via `SentryIntakeSource`
- Events are emitted to a caller-supplied `tokio::sync::mpsc` channel

### Feature Flag

```toml
oris-intake = { version = "0.4", features = ["webhook"] }
```

### Bug Fix: `GitlabPipelineAttributes.ref_`

Added `#[serde(rename = "ref")]` to correctly deserialize the `ref` field from real GitLab webhook JSON.

## Validation

- `cargo fmt --all -- --check` ✓
- `cargo build -p oris-intake --features webhook` clean ✓
- `cargo test -p oris-intake --features webhook` — 55 passed, 0 failed ✓
- `cargo publish -p oris-intake --dry-run` ✓

## Crate versions

- `oris-intake` v0.3.0 → **v0.4.0**
