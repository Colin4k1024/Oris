# v1.0 Proposal-to-PR Baseline — Verification Report

**Issue:** #416 Proposal-to-PR baseline complete
**Parent Milestone:** v1.0 Trusted Improvement System (#369)
**Status:** Complete
**Date:** 2026-03-27

## Verification Summary

The proposal-to-PR baseline for v1.0 is verified as complete across all four areas specified in the issue.

## Deliverables

### 1. Proposals as First-Class Objects

**Requirement:** Proposals are first-class objects

**Evidence:**
- **Source:** `crates/oris-orchestrator/src/proposal_generator.rs` - `ProposalGenerator`
- **Evidence:** `crates/oris-orchestrator/src/evidence.rs` - evidence bundling for proposals
- **Contracts:** `crates/oris-agent-contract/` - `ProposalContract` trait
- **Mutation Proposals:** Autonomous mutation proposal contracts for bounded work (PR #323)

### 2. Review-Ready Branch/PR Artifacts

**Requirement:** Successful bounded work can be turned into review-ready branch or PR artifacts

**Evidence:**
- **Source:** `crates/oris-orchestrator/src/github_delivery.rs` - `GitHubPrDeliveryAdapter`
- **Pipeline:** `deliver(PrPayload)` with stages:
  1. Credential gate - ORIS_GITHUB_TOKEN verification
  2. PR creation - POST to GitHub API
  3. CI poll loop - polling until pass/fail/timeout
  4. Merge gate - squash-merge when CI passes
- **Ports:** `PrCreationPort`, `CiCheckPort`, `MergePort` traits for testability
- **Decision:** `AutonomousPrLaneDecision` with `pr_ready` status

### 3. Evidence and Provenance in Delivery Artifacts

**Requirement:** Each delivery artifact carries evidence and provenance

**Evidence:**
- **Source:** `crates/oris-orchestrator/src/evidence.rs` - evidence bundling
- **Evidence:** `crates/oris-intake/src/evidence.rs` - intake evidence structures
- **Provenance:** Evidence includes source, timestamp, confidence metrics
- **Bundle:** Evidence bundles attached to proposals and delivery artifacts

### 4. Review Outcomes Feed Future Trust

**Requirement:** Review outcomes feed future system trust

**Evidence:**
- **Confidence Lifecycle:** `crates/oris-evolution/src/confidence.rs` - review outcomes affect confidence
- **Demotion/Revocation:** Governor responds to replay failures and confidence regression
- **Learning:** `crates/oris-evokernel/src/core.rs` - supervised devloop learns from review

## Architecture

```
Proposal Generation → Evidence Bundling → PR Lane Evaluation
                                              ↓
                                    PR Ready → GitHub Delivery
                                              ↓
                                    CI Poll Loop
                                              ↓
                                    Merge Gate → Trust Update
```

## Key Components

| Component | File | Purpose |
|-----------|------|---------|
| ProposalGenerator | `proposal_generator.rs` | Generate proposals from candidates |
| Evidence | `evidence.rs` | Bundle evidence and provenance |
| GitHubPrDeliveryAdapter | `github_delivery.rs` | PR creation and CI polling |
| AutonomousPrLaneDecision | `github_delivery.rs` | Pre-delivery gate decision |
| ProposalContract | `oris-agent-contract/` | Proposal interface trait |

## Parent Milestone Exit Checklist

**Parent Milestone Exit Checklist:**
- [x] Proposal-to-PR baseline complete (this issue)