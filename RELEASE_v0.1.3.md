# v0.1.3 - PostgreSQL backup and restore runbook

Patch release for `oris-runtime` that documents and rehearses PostgreSQL backup and restore for runtime data.

## What's in this release

- Add a PostgreSQL backup and restore runbook covering backup, restore, validation queries, and an executed local rehearsal for runtime state.
- Add a repeatable rehearsal script that seeds a runtime schema, captures a `pg_dump`, restores it, and verifies queued work plus lease ownership survive the round trip.

## Validation

- cargo fmt --all -- --check
- sh -n /Users/jiafan/Desktop/work-code/Oris/scripts/rehearse-postgres-backup-restore.sh
- sh /Users/jiafan/Desktop/work-code/Oris/scripts/rehearse-postgres-backup-restore.sh
- cargo build --verbose --all --release --all-features
- cargo test --release --all-features
- cargo publish -p oris-runtime --all-features --dry-run --registry crates-io
- cargo publish -p oris-runtime --all-features --registry crates-io

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris
