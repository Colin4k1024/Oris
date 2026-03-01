# Release Notes

## Purpose

- Create one release note file per published crate version.
- Name the file `RELEASE_v<version>.md` at the repository root.
- Base the content on the completed issue set for that release. For the default workflow, that is the single issue just completed.

## Required Sections

Use this shape:

```md
# v<version> - <short release title>

<one-sentence release summary for oris-runtime>

## What's in this release

- <user-facing shipped change>
- <secondary shipped change, if any>

## Validation

- cargo fmt --all -- --check
- <targeted test commands actually run>
- cargo publish -p oris-runtime --all-features --dry-run
- cargo publish -p oris-runtime --all-features

## Links

- Crate: https://crates.io/crates/oris-runtime
- Docs: https://docs.rs/oris-runtime
- Repo: https://github.com/Colin4k1024/Oris
```

## Drafting Rules

- Keep the title specific to the shipped capability, not the internal issue title.
- Prefer externally visible behavior in the bullet list: new API, bug fix, feature flag, migration safety, example, or docs change that users will notice.
- Include only commands actually executed in the `Validation` section.
- If publish has not happened yet, draft the file before publish, then update the validation block after the real publish succeeds.

## Issue-to-Release Mapping

- Read the issue with `gh issue view <issue_number>` and extract the shippable outcome, not the implementation detail.
- If multiple issues are intentionally bundled into one release, group them into one `RELEASE_v<version>.md` and mention the dominant theme in the title.
- Keep the version number, tag, release note filename, and issue closeout comment identical.
