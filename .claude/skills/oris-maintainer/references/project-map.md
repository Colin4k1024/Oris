# Project Map

## Workspace Layout

- Root workspace manifest: `Cargo.toml`
- Publishable crate: `crates/oris-runtime/Cargo.toml`
- Main library entry: `crates/oris-runtime/src/lib.rs`
- Example workspace members:
  - `examples/vector_store_surrealdb`
  - `examples/oris_starter_axum`

## High-Value Project Files

- Product and positioning overview: `README.md`
- Strategy and long-range roadmap: `docs/ORIS_2.0_STRATEGY.md`
- Runtime schema notes: `docs/runtime-schema-migrations.md`
- Kernel and API details: `docs/kernel-api.md`
- Prior release note pattern: `RELEASE_v0.1.0.md`

## GitHub and Automation Hooks

- Main CI and publish workflow: `.github/workflows/ci.yml`
- Basic Rust workflow: `.github/workflows/rust.yml`
- Feature and bug issue templates: `.github/ISSUE_TEMPLATE/`
- Pull request template: `.github/PULL_REQUEST_TEMPLATE.md`

## Utility Scripts

- `scripts/import_issues_from_csv.sh`: import roadmap items from `docs/issues-roadmap.csv` into GitHub issues
- `scripts/scaffold_example_template.sh`: scaffold new example code
- `scripts/run-pgvector`: helper for local pgvector-related work

## Maintenance Heuristics

- Start every issue by locating the narrowest crate module or example that the issue affects.
- Treat `crates/oris-runtime` as the release-critical surface. Keep changes there deliberate and review `README.md` when public behavior changes.
- Read CI definitions before adding or changing validation steps so local checks stay aligned with the existing pipeline.
