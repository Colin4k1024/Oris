# Security Policy

## Supported versions

Security fixes are prioritized for the active development line:

| Version | Supported |
| --- | --- |
| `main` | Yes |
| Latest `0.1.x` | Yes |
| Older versions | Best effort |

## Reporting a vulnerability

Please do not report security vulnerabilities in public GitHub issues.

Use one of the following:

1. Preferred: GitHub Security Advisory (private vulnerability report for this repository).
2. Fallback: contact the maintainers at security@oris-project.example.

When reporting, include:

- Affected component(s) and version/commit.
- Reproduction steps or proof of concept.
- Impact assessment (confidentiality/integrity/availability).
- Any suggested remediation.

## What to expect

- Acknowledgment target: within 48 hours.
- Initial triage target: within 7 days.
- We may ask for additional details to reproduce.
- We coordinate disclosure timing and fix release before public details.
- Our target for fix and coordinated disclosure is within 90 days, or sooner for critical severity issues.

## Disclosure policy

- We follow responsible disclosure.
- After a fix is available, maintainers may publish an advisory and remediation guidance.

## Bounty scope

The Oris project uses a recognition-based bounty model for open source security research. Reports with a working proof of concept and clear product impact are prioritized highest.

### In scope: high priority

- Sandbox escape that breaks `crates/oris-sandbox/` process isolation or allows mutation execution to escape the intended boundary.
- Evolution state deserialization flaws, including remote code execution or equivalent compromise via a malicious `EvolutionEnvelope` or related network payload.
- Persistence encryption bypass that defeats at-rest protection for Oris-managed state or checkpoint data.
- Authentication bypass that overrides `ExecutionApiAuthConfig` checks or grants unauthorized access to runtime APIs.

### In scope: standard priority

- SQL injection in runtime repositories or persistence paths maintained by Oris.
- HMAC bypass in intake webhook signature verification.
- Privilege escalation in the worker lease system or related task-claim flows.

### Out of scope

- Theoretical issues without a working proof of concept.
- Vulnerabilities that exist only in third-party dependencies without an Oris-specific amplification path or exploitable integration gap.
- Best-practice suggestions that do not demonstrate a security impact.

## Coordinated disclosure process

1. Report the issue through GitHub Security Advisories or by emailing security@oris-project.example.
2. Maintainers acknowledge receipt within 48 hours.
3. Maintainers complete initial triage within 7 days, including severity assessment and reproduction status.
4. Maintainers work with the reporter on remediation and target coordinated disclosure within 90 days, or sooner for critical issues.

## Recognition tiers

| Severity | Recognition |
| --- | --- |
| Critical (CVSS 9+) | Hall of Fame entry and featured blog post |
| High (CVSS 7-8.9) | Hall of Fame entry |
| Medium / Low | Acknowledgment in release notes |

## Scope notes

For integrations with third-party providers (LLM APIs, external tools, storage backends), please include provider and configuration details because security behavior may depend on deployment choices.
