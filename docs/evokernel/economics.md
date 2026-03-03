# Oris Evolution Economy and Anti-Collapse Mechanism


> **Implementation Status: In Progress** 🔄
Source: https://www.notion.so/317e8a70eec5804791a6c1b843a08464

Last synced: March 3, 2026

## Current Implementation Snapshot (March 3, 2026)

The current `crates/oris-economics` crate is an experimental local ledger scaffold:

- `EvuAccount`, `ReputationRecord`, `StakePolicy`, and `ValidationSettlement`
- `EconomicsSignal` plus `EvuLedger` helpers for:
  publish stake reservation, remote reuse settlement, validator divergence penalties,
  selector reputation bias, and governor-facing signal snapshots
- experimental re-export through `oris-runtime::economics`

Currently wired into the checked-in EvoKernel flow:

- publish gating for remote-facing asset export via EVU stake reservation
- remote replay success/failure settlement for the recorded publisher
- reputation-biased replay candidate ordering as a secondary tie-breaker

Still not implemented:

- validator staking enforcement
- publish-window rate limits

## 1. Purpose

The evolution economy defines how evolutionary intelligence is:

- incentivized
- validated
- distributed
- protected from collapse

Without an economic control layer, self-evolving networks degrade through spam,
poisoned strategies, or uncontrolled optimization loops.

## 2. Core Problem

Distributed evolution introduces:

- unlimited asset publishing
- fake success reporting
- evolution farming
- strategy monopolization
- malicious intelligence injection

Pure technical validation is insufficient at network scale.

## 3. Economic Design Principles

### 3.1 Evolution Has Cost

Publishing intelligence must require commitment.

### 3.2 Verification Has Reward

Nodes contributing validated improvements should gain influence.

### 3.3 Failure Has Penalty

Incorrect or harmful evolution must incur measurable loss.

### 3.4 Intelligence Emerges From Selection Pressure

Scarcity and validation create fitness.

## 4. Evolution Value Units (EVU)

```text
EVU - Evolution Value Unit
```

EVU measures verified contribution to network intelligence. It is not required
to be financial; it is a trust-weighted accounting unit.

## 5. Evolution Lifecycle Economics (Target)

- Stage 1, local creation: compute and validation cost, no EVU issuance.
- Stage 2, publication stake: publishing requires a base stake.
- Stage 3, network validation: reuse success rewards, failure burns stake.
- Stage 4, promotion: sufficiently reused assets become network-promoted assets.

## 6. Validator Role (Planned)

Validators:

- reproduce execution
- confirm validation success
- report outcomes

Validators also stake EVU to participate.

Incentives:

- correct validation -> reward
- incorrect validation -> penalty

## 7. Reputation System

```rust
struct Reputation {
    publish_success_rate: f32,
    validator_accuracy: f32,
    reuse_impact: u64,
}
```

Reputation affects:

- selection weighting
- trust priority
- propagation speed

## 8. Anti-Spam Mechanisms (Target)

- publication cost
- promotion threshold requiring independent confirmations
- rate limiting via `max_publish_per_window`

## 9. Anti-Monoculture Protection

Economic mitigation:

- diminishing rewards for repeated identical strategies
- diversity bonus for novel successful solutions

Example:

```text
reward proportional to novelty_score
```

## 10. Confidence Market Dynamics (Planned)

Positive reuse:

- increases EVU
- increases propagation likelihood

Negative reuse:

- reduces confidence
- triggers governor review

Confidence acts as evolutionary currency.

## 11. Revocation Economics (Planned)

When regression occurs:

```text
asset -> revoked
```

Consequences:

- publisher reputation decreases
- validator rewards reverse
- future influence is reduced

## 12. Evolution Inflation Control (Planned)

Controls:

- confidence decay over time
- archival of inactive assets
- storage pruning policies

This prevents intelligence inflation.

## 13. Network Health Metrics

- promotion ratio
- revoke rate
- reuse success rate
- diversity index
- EVU circulation rate

Healthy systems show gradual growth, not spikes.

## 14. Sybil Resistance (Target)

Strategies:

- stake requirement
- execution proof requirement
- reputation accumulation delay
- reuse-based trust weighting

Influence cannot be created instantly.

## 15. Evolution Market (Future Extension)

Potential extensions:

- shared enterprise evolution pools
- strategy marketplaces
- organizational intelligence exchange
- capability licensing

## 16. Failure Modes Prevented

The economy layer protects against:

- evolution spam collapse
- malicious asset flooding
- false validation consensus
- runaway mutation loops
- network intelligence degradation

## 17. Integration With Governor

Division of labor:

| Function | Governor | Economy |
| --- | --- | --- |
| Safety | yes |  |
| Stability | yes |  |
| Incentive |  | yes |
| Trust Scaling |  | yes |
| Network Sustainability |  | yes |

Both layers are required.

## 18. Implementation Guidance

Initial implementations may use:

- internal scoring only
- non-financial EVU tracking
- local reputation tables

Tokenization is optional.

## 19. Long-Term Vision

```text
verified execution
-> rewarded contribution
-> trusted intelligence
-> scalable evolution
```

## 20. Non-Goals

This system does not:

- introduce speculation markets
- require cryptocurrency
- replace governance controls
- monetize intelligence prematurely

Primary goal: network survival.
