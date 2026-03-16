# Command Checklist

## Rule

- Use these command groups as the default order of operations for every issue.
- Run the full group unless the issue clearly does not require a step.
- If you skip a command, say why in the working notes or issue comment.

## 1. Preflight

Run these before selecting work:

```bash
git status --short --branch
gh auth status
python3 scripts/sync_issues_roadmap_status.py --dry-run
gh issue list --state open --limit 20
```

If the dry-run reports changed rows (for example new roadmap rows without `issue_number`), run the same command without `--dry-run` before selecting the active issue.

## 2. Issue Selection

Run these while choosing the active issue:

```bash
gh issue view <issue_number>
```

Then post the `in progress` state comment from `references/issue-state-machine.md`.

## 3. Planning

Run these before editing code:

```bash
rg -n "<symbol_or_keyword>" crates examples docs .github
git branch --show-current
```

Then classify the issue with `references/issue-test-matrix.md` so the later validation scope is fixed before coding.
This same issue type also sets the default version bump in `references/versioning-policy.md`.

If a new issue branch is needed, create or switch to it before editing.

## 4. Development Loop

Use this as the default inner loop:

```bash
cargo fmt --all
cargo test -p oris-runtime <targeted_test_or_module>
```

Repeat the loop after meaningful code changes until the targeted fix or feature is stable.

## 5. Pre-Release Validation

Run these in order before any publish attempt:

```bash
cargo fmt --all -- --check
cargo build --verbose --all --release --all-features
cargo test --release --all-features
```

After the baseline, run the additional required commands from `references/issue-test-matrix.md` for the chosen issue type. Then add any subsystem-specific regression commands from `references/validation-and-release.md`.

## 6. Release Execution

Run these in order:

```bash
cargo publish -p oris-runtime --all-features --dry-run
cargo publish -p oris-runtime --all-features
```

Do not run the real publish until the dry-run succeeds.

## 7. Release Finalization

Run these after a successful publish:

```bash
git push
```

If the workflow uses a tag, create and push `v<version>` before opening the PR.

Post the `released` status comment while the PR is pending review/merge:

```bash
gh issue comment <issue_number> --body "Status: released

Released version:
- oris-runtime v<version>"
```

Determine the repository default branch, then open a PR targeting it:

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

Enable auto-merge so the PR merges automatically once branch-protection checks pass:

```bash
gh pr merge --auto --squash
```

> **Note:** `--auto` requires the repository to have auto-merge enabled and any required status checks configured. If the command fails with "auto-merge is not allowed", run `gh pr merge --squash` manually after review instead.

Close the issue only **after** the PR has merged:

```bash
gh issue close <issue_number> --comment "Completed and released in oris-runtime v<version>. Merged via PR #<pr_number>."
```

## 8. Failure Path

If any command in the release path fails:

- Stop the checklist immediately.
- Follow `references/publish-failure.md`.
- Do not continue into later command groups.
