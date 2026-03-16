# Issue Closeout

## Closeout Policy

- Close an issue only after the PR targeting the default branch has successfully merged and the crate publish succeeded.
- Do not close an issue when auto-merge is still pending. Wait for the merge event, then close.
- Use one released crate version per completed issue by default.
- Keep the issue comment short and factual: shipped change, validation run, released version, and merged PR number.

## Comment Template

Use this pattern when the issue should remain open briefly after release confirmation:

```bash
gh issue comment <issue_number> --body "Shipped in oris-runtime v<version>.

Summary:
- <one-line behavior change>

Validation:
- cargo fmt --all -- --check
- <targeted test commands>
- <release dry-run or publish confirmation>
"
```

## Close Template

Use this pattern after the PR has merged:

```bash
gh issue close <issue_number> --comment "Completed and released in oris-runtime v<version>. Merged via PR #<pr_number>.

Validation:
- cargo fmt --all -- --check
- <targeted test commands>
- cargo publish -p oris-runtime --all-features
"
```

## Notes

- Replace placeholders with exact commands actually run. Do not claim coverage you did not execute.
- If publish was blocked, post a status comment instead of closing the issue.
- If the issue was completed without a release by explicit user instruction, say that clearly in the final comment and do not use the default release wording.
