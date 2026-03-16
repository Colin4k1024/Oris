# Issue Test Matrix

## Rule

- Classify every issue into exactly one primary type before coding.
- Run the mapped commands for that type as the minimum validation floor.
- If an issue touches multiple areas, choose the highest-risk type or run the union of required commands.

## Types

Use one of these primary types:

- `docs-or-release`
- `bugfix`
- `feature`
- `persistence`
- `backend-config`
- `security`

## Required Validation by Type

### `docs-or-release`

Use for documentation-only changes, release note work, version bumps, or workflow plumbing that does not change runtime logic.
Default version bump: `patch`

Required commands:

```bash
cargo fmt --all -- --check
cargo build --verbose --all --release --all-features
```

### `bugfix`

Use for backward-compatible behavior fixes in existing runtime code.
Default version bump: `patch`

Required commands:

```bash
cargo fmt --all -- --check
cargo test -p oris-runtime <targeted_test_or_module>
cargo build --verbose --all --release --all-features
cargo test --release --all-features
```

### `feature`

Use for new backward-compatible public capability, new API surface, new tool, or new example-backed workflow.
Default version bump: `minor`

Required commands:

```bash
cargo fmt --all -- --check
cargo test -p oris-runtime <targeted_test_or_module>
cargo test --workspace
cargo build --verbose --all --release --all-features
cargo test --release --all-features
```

### `persistence`

Use for SQLite, Postgres, checkpoints, schema changes, runtime repository logic, or durable execution state changes.
Default version bump: `patch` unless the issue also adds a new backward-compatible public capability, in which case escalate to `minor`

Required commands:

```bash
cargo fmt --all -- --check
cargo test -p oris-runtime <targeted_test_or_module>
cargo build --verbose --all --release --all-features
cargo test --release --all-features
cargo test -p oris-runtime --features "sqlite-persistence,kernel-postgres" kernel::runtime::sqlite_runtime_repository::tests::schema_migration -- --nocapture --test-threads=1
cargo test -p oris-runtime --features "sqlite-persistence,kernel-postgres" kernel::runtime::postgres_runtime_repository::tests::postgres_schema_migration_ -- --nocapture --test-threads=1
```

### `backend-config`

Use for startup configuration, execution runtime wiring, env parsing, or backend initialization changes.
Default version bump: `patch` unless the issue adds a new user-facing configuration capability, in which case escalate to `minor`

Required commands:

```bash
cargo fmt --all -- --check
cargo test -p oris-runtime <targeted_test_or_module>
cargo build --verbose --all --release --all-features
cargo test --release --all-features
cargo test -p oris-runtime --features "sqlite-persistence,kernel-postgres" kernel::runtime::backend_config::tests:: -- --nocapture --test-threads=1
```

### `security`

Use for auth, permission checks, API handler protections, secret handling, or execution server security behavior.
Default version bump: `patch` unless the issue adds a new backward-compatible security feature exposed to users, in which case escalate to `minor`

Required commands:

```bash
cargo fmt --all -- --check
cargo test -p oris-runtime <targeted_test_or_module>
cargo build --verbose --all --release --all-features
cargo test --release --all-features
cargo test -p oris-runtime --features "sqlite-persistence,execution-server" kernel::runtime::api_handlers::tests::security_ -- --nocapture --test-threads=1
```

## Escalation Rule

- If the issue classification is unclear, do not guess silently. State the ambiguity and choose the safer, higher-risk test set.
- If a required command cannot run because of missing services or credentials, mark the issue `blocked` and record the exact gap.
- If the shipped impact requires a higher version bump than the type default, keep the issue type for testing but use the higher bump and record the reason.
