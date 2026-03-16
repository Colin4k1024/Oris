# Issue State Machine

## States

Use this open-issue state flow:

`todo -> in progress -> blocked -> in progress -> released -> closed`

Notes:

- `blocked` is temporary. Return to `in progress` once the blocker is cleared.
- `released` means the crate version has shipped, but the issue is still open long enough to record the release cleanly.
- `closed` is the terminal state and should happen only after release success or explicit user override.

## Representation Rule

- Prefer a dedicated status label if the repository already has one, for example:
  - `status:todo`
  - `status:in-progress`
  - `status:blocked`
  - `status:released`
- If those labels do not exist, do not stop to create them. Use issue comments as the canonical state record.
- If labels exist, keep only one status label at a time.

## Transition Rules

### Move to `in progress`

Do this immediately after choosing the issue as the active work item.

```bash
gh issue comment <issue_number> --body "Status: in progress

Plan:
- <one-line implementation target>
"
```

### Move to `blocked`

Do this when validation, release, or implementation is blocked by a missing prerequisite.

```bash
gh issue comment <issue_number> --body "Status: blocked

Blocker:
- <exact dependency, secret, service, or decision that is missing>

Next step:
- <specific unblock action>
"
```

### Move from `blocked` back to `in progress`

Do this once the blocker is cleared and active work resumes.

```bash
gh issue comment <issue_number> --body "Status: in progress

Blocker cleared:
- <what changed>
"
```

### Move to `released`

Do this after `cargo publish` succeeds and before the issue is closed.

```bash
gh issue comment <issue_number> --body "Status: released

Released version:
- oris-runtime v<version>
"
```

### Move to `closed`

Use the closeout command in `references/issue-closeout.md`. Do not close directly from `todo` or `blocked`.

## Practical Rule

- If labels are available, update the label and post the matching comment.
- If labels are not available, the comment alone is sufficient.
- Every status comment should be factual, short, and based only on work actually completed.
