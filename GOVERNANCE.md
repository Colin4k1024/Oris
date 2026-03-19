# Governance

This document describes how the Oris project is maintained and how project decisions are made.

## Project roles

- Maintainers:
  - Review and merge pull requests.
  - Triage issues and security reports.
  - Manage releases and roadmap priorities.
- Contributors:
  - Propose changes through issues and pull requests.
  - Follow [CONTRIBUTING.md](CONTRIBUTING.md) and [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).

## Decision making

- Prefer rough consensus through public discussion in issues/PRs.
- For conflicting proposals, maintainers make the final call based on:
  - correctness and reliability,
  - API stability and migration cost,
  - long-term maintainability.

## Architecture Review Board

The Architecture Review Board (ARB) provides structured review for major architectural changes that can affect protocol compatibility, persistence guarantees, or security posture.

### Charter

- Purpose: review and record decisions for changes that materially affect Oris architecture.
- Scope: ARB sign-off is required for new persistence backends, changes to the evolution network or runtime API contracts, new gossip or transport layers, and other major architecture changes that can alter downstream integration behavior.
- Composition: the ARB is made up of rotating maintainer seats so architectural review is shared instead of concentrated in a single maintainer.

### Voting

- Non-breaking architectural changes require a simple majority of recorded ARB votes.
- Protocol or contract changes require a 2/3 supermajority of recorded ARB votes.
- Security-sensitive proposals may be vetoed by any ARB reviewer when the proposal introduces unresolved security risk. Vetoes must include the blocking concern in the recorded decision.

### Major Architecture Change Process

- The author opens an RFC discussion using `.github/DISCUSSION_TEMPLATE/rfc.md`.
- The RFC discussion remains open for a minimum 2-week comment period before a decision is recorded.
- The ARB reviews the proposal asynchronously and records the vote outcome in the discussion.
- Approved proposals result in an implementation issue or pull request scoped to the accepted design.
- Rejected proposals remain documented in the discussion with the reason for rejection.

## Release policy

- `main` is the active development branch.
- Releases are tagged and published through CI workflows.
- Breaking behavior changes must include migration notes in PR descriptions and release notes.

## Change categories

- Patch changes:
  - bug fixes,
  - documentation and non-breaking internal improvements.
- Minor changes:
  - additive APIs/features with backward compatibility.
- Breaking changes:
  - contract or behavior changes requiring user action.
  - should be announced clearly before release.

## Security and responsible disclosure

- Security issues follow [SECURITY.md](SECURITY.md).
- Private reporting is required for vulnerabilities until a fix is available.

## Communication channels

- Issues: bug reports, feature requests, support questions.
- Pull requests: implementation and design review.

## Maintainer expectations

- Act consistently with project policies.
- Keep review feedback technical, respectful, and actionable.
- Prioritize reproducible reports and tests for behavior changes.

## Policy updates

Governance policies may evolve as the project grows. Changes are made by pull request in this repository.
