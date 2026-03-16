---
applyTo: "**"
---

# Oris Maintainer

Maintain the Oris Rust agent runtime with an issue-driven GitHub workflow.
When working in this repository, pick up a GitHub issue, implement the scoped change, validate the Rust workspace, publish `oris-runtime` to crates.io, open a PR targeting the default branch, and close the issue after the PR merges.

Full workflow, operating rules, and reference commands live in [`skills/oris-maintainer/SKILL.md`](../../skills/oris-maintainer/SKILL.md). Read that file at the start of every issue cycle.

---

## Workflow Summary

### 1. Preflight

```bash
git status --short --branch
gh auth status
gh issue list --state open --limit 20
```

### 2. Select Issue

```bash
gh issue view <issue_number>
```

Post `Status: in progress` comment immediately. Follow selection order in `skills/oris-maintainer/references/issue-selection.md`.

### 3. Plan & Branch

- Branch naming: `codex/issue-<number>-<slug>`
- Classify the issue with `skills/oris-maintainer/references/issue-test-matrix.md` before coding.

### 4. Develop

```bash
cargo fmt --all
cargo test -p oris-runtime <targeted_test_or_module>
```

Repeat until stable. Change only what the issue requires.

### 5. Pre-Release Validation

```bash
cargo fmt --all -- --check
cargo build --verbose --all --release --all-features
cargo test --release --all-features
```

Run additional subsystem commands from `skills/oris-maintainer/references/validation-and-release.md` for the issue type.

### 6. Publish

```bash
cargo publish -p oris-runtime --all-features --dry-run
cargo publish -p oris-runtime --all-features
```

Draft `RELEASE_v<version>.md` before publishing. Version bump policy: `skills/oris-maintainer/references/versioning-policy.md`.

### 7. Open PR and Auto-Merge

```bash
git push

# Post released status comment before PR is merged
gh issue comment <issue_number> --body "Status: released

Released version:
- oris-runtime v<version>"

# Determine default branch and open PR
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

# Enable auto-merge
gh pr merge --auto --squash
```

> If auto-merge is not enabled on the repository, run `gh pr merge --squash` manually after review.

### 8. Close Issue (after PR merges)

```bash
gh issue close <issue_number> --comment "Completed and released in oris-runtime v<version>. Merged via PR #<pr_number>."
```

Do **not** close the issue while auto-merge is still pending.

---

## Operating Rules

- Never close an issue before the PR has merged and the publish succeeded.
- Never bump crate versions speculatively.
- Never reuse stale release notes — each crate version gets its own `RELEASE_v<version>.md`.
- Surface blocked steps (missing secrets, services, or approvals) immediately.
- One explicit issue status at a time: `in progress`, `blocked`, or `released`.

---

## Key Reference Files

| File | Use when |
|------|----------|
| `skills/oris-maintainer/references/command-checklist.md` | Start of every issue — full ordered command groups |
| `skills/oris-maintainer/references/issue-selection.md` | Choosing the next issue from the open queue |
| `skills/oris-maintainer/references/issue-state-machine.md` | Updating issue status or posting comments |
| `skills/oris-maintainer/references/issue-test-matrix.md` | Determining required validation scope |
| `skills/oris-maintainer/references/versioning-policy.md` | Choosing the version bump |
| `skills/oris-maintainer/references/validation-and-release.md` | Full release sequence and CI-aligned regression commands |
| `skills/oris-maintainer/references/release-notes.md` | Drafting `RELEASE_v<version>.md` |
| `skills/oris-maintainer/references/publish-failure.md` | Any failed `cargo publish` step |
| `skills/oris-maintainer/references/issue-closeout.md` | Final issue comment and close command |
| `skills/oris-maintainer/references/project-map.md` | Repository layout before large changes |
