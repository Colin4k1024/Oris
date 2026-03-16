# Validation and Release

## Fast Iteration Checks

- Format the workspace: `cargo fmt --all`
- Run targeted tests first:
  - `cargo test -p oris-runtime <test_name_or_module>`
  - `cargo test --workspace`

Use the narrowest command that proves the issue is fixed before moving to broader validation.

## Pre-Release Baseline

Run these before a crates.io publish:

```bash
cargo fmt --all -- --check
cargo build --verbose --all --release --all-features
cargo test --release --all-features
```

## CI-Aligned Regression Commands

Run these when the changed issue touches the matching subsystem:

- SQLite runtime schema migration:

```bash
cargo test -p oris-runtime --features "sqlite-persistence,kernel-postgres" kernel::runtime::sqlite_runtime_repository::tests::schema_migration -- --nocapture --test-threads=1
```

- Postgres runtime schema migration (environment-dependent):

```bash
cargo test -p oris-runtime --features "sqlite-persistence,kernel-postgres" kernel::runtime::postgres_runtime_repository::tests::postgres_schema_migration_ -- --nocapture --test-threads=1
```

- Backend config startup checks:

```bash
cargo test -p oris-runtime --features "sqlite-persistence,kernel-postgres" kernel::runtime::backend_config::tests:: -- --nocapture --test-threads=1
```

- Security regression checks for execution server APIs:

```bash
cargo test -p oris-runtime --features "sqlite-persistence,execution-server" kernel::runtime::api_handlers::tests::security_ -- --nocapture --test-threads=1
```

## Release Sequence

1. Choose the next version with `references/versioning-policy.md`.
2. Update and confirm `version` in `crates/oris-runtime/Cargo.toml`.
3. Confirm the worktree is intentional and clean enough for a release.
4. Dry-run the publish when possible:

```bash
cargo publish -p oris-runtime --all-features --dry-run
```

5. Publish the crate:

```bash
cargo publish -p oris-runtime --all-features
```

6. If using git tags, align the tag to `v<version>`. The CI workflow enforces that tag and crate version match.
7. Push the branch and tag.
8. Determine the repository default branch and open a PR targeting it:

```bash
DEFAULT_BRANCH=$(gh repo view --json defaultBranchRef -q .defaultBranchRef.name)

gh pr create \
  --base "$DEFAULT_BRANCH" \
  --title "fix: <one-line issue title> (#<issue_number>)" \
  --body "Closes #<issue_number>

## Summary
<one-line behavior change>

## Validation
- \`cargo fmt --all -- --check\`
- \`<targeted test command>\`
- \`cargo publish -p oris-runtime --all-features --dry-run\` passed
- Released as oris-runtime v<version>"
```

9. Enable auto-merge so the PR merges automatically once required checks pass:

```bash
gh pr merge --auto --squash
```

> If the repository does not have auto-merge enabled, run `gh pr merge --squash` manually after review.

Close the issue only after the PR has merged. Document the released version and PR number in the final issue comment.

## Safety Rules

- `cargo publish` is externally visible and effectively irreversible for that exact version. Run it only after validation is complete.
- If a required service, token, or network path is unavailable, stop before publish and report the missing prerequisite.
- Never claim that a release is complete until the publish command succeeds.
