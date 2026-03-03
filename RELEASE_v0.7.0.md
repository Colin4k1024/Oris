# v0.7.0 - EvoKernel Observability Metrics

Minor release adding built-in EvoKernel observability metrics and health reporting to `oris-runtime`.

## What's in this release

- The execution server now exposes store-derived evolution metrics for replay success, promotion ratio, revoke frequency, and mutation velocity on the existing Prometheus `/metrics` endpoint.
- The execution server now serves a built-in `/healthz` endpoint that reports the current evolution observability snapshot for scrape and readiness checks.

## Validation

- cargo fmt --all -- --check
- cargo test -p oris-evokernel metrics_snapshot_tracks_replay_promotion_and_revocation_signals -- --nocapture
- cargo test -p oris-runtime --features "execution-server,evolution-network-experimental,sqlite-persistence" evolution_metrics_and_health_are_exposed_from_runtime_routes -- --nocapture
- cargo test -p oris-runtime --features "execution-server,evolution-network-experimental,sqlite-persistence" metrics_endpoint_is_scrape_ready_and_exposes_runtime_metrics -- --nocapture
- cargo test -p oris-runtime --features "execution-server,evolution-network-experimental,sqlite-persistence" evolution_publish_fetch_and_revoke_routes_work -- --nocapture
- cargo test -p oris-evokernel -- --nocapture
- cargo test --workspace
- /bin/zsh -lc "unset ORT_LIB_LOCATION ORT_PREFER_DYNAMIC_LINK ORT_LIB_PROFILE; cargo build --verbose --all --release --all-features"
- /bin/zsh -lc "unset ORT_LIB_LOCATION ORT_PREFER_DYNAMIC_LINK ORT_LIB_PROFILE; cargo test --release --all-features"

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris
