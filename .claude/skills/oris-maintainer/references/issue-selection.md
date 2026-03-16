# Issue Selection

## Default Priority Order

Choose the next issue using this order:

1. An issue explicitly named by the user.
2. An issue already tied to the active release target:
   - current git branch name
   - active GitHub milestone, if used
   - explicit release label, if used
3. Open issues that represent breakage or release risk:
   - bug
   - regression
   - security
   - publish
4. Open feature or enhancement issues.
5. Oldest remaining open issue by creation time.

## Skip Rules

- Skip issues labeled or described as blocked, waiting, or duplicate unless the user explicitly asks for them.
- Skip issues whose scope is unclear until the issue body or comments are clarified.
- Skip issues that require unavailable credentials, infrastructure, or external services when another ready issue exists.

## Tie-Breaking

- Prefer the issue that touches the smallest surface area and can become a single clean release.
- Prefer issues that keep the current branch relevant instead of forcing an unrelated context switch.
- If two issues are equally valid, choose the older one.

## Practical Commands

Use simple GitHub CLI reads to rank candidates:

```bash
gh issue list --state open --limit 20
gh issue view <issue_number>
```

If labels or milestones are inconsistent, fall back to the oldest clearly actionable open issue.
