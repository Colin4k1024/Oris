# Oris Evolution Stability and Safety Model


> **Implementation Status: In Progress** 🔄
Source: migrated from internal planning notes.

Last synced: March 3, 2026

## Current Implementation Snapshot (March 3, 2026)

The current `crates/oris-governor` crate implements a narrow default policy:

- promotion after a configurable success threshold
- blast radius caps using changed file and line counts
- replay-failure-based revocation
- time-window mutation rate limiting with a cooldown derived from the active window
- confidence regression checks using decayed confidence vs. historical peak
- a promotion cooling window
- `Candidate`, `Promoted`, and `Revoked` decisions
- experimental re-export through `oris-runtime::governor` behind `governor-experimental`

Not yet implemented in the checked-in crate:

- concurrent mutation rate limits
- richer confidence history beyond the current peak-and-last-update inputs
- automatic rollback orchestration
- global pause or freeze controls

## Related Documents

- [evolution.md](evolution.md)
- [economics.md](economics.md)
- [network.md](network.md)

## 1. Purpose

The evolution governor protects Oris from instability caused by uncontrolled
self-modification. It acts as:

- safety controller
- evolutionary immune system
- stability regulator

## 2. Governance Principles

1. Evolution must be slow.
2. Large mutations require stronger evidence.
3. Diversity must be preserved.
4. Regression must be reversible.
5. No strategy is permanently trusted.

## 3. Governor Position in Architecture

```text
Execution
-> Validation
-> Governor
-> Solidify
```

Governor approval is required before persistence.

## 4. Control Domains

### 4.1 Mutation Rate Control (Implemented Baseline)

Limit evolution velocity.

Example:

```text
max_capsules_per_hour = N
```

### 4.2 Blast Radius Control

Measure impact scope.

```rust
struct BlastRadius {
    files_changed: usize,
    lines_changed: usize,
}
```

Large changes require stricter promotion thresholds.

### 4.3 Diversity Preservation

Avoid evolutionary monoculture.

```text
exploration_rate ~= 10-20%
```

### 4.4 Regression Detection

Continuously monitor:

- success rate decline
- latency increase
- validation failures

Automatic response:

```text
revoke_gene(gene_id)
```

### 4.5 Confidence Decay (Implemented Baseline)

All experience ages:

```text
confidence *= e^(-lambda * t)
```

### 4.6 Evolution Cooling

Cooldown after promotion.

Example:

```text
promotion_cooldown = 30 minutes
```

## 5. Gene Lifecycle Governance

```text
Candidate
-> Promoted
-> Revoked
-> Archived
```

State transitions are enforced by governor rules.

## 6. Quarantine System

External or risky assets enter quarantine and require:

- sandbox validation
- replay verification
- local success confirmation

## 7. Failure Containment

Failures remain localized through:

- blast radius enforcement
- staged rollout
- rollback capability
- replay verification

## 8. Anti-Hallucination Protection

The governor ignores:

- LLM explanations
- single-run success
- unverifiable claims

Only execution evidence counts.

## 9. Emergency Safeguards (Planned)

The governor may trigger:

- global evolution pause
- automatic rollback
- promotion freeze
- capsule invalidation

## 10. Observability Requirements

Metrics:

- mutation rate
- promotion ratio
- revoke frequency
- replay success rate
- system stability index

## 11. Stability Success Indicators

Healthy evolution shows:

- gradual improvement
- decreasing reasoning dependency
- stable execution performance
- controlled mutation rate

## 12. Non-Goals

The governor does not:

- block experimentation entirely
- guarantee optimal solutions
- replace validation systems

Its role is stability, not optimization.
