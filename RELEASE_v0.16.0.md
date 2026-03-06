# v0.16.0 - Remote issue orchestration and roadmap sync

`oris-runtime` v0.16.0 ships the EvoMap compatibility completion stream together with automation that keeps the roadmap ledger aligned to remote GitHub issues.

## What's in this release

- Added remote GitHub issue listing and deterministic issue selection in `oris-orchestrator` (`P0 > P1`, then milestone, then issue number), with RFC/blocked filtering and single-issue execution entrypoints.
- Added roadmap sync backfill by exact title for empty `issue_number`, plus `--track` scoping and ambiguity-safe skip behavior, so CSV bookkeeping can be reconciled before orchestrator selection.
- Updated maintainer workflow docs to enforce the "sync roadmap first, then select issue" release loop.

## Validation

- python3 scripts/sync_issues_roadmap_status.py --repo Colin4k1024/Oris --track evomap-alignment --dry-run
- python3 -m pytest scripts/tests/test_sync_issues_roadmap_status.py
- cargo test -p oris-orchestrator
- cargo test -p oris-orchestrator --test github_adapter_http --release
- cargo fmt --all -- --check
- cargo build --verbose --all --release --all-features
- cargo test --release --all-features
- cargo publish -p oris-runtime --all-features --dry-run (failed: crates.io replaced by `aliyun`)
- cargo publish -p oris-runtime --all-features --dry-run --registry crates-io (failed: packaged build resolved stale crates.io dependency APIs)

## Release Status

- Status: blocked before publish
- Crate published: no

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris
