# Versioning Policy

## Default Semver Rule

- Use `patch` for issue-sized bug fixes, internal hardening, docs corrections, validation-only changes, and backward-compatible behavior fixes.
- Use `minor` for new backward-compatible public capability, new user-facing API surface, new feature flag that exposes new behavior, or meaningful example additions that expand supported workflows.
- Do not take a `major` version step automatically. If the issue breaks compatibility, stop and get an explicit user decision on the release plan.

## Type-Based Defaults

Start from the primary issue type chosen in `references/issue-test-matrix.md`:

| Issue type | Default bump |
|---|---|
| `docs-or-release` | `patch` |
| `bugfix` | `patch` |
| `feature` | `minor` |
| `persistence` | `patch` |
| `backend-config` | `patch` |
| `security` | `patch` |

Use this table as the default release class. Move upward only when the shipped impact exceeds the type default.

## Decision Table

Choose the bump by the highest-impact shipped change in the issue:

| Issue outcome | Bump |
|---|---|
| Fixes incorrect behavior without breaking callers | `patch` |
| Refactors internals with no public API or behavior break | `patch` |
| Tightens validation, tests, CI, docs, or release plumbing only | `patch` |
| Adds a new backward-compatible API, tool, feature, or workflow | `minor` |
| Adds a new optional capability behind a feature flag | `minor` |
| Changes or removes an existing public contract, defaults, or compatibility expectation | stop and escalate |

## Escalation Triggers

Stop and ask the user before changing the version when any of these are true:

- Existing public API signatures are removed or changed incompatibly.
- Existing behavior changes in a way that can break current users.
- A feature flag is removed, renamed, or made mandatory.
- A data format, persistence contract, or runtime expectation changes incompatibly.

## Practical Workflow

1. Start from the issue type default in the table above.
2. Read the issue and identify the highest-impact shipped outcome.
3. If the shipped impact fits the default, keep that bump.
4. If the shipped impact is higher but still backward-compatible, raise the bump from `patch` to `minor` and record why.
5. If the result is `patch` or `minor`, update `crates/oris-runtime/Cargo.toml`.
6. If the result is "stop and escalate", do not edit the version yet. Ask the user whether to plan a breaking release.

## Consistency Rule

- Keep the chosen version identical across `crates/oris-runtime/Cargo.toml`, `RELEASE_v<version>.md`, git tag `v<version>`, and the final issue closeout comment.
- If the final bump is higher than the issue type default, mention that reason in the release note or working notes.
