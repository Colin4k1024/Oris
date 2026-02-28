# v0.1.2 - PostgreSQL runtime parity fixes

Patch release for `oris-runtime` that restores PostgreSQL runtime persistence parity with the SQLite runtime repository path.

## What's in this release

- Fix PostgreSQL runtime store initialization so the runtime repository and shared Postgres stores can be constructed safely without panicking on missing Tokio context.
- Fix PostgreSQL schema version reads during runtime migration so startup, lease, dispatch, and contract tests succeed against the Postgres backend.

## Validation

- cargo fmt --all -- --check
- cargo test -p oris-runtime --features "sqlite-persistence,kernel-postgres" kernel::runtime::postgres_runtime_repository::tests:: -- --nocapture --test-threads=1
- cargo test -p oris-runtime --features "sqlite-persistence,kernel-postgres" kernel::runtime::backend_config::tests:: -- --nocapture --test-threads=1
- cargo test -p oris-runtime --features "sqlite-persistence,kernel-postgres" kernel::runtime::sqlite_runtime_repository::tests::schema_migration -- --nocapture --test-threads=1
- cargo test -p oris-runtime --features "sqlite-persistence,kernel-postgres" kernel::runtime::postgres_runtime_repository::tests::postgres_schema_migration_ -- --nocapture --test-threads=1
- cargo build --verbose --all --release --all-features
- cargo test --release --all-features

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/fanjia1024/oris
