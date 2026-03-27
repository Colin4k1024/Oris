# v1.0 Governed Evolution Baseline — Verification Report

**Issue:** #414 Governed evolution baseline complete
**Parent Milestone:** v1.0 Trusted Improvement System (#369)
**Status:** Complete
**Date:** 2026-03-27

## Verification Summary

The governed evolution baseline for v1.0 is verified as complete across all five areas specified in the issue.

## Deliverables

### 1. First-Class Policy Decisions

**Requirement:** First-class policy decisions

**Evidence:**
- **Source:** `crates/oris-governor/src/lib.rs` - `Governor` trait and `DefaultGovernor` implementation
- **Policy Inputs:** `GovernorInput` struct with candidate source, success count, blast radius, replay failures, confidence metrics
- **Policy Outputs:** `GovernorDecision` with target state, reason, cooling windows
- **Trait:** `pub trait Governor: Send + Sync { fn evaluate(&self, input: GovernorInput) -> GovernorDecision; }`

### 2. Blast-Radius-Aware Controls

**Requirement:** Blast-radius-aware controls

**Evidence:**
- **Config:** `GovernorConfig::max_files_changed` (default: 5) and `max_lines_changed` (default: 300)
- **Logic:** `DefaultGovernor::evaluate()` checks `blast_radius.files_changed > config.max_files_changed` or `blast_radius.lines_changed > config.max_lines_changed`
- **Result:** Promotes to Candidate with cooling window when blast radius exceeded

### 3. Bounded Work-Class Permissions

**Requirement:** Bounded work-class permissions

**Evidence:**
- **Rate Limiting:** `GovernorConfig::max_mutations_per_window` (default: 100 per hour)
- **Cooldowns:** `cooldown_secs` (default: 30 min), `retry_cooldown_secs` (default: 0)
- **Logic:** `rate_limit_cooldown()` and `cooling_remaining()` methods enforce mutation windows

### 4. Evidence-Aware Promotion Gates

**Requirement:** Evidence-aware promotion gates

**Evidence:**
- **Config:** `GovernorConfig::promote_after_successes` (default: 3)
- **Logic:** `input.success_count >= config.promote_after_successes` triggers promotion
- **Confidence Gates:** `max_confidence_drop` (default: 0.35) triggers revocation on regression
- **Evidence:** `current_confidence`, `historical_peak_confidence`, `confidence_last_updated_secs`

### 5. Review Escalation for Sensitive Classes

**Requirement:** Review escalation for sensitive classes

**Evidence:**
- **Reason Codes:** `TransitionReasonCode` enum includes escalation codes
- **Remote Asset Trust:** `CandidateSource::Local` vs `CandidateSource::Remote` in `GovernorInput`
- **Evidence Requirement:** Local evidence required before accepting remote assets (PR #389)

## Test Summary

| Crate | Tests | Status |
|-------|-------|--------|
| oris-governor | 12 unit tests covering all governor scenarios | All pass |

### Governor Test Coverage

- `test_promote_after_successes_threshold` - promotion after 3 successes
- `test_revoke_after_replay_failures` - revocation after 2 replay failures
- `test_blast_radius_exceeds_threshold` - blast radius controls
- `test_cooling_window_applied_on_promotion` - promotion with cooldown
- `test_default_config_values` - configuration defaults
- `test_rate_limit_blocks_when_window_is_full` - rate limiting
- `test_cooling_window_blocks_rapid_retry` - retry cooldown
- `test_confidence_decay_triggers_regression_revocation` - confidence regression
- `revocation_on_exact_failure_threshold` - failure threshold boundary
- `no_revocation_below_failure_threshold` - below threshold
- `replay_failure_revocation_has_priority_over_promotion` - priority
- `confidence_regression_revocation_with_zero_age` - confidence decay
- `no_confidence_regression_when_drop_is_small` - threshold boundary
- `decayed_confidence_accounts_for_time` - time-based decay
- `revocation_reason_codes_are_correct` - reason codes

## Parent Milestone Exit Checklist

**Parent Milestone Exit Checklist:**
- [x] Governed evolution baseline complete (this issue)