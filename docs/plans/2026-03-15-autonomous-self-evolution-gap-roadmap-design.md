# Autonomous Self-Evolution Gap Roadmap Design

Date: 2026-03-15
Status: Draft for review
Scope: self-evolution autonomy gap and feature split only

## Context

The checked-in Oris repository has crossed the supervised closed-loop boundary for bounded self-evolution work.

Current source-of-truth status lives in:

- [docs/evokernel/current-project-status.md](/Users/jiafan/Desktop/poc/Oris/docs/evokernel/current-project-status.md)
- [docs/evokernel/self-evolution-acceptance-checklist.md](/Users/jiafan/Desktop/poc/Oris/docs/evokernel/self-evolution-acceptance-checklist.md)
- [docs/evokernel/implementation-roadmap.md](/Users/jiafan/Desktop/poc/Oris/docs/evokernel/implementation-roadmap.md)

The accurate current statement is:

> Oris supports a supervised closed-loop self-evolution path with bounded acceptance gating.

It is still not accurate to describe the repository as a fully autonomous self-improving development and release system.

## Goal

Define the gap between the current supervised self-evolution boundary and a future agent that can independently discover, plan, execute, validate, and deliver bounded software improvements.

Define the feature split needed to close that gap without overclaiming autonomy or collapsing the work into one monolithic track.

## Non-Goals

This roadmap does not include:

- unconstrained codebase-wide autonomous mutation
- removal of fail-closed approval or governor boundaries
- autonomous release for high-risk change classes
- UI-first planning detached from executable backend contracts
- generic platform work that does not move the self-evolution autonomy boundary

## Current Boundary

The repository already provides:

- replay-driven mutation capture and later reuse
- bounded candidate selection when explicit metadata is supplied by a caller
- machine-readable mutation proposal contracts for bounded work
- replay-assisted supervised execution with fail-closed fallback semantics
- acceptance gating over closed-loop evidence
- bounded branch and pull-request artifact preparation without autonomous merge or release
- quarantined remote asset reuse with local validation before promotion

This means the system already has learning, replay, bounded execution, and auditability.

The system does not yet have a full autonomous operating loop.

## Gap Statement

The shortest correct summary of the gap is:

> Oris already knows how to learn and safely reuse bounded solutions, but it does not yet know how to independently run the full software-improvement business process.

The missing capability is not one thing. It is a stack of missing autonomy layers.

## Missing Autonomy Layers

### 1. Autonomous intake

Current state:

- issue candidates must be supplied by an external caller or bounded test harness

Missing capability:

- continuous discovery from CI failures, runtime alerts, regressions, backlog signals, and external feedback

Why it matters:

- without autonomous intake, the system cannot decide what to work on by itself

### 2. Autonomous task planning

Current state:

- the system can select within a bounded path, but does not yet perform general autonomous planning across broader work classes

Missing capability:

- task classification, feasibility scoring, blast-radius estimation, execution budgeting, and priority ranking

Why it matters:

- without planning, autonomous intake only creates noise

### 3. Autonomous proposal generation

Current state:

- proposal contracts are structured and auditable, but still rely on explicit supervised entry points

Missing capability:

- self-generated bounded proposals with machine-readable evidence expectations and rollback constraints

Why it matters:

- the agent must be able to turn a discovered problem into an executable and governable unit of work

### 4. Broader semantic generalization

Current state:

- replay works for the currently implemented equivalence boundary and normalized signal matching

Missing capability:

- stronger task-class abstraction and semantic equivalence beyond narrow normalized patterns

Why it matters:

- without this layer, the system only appears to improve on exact or near-exact repeats

### 5. Continuous confidence control

Current state:

- confidence lifecycle primitives exist, but the autonomy boundary still lacks full background revalidation and automatic demotion behavior

Missing capability:

- shadow-mode revalidation, confidence decay, demotion, revocation propagation, and stale-asset quarantine

Why it matters:

- autonomous systems require active forgetting and requalification, not only successful learning

### 6. Autonomous delivery

Current state:

- the system can prepare branch and PR artifacts, but does not yet independently operate a safe PR lane or release lane

Missing capability:

- low-risk autonomous PR creation, evidence packaging, check watching, merge gating, release gating, and publish gating

Why it matters:

- autonomous learning without autonomous delivery is still only a supervised assistant

### 7. Operational governance at autonomy scale

Current state:

- fail-closed behavior and basic governor boundaries exist

Missing capability:

- risk-tiered policy, permission boundaries by task class, environment separation, kill switches, rate limits, and incident rollback playbooks

Why it matters:

- the main failure mode of autonomy is not inability to act but inability to stop acting safely

## Target Statement

The future target should not be phrased as generic full autonomy.

The correct target statement is:

> Oris can independently operate a bounded autonomous self-evolution lane for explicitly allowed low-risk task classes under fail-closed governance.

This target is intentionally narrower than unconstrained autonomous software development.

## Recommended Feature Split

The autonomy gap should be split into six feature streams.

### Stream A. `autonomous-intake-experimental`

Goal:

- allow the system to discover candidate work without caller-supplied issue metadata

Scope:

- CI failure intake
- test regression intake
- lint and compile regression intake
- runtime panic and incident intake
- deduplication and stable reason-code classification

Exit criteria:

- machine-readable discovered candidates are stable and deduplicated
- unsupported or ambiguous candidates fail closed
- no proposal or mutation execution is triggered automatically in this stream alone

### Stream B. `task-planning-experimental`

Goal:

- convert discovered candidates into bounded executable plans

Scope:

- task-class classification
- feasibility scoring
- blast-radius estimation
- evidence template generation
- policy-based denial for high-risk tasks

Exit criteria:

- identical issue classes produce stable planning reason codes
- high-risk or low-confidence plans are rejected before proposal generation
- plan outputs are machine-readable and auditable

### Stream C. `autonomous-proposal-experimental`

Goal:

- allow the agent to generate bounded mutation proposals without a human caller constructing them first

Scope:

- proposal contract generation
- expected evidence declaration
- validation budget declaration
- rollback and denial conditions
- proposal quality checks

Exit criteria:

- proposals remain bounded by policy and file scope
- malformed or weak-evidence proposals fail closed
- proposal contracts can feed existing supervised execution paths without format drift

### Stream D. `continuous-confidence-experimental`

Goal:

- continuously govern evolution assets so autonomous reuse remains trustworthy over time

Scope:

- confidence decay
- background revalidation
- demotion and quarantine re-entry
- asset revocation propagation
- stale replay suppression

Exit criteria:

- stale or repeatedly failing assets lose eligibility automatically
- replay hit rate and correctness remain aligned with event evidence
- demotion and quarantine transitions are externally observable and auditable

### Stream E. `autonomous-pr-experimental`

Goal:

- enable a bounded low-risk autonomous pull-request lane

Scope:

- branch creation
- patch application and proof packaging
- automatic PR creation for approved task classes
- CI/watch integration
- reviewer-facing evidence summaries

Exit criteria:

- only explicitly allowed low-risk task classes can auto-open PRs
- failed checks or missing evidence stop the lane immediately
- PR payloads include enough evidence for deterministic audit

### Stream F. `autonomous-release-experimental`

Goal:

- enable bounded autonomous merge and release for the narrowest safe class of changes

Scope:

- merge gate
- release gate
- publish gate
- rollback hooks
- policy kill-switches and incident stop conditions

Exit criteria:

- merge or publish never occurs without policy-qualified evidence
- release automation is restricted to the narrowest safe task classes
- any gate drift or missing evidence fails closed before publish

## Recommended Order

The streams should be delivered in the following order:

1. `autonomous-intake-experimental`
2. `task-planning-experimental`
3. `autonomous-proposal-experimental`
4. `continuous-confidence-experimental`
5. `autonomous-pr-experimental`
6. `autonomous-release-experimental`

Rationale:

- intake without planning creates unmanaged noise
- planning without proposal generation cannot produce executable work
- proposal generation without confidence control creates asset pollution
- PR automation before confidence control and planning stability would amplify mistakes
- release automation should be the final stage because it has the highest blast radius

## Suggested Phase Model

### Phase 1. Autonomous Discovery

Includes:

- Stream A

Success statement:

- the system can independently discover and classify bounded candidate work, but not yet execute it autonomously

### Phase 2. Autonomous Planning

Includes:

- Stream B
- Stream C

Success statement:

- the system can independently discover a bounded problem and convert it into an auditable executable proposal

### Phase 3. Autonomous Learning Stability

Includes:

- Stream D

Success statement:

- autonomous reuse becomes continuously governed rather than relying only on static confidence assumptions

### Phase 4. Autonomous Delivery

Includes:

- Stream E

Success statement:

- the system can open bounded low-risk pull requests on its own with full evidence packaging

### Phase 5. Bounded Autonomous Release

Includes:

- Stream F

Success statement:

- the system can merge or release only within narrowly approved low-risk lanes under fail-closed governance

## Architectural Guidance

This work should extend the current architecture rather than replacing it.

Primary integration points:

- [crates/oris-evokernel/src/core.rs](/Users/jiafan/Desktop/poc/Oris/crates/oris-evokernel/src/core.rs)
- [crates/oris-orchestrator/src/autonomous_loop.rs](/Users/jiafan/Desktop/poc/Oris/crates/oris-orchestrator/src/autonomous_loop.rs)
- [docs/evokernel/self-evolution-acceptance-checklist.md](/Users/jiafan/Desktop/poc/Oris/docs/evokernel/self-evolution-acceptance-checklist.md)

Rules:

- keep every stage machine-readable
- preserve fail-closed semantics across all new autonomy lanes
- do not widen task class scope and delivery authority in the same issue
- require explicit evidence contracts before any new automatic action is allowed

## Acceptance Philosophy

The autonomy track should be accepted stage by stage, not by aspiration.

For each stream, the repository should only advance the public product statement after:

- a bounded capability exists in checked-in code
- the corresponding regression gate exists
- fail-closed behavior is covered
- policy boundaries are externally inspectable

## Definition Of “Agent Can Independently Evolve”

For Oris, the claim should become valid only when all of the following are true:

1. the system can independently discover and triage bounded issues
2. the system can independently generate bounded executable proposals with explicit evidence requirements
3. the system can execute, validate, evaluate, and learn from those proposals in a replay-aware path
4. the system can autonomously open bounded low-risk PRs with complete audit evidence
5. the system can merge or release only inside narrowly approved low-risk lanes with fail-closed governance

Until these conditions are met, the repository should continue to describe itself as supervised, bounded, and auditable self-evolution rather than fully autonomous self-improvement.