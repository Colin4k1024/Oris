# Runtime Schema Migrations

This document defines the schema migration workflow for runtime persistence layers.

## Scope

- SQLite runtime repository (`SqliteRuntimeRepository`)
- PostgreSQL runtime repository (`PostgresRuntimeRepository`)

Migrations are forward-only and idempotent.

## Version Tracking

### SQLite

- Table: `runtime_schema_migrations`
- Columns: `version`, `name`, `applied_at_ms`
- Current version: `2`

### PostgreSQL

- Table: `runtime_schema_migrations` in runtime schema
- Columns: `version`, `name`, `applied_at_ms`
- Current version: `2`

## Local Validation

Run migration regression tests:

```bash
cargo test -p oris-runtime --features "sqlite-persistence" kernel::runtime::sqlite_runtime_repository::tests::schema_migration -- --nocapture --test-threads=1
cargo test -p oris-runtime --features "sqlite-persistence,kernel-postgres" kernel::runtime::postgres_runtime_repository::tests::postgres_schema_migration_ -- --nocapture --test-threads=1
```

To validate PostgreSQL upgrade behavior end-to-end:

```bash
export ORIS_TEST_POSTGRES_URL=postgres://<user>:<password>@<host>:5432/<db>
cargo test -p oris-runtime --features "sqlite-persistence,kernel-postgres" kernel::runtime::postgres_runtime_repository::tests:: -- --nocapture --test-threads=1
```

## CI Validation

- `cargo test --all-features` includes SQLite migration tests.
- PostgreSQL-specific migration tests are environment-gated by `ORIS_TEST_POSTGRES_URL`.

## Rollback Path

Runtime schema migrations are forward-only. Rollback is operational:

1. Stop writers (API + workers) to quiesce runtime mutations.
2. Restore persistence from backup/snapshot taken before migration.
3. Deploy the previous application version.
4. Restart control plane and workers.

### SQLite rollback

1. Stop runtime services.
2. Replace SQLite file with pre-upgrade backup.
3. Restart services with previous binary.

### PostgreSQL rollback

1. Stop runtime services.
2. Restore database/schema from pre-upgrade backup (logical or physical).
3. Deploy previous binary and re-enable traffic.

## Operator Notes

- If runtime starts against a newer schema version than supported, startup fails fast.
- Always take a backup before deploying binaries that may apply new migrations.
