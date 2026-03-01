# Contributing to Oris

Thanks for your interest in contributing to Oris.

## Ways to contribute

- Report bugs and regressions.
- Propose features and design improvements.
- Improve docs, examples, and tests.
- Submit code changes through pull requests.

## Development setup

1. Install Rust stable (`rustup` recommended).
2. Clone the repository.
3. Build once to verify toolchain and dependencies:

```bash
cargo build --all
```

## Development workflow

1. Create a branch from `main`.
2. Make focused changes.
3. Run formatting, lint, and tests locally.
4. Open a pull request with clear context and validation steps.

## Local checks

Run these before opening a PR:

```bash
cargo fmt --all
bash scripts/update_runtime_api_contract.sh
bash scripts/check_release_metadata.sh
cargo clippy -p oris-runtime --lib --features "sqlite-persistence,execution-server,kernel-postgres" -- -D clippy::correctness -D clippy::suspicious -D clippy::perf
cargo clippy -p oris_starter_axum --no-deps -- -D warnings
cargo test --all-features
```

For runtime-specific work, also run:

```bash
cargo test -p oris-runtime kernel::driver::tests:: -- --nocapture
cargo test -p oris-runtime --features "sqlite-persistence,execution-server" kernel::runtime::api_handlers::tests:: -- --nocapture
cargo test -p oris-runtime --features "sqlite-persistence,kernel-postgres" kernel::runtime::postgres_runtime_repository::tests:: -- --nocapture --test-threads=1
cargo test -p oris-runtime --features "sqlite-persistence" kernel::runtime::sqlite_runtime_repository::tests::schema_migration -- --nocapture --test-threads=1
cargo test -p oris-runtime --features "sqlite-persistence,kernel-postgres" kernel::runtime::backend_config::tests:: -- --nocapture --test-threads=1
./scripts/run_scheduler_stress_suite.sh
cargo test -p oris-runtime --features "sqlite-persistence,execution-server" kernel::runtime::api_handlers::tests::run_to_worker_flow_propagates_trace_context_end_to_end -- --nocapture --test-threads=1
cargo test -p oris-runtime --features "sqlite-persistence,execution-server" kernel::runtime::api_handlers::tests::metrics_endpoint_is_scrape_ready_and_exposes_runtime_metrics -- --nocapture --test-threads=1
cargo test -p oris-runtime --features "sqlite-persistence,execution-server" kernel::runtime::api_handlers::tests::observability_assets_reference_metrics_present_in_sample_workload -- --nocapture --test-threads=1
cargo test -p oris-runtime agent::checkpoint::tests:: -- --nocapture
cargo test -p oris-runtime graph::plugin::tests:: -- --nocapture
cargo check -p oris-runtime --example custom_node_plugins --offline
cargo check -p oris-runtime --example execution_server --features "sqlite-persistence,execution-server" --offline
cargo check -p oris_starter_axum --offline
bash scripts/verify_cargo_generate_templates.sh
```

To execute the PostgreSQL branch of the runtime repository contract tests, set:

```bash
export ORIS_TEST_POSTGRES_URL=postgres://<user>:<password>@<host>:5432/<db>
```

Migration workflow and rollback runbook:

- [docs/runtime-schema-migrations.md](docs/runtime-schema-migrations.md)
- [docs/production-operations-guide.md](docs/production-operations-guide.md)
- [docs/incident-response-runbook.md](docs/incident-response-runbook.md)

For security-focused changes, run the dedicated regression slice:

```bash
cargo test -p oris-runtime --features "sqlite-persistence,execution-server" kernel::runtime::api_handlers::tests::security_ -- --nocapture --test-threads=1
```

For scheduler failover/conflict changes, the stress suite is the fast regression entrypoint:

```bash
./scripts/run_scheduler_stress_suite.sh
```

Baseline output reference:

- [docs/scheduler-stress-baseline.md](docs/scheduler-stress-baseline.md)

Runtime API contract:

- Canonical artifact: [docs/runtime-api-contract.json](docs/runtime-api-contract.json)

## Pull request expectations

- Keep PRs small and scoped to one problem.
- Include tests for behavior changes.
- Update docs/examples when public behavior changes.
- Avoid unrelated refactors in the same PR.
- Add migration notes when changing public API shape.

Use the PR template in `.github/PULL_REQUEST_TEMPLATE.md`.

## Issue reporting

Use GitHub issue templates for bugs and feature requests:

- `.github/ISSUE_TEMPLATE/bug_report.md`
- `.github/ISSUE_TEMPLATE/feature_request.md`

For security reports, do not open a public issue. See [SECURITY.md](SECURITY.md).

## Code style

- Follow Rust idioms and `rustfmt`.
- Prefer explicit, testable behavior over implicit magic.
- Preserve backward compatibility where possible.

## License

By contributing, you agree that your contributions are licensed under the same MIT license as this repository. See [LICENSE](LICENSE).

## Community standards

This project follows [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
