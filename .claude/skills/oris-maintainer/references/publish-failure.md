# Publish Failure

## Hard Stop Rule

- Any failure in version bump validation, release note preparation, dry-run publish, real publish, tag alignment, or push is a stop condition.
- Do not close the issue.
- Do not start the next issue.
- Do not silently bump to another version just to get an upload through.

## Failure Handling Sequence

1. Capture the exact failing command and the exact error.
2. Determine which phase failed:
   - before `cargo publish --dry-run`
   - during `cargo publish --dry-run`
   - during real `cargo publish`
   - after publish, during tag or push
3. Preserve the repository state needed for recovery:
   - keep modified release notes
   - keep version changes
   - avoid unrelated cleanup that obscures the failure
4. Post a short status comment on the issue describing:
   - which release step failed
   - whether the crate was actually published
   - what is blocked next
5. Stop and resolve the release state before continuing the loop.

## Special Cases

### Dry-Run Failure

- Keep the issue open.
- Fix packaging, manifest, or validation problems first.
- Do not create or push a release tag.

### Real Publish Failure

- Do not retry blindly.
- First determine whether crates.io accepted the version partially or completely.
- If the publish result is ambiguous, treat the release as unknown and stop until the actual crates.io state is confirmed.
- If the version is now taken, do not overwrite the release story. Update the issue and release notes to reflect the real state, then decide the next version deliberately.

### Post-Publish Tag or Push Failure

- Treat the crate as released if `cargo publish` succeeded.
- Keep the issue open until the git tag, branch push, and issue closeout record are made consistent with the published version.
- Fix repository metadata drift before moving on.

## Status Comment Pattern

Use a factual blocker comment when the issue cannot be closed:

```bash
gh issue comment <issue_number> --body "Release blocked for oris-runtime v<version>.

Failed step:
- <exact failing command>

Current state:
- crate published: <yes/no/unknown>
- next action: <specific unblock step>
"
```
