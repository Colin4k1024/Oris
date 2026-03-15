# Autonomous Self-Evolution Issue Drafts

This document contains GitHub-ready draft issues derived from:

- [docs/plans/2026-03-15-autonomous-self-evolution-gap-roadmap-design.md](/Users/jiafan/Desktop/poc/Oris/docs/plans/2026-03-15-autonomous-self-evolution-gap-roadmap-design.md)

Recommended labels for all issues:

- `type/feature`
- `priority/P1`
- `area/evolution`
- `plan`

Recommended execution order:

1. `AUTO-01`
2. `AUTO-02`
3. `AUTO-03`
4. `AUTO-04`
5. `AUTO-05`
6. `AUTO-06`
7. `AUTO-07`

## Issue Draft 1 (`AUTO-01`)

### Title

`[EVO26-AUTO-01][P1] Autonomous Candidate Intake From CI and Runtime Signals`

### Body

## Why

Oris currently requires an external caller or bounded harness to supply self-evolution candidates. That is enough for supervised closed-loop self-evolution, but it is not enough for an agent that can independently decide what to work on.

The first missing autonomy layer is intake. The runtime needs a bounded way to discover and classify candidate work from CI failures, test regressions, lint or compile regressions, and runtime incidents without immediately widening into arbitrary autonomous work selection.

## Scope

- define a machine-readable autonomous intake contract for discovered candidates
- ingest CI failure, test regression, compile regression, lint regression, and panic or incident signals
- deduplicate candidates and normalize them into stable candidate classes
- emit stable reason codes for supported, unsupported, ambiguous, and denied candidates
- keep this issue limited to discovery and classification only

## Definition of Done

- the runtime can produce discovered candidate records without caller-supplied issue metadata
- duplicate signals collapse into stable candidate identities
- unsupported or ambiguous discovered candidates fail closed with explicit reason codes
- discovered candidate outputs are represented consistently across contracts, events, and regression tests

## Non-goals

- no task planning yet
- no autonomous mutation proposal generation yet
- no autonomous execution yet
- no PR or release automation

## Required Machine-Readable Outputs

- `discovered_candidate`
- `candidate_source`
- `candidate_class`
- `dedupe_key`
- `reason_code`
- `fail_closed`

## Minimum Validation

- `cargo test -p oris-evokernel --test evolution_lifecycle_regression autonomous_intake_ -- --nocapture`
- `cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental -- --nocapture`
- `cargo test --workspace`

---

## Issue Draft 2 (`AUTO-02`)

### Title

`[EVO26-AUTO-02][P1] Bounded Task Planning and Risk Scoring For Autonomous Intake`

### Body

## Why

Autonomous intake is not sufficient if discovered candidates cannot be converted into bounded executable plans. The runtime needs a planning layer that can classify task type, estimate feasibility, bound blast radius, and reject high-risk work before any proposal generation begins.

This is the first point where the system starts to make autonomous choices about whether work should proceed.

## Scope

- define a machine-readable planning contract for discovered candidates
- classify task type and assign a bounded execution class
- score feasibility, risk tier, and expected validation burden
- attach expected evidence templates and denial conditions
- reject high-risk, low-confidence, or unsupported work fail closed

## Definition of Done

- a discovered candidate can be transformed into an auditable plan record
- planning reason codes remain stable for equivalent candidate classes
- high-risk candidates are denied before proposal generation
- planning outputs remain machine-readable and consistent across contracts, events, and tests

## Non-goals

- no code mutation proposal generation yet
- no autonomous execution yet
- no broad open-ended planning across arbitrary repositories
- no policy bypass for convenience

## Required Machine-Readable Outputs

- `task_plan`
- `task_class`
- `risk_tier`
- `feasibility_score`
- `validation_budget`
- `reason_code`
- `denial_condition`

## Minimum Validation

- `cargo test -p oris-evokernel --test evolution_lifecycle_regression autonomous_planning_ -- --nocapture`
- `cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental -- --nocapture`
- `cargo test --workspace`

---

## Issue Draft 3 (`AUTO-03`)

### Title

`[EVO26-AUTO-03][P1] Autonomous Mutation Proposal Contracts For Bounded Work`

### Body

## Why

Once the runtime can discover and plan bounded work, it still needs a contract that turns a plan into an executable mutation proposal. Today proposal contracts are auditable, but the autonomy boundary still assumes a supervised caller entry point.

This issue moves proposal generation into the runtime while preserving the same fail-closed contract quality.

## Scope

- generate bounded mutation proposal contracts from approved plans
- declare target file scope, expected evidence, validation steps, rollback conditions, and policy constraints
- reject malformed, weak-evidence, or out-of-bounds proposals before execution
- keep proposal outputs compatible with existing supervised execution paths
- preserve stable proposal reason-code semantics

## Definition of Done

- an approved autonomous plan can produce a machine-readable mutation proposal
- weak or malformed proposals fail closed before execution
- proposal contracts integrate with existing execution surfaces without shape drift
- proposal outputs are auditable across events, contracts, and regression tests

## Non-goals

- no autonomous branch creation yet
- no autonomous merge or release
- no open-ended unbounded file mutation
- no automatic approval bypass

## Required Machine-Readable Outputs

- `mutation_proposal`
- `proposal_scope`
- `expected_evidence`
- `rollback_conditions`
- `approval_mode`
- `reason_code`
- `fail_closed`

## Minimum Validation

- `cargo test -p oris-evokernel --test evolution_lifecycle_regression autonomous_proposal_ -- --nocapture`
- `cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental -- --nocapture`
- `cargo test --workspace`

---

## Issue Draft 4 (`AUTO-04`)

### Title

`[EVO26-AUTO-04][P1] Semantic Task-Class Generalization Beyond Normalized Signals`

### Body

## Why

The current replay boundary is still driven by the supported equivalence layer and normalized signal matching. That is strong enough for bounded repeated tasks, but it is not enough for a future autonomous agent that should reuse knowledge across broader task families without overmatching.

This issue expands the semantic generalization layer while keeping false-positive replay at zero for unrelated work.

## Scope

- introduce stronger task-class abstractions beyond current normalized signal matching
- define semantic equivalence rules for bounded low-risk task families
- preserve deterministic replay selection and false-positive prevention
- expose equivalence explanation metadata for audit and debugging
- keep generalized replay limited to explicitly approved task families

## Definition of Done

- equivalent bounded task families can reuse previously learned assets beyond exact normalized matches
- unrelated tasks still do not replay falsely
- replay selection remains deterministic and machine-auditable
- semantic equivalence evidence is externally inspectable in regression coverage

## Non-goals

- no embedding-only fuzzy retrieval with opaque matches
- no widening into arbitrary open-domain semantic reuse
- no autonomy in delivery or release yet
- no silent fallback to unsafe replay on weak equivalence

## Required Machine-Readable Outputs

- `task_equivalence_class`
- `equivalence_explanation`
- `replay_match_confidence`
- `replay_decision`
- `reason_code`
- `fail_closed`

## Minimum Validation

- `cargo test -p oris-evokernel --test evolution_lifecycle_regression semantic_replay_ -- --nocapture`
- `cargo test -p oris-runtime --test agent_self_evolution_travel_network --features full-evolution-experimental -- --nocapture`
- `cargo test --workspace`

---

## Issue Draft 5 (`AUTO-05`)

### Title

`[EVO26-AUTO-05][P1] Continuous Confidence Revalidation and Asset Demotion`

### Body

## Why

Autonomy requires not only successful learning but also active forgetting, requalification, and suppression of stale assets. Without continuous confidence control, autonomous reuse will eventually overfit to outdated or low-quality assets.

This issue adds the missing lifecycle management required before autonomous delivery can be trusted.

## Scope

- add confidence decay for stale or weakly supported assets
- add background revalidation and shadow-mode replay checks
- demote or quarantine assets after repeated failed reuse
- propagate revocation and quarantine transitions through the event model
- ensure replay eligibility reflects current confidence state, not just historical success

## Definition of Done

- assets lose replay eligibility automatically when confidence decays below policy thresholds
- repeated failed reuse causes demotion or quarantine rather than silent continued reuse
- revalidation and demotion events are externally observable and auditable
- replay metrics remain aligned with event evidence after confidence transitions

## Non-goals

- no autonomous PR creation yet
- no autonomous merge or publish yet
- no removal of explicit policy thresholds
- no confidence updates that cannot be explained from evidence

## Required Machine-Readable Outputs

- `confidence_state`
- `revalidation_result`
- `demotion_decision`
- `quarantine_transition`
- `replay_eligibility`
- `reason_code`

## Minimum Validation

- `cargo test -p oris-evokernel --test evolution_lifecycle_regression confidence_revalidation_ -- --nocapture`
- `cargo test -p oris-runtime --test agent_self_evolution_travel_network --features full-evolution-experimental -- --nocapture`
- `cargo test --workspace`

---

## Issue Draft 6 (`AUTO-06`)

### Title

`[EVO26-AUTO-06][P1] Bounded Autonomous Pull-Request Lane For Low-Risk Task Classes`

### Body

## Why

At this point the system can discover, plan, propose, and continuously govern bounded work, but it still cannot independently deliver value to a human review surface. The next step is not autonomous release. It is a bounded autonomous PR lane for explicitly approved low-risk task classes.

This gives Oris a practical autonomy milestone without taking on release blast radius too early.

## Scope

- create branches automatically for explicitly allowed low-risk classes
- package patch evidence, validation evidence, and audit summaries into PR payloads
- open PRs automatically only when all required gates pass
- stop the lane immediately on missing evidence, failed checks, or policy denial
- keep branch and PR behavior deterministic and audit-friendly

## Definition of Done

- qualifying low-risk tasks can open PRs autonomously with full evidence packages
- denied or under-evidenced tasks fail closed before PR creation
- PR summaries expose enough evidence for a reviewer to understand the change without hidden state
- autonomous PR behavior remains restricted to explicitly allowed task classes

## Non-goals

- no autonomous merge yet
- no autonomous publish or release yet
- no auto-PR for high-risk classes
- no best-effort continuation after gate failure

## Required Machine-Readable Outputs

- `delivery_summary`
- `branch_name`
- `pr_payload`
- `evidence_bundle`
- `delivery_status`
- `reason_code`
- `approval_state`

## Minimum Validation

- `cargo test -p oris-orchestrator autonomous_pr_ -- --nocapture`
- `cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental -- --nocapture`
- `cargo test -p oris-runtime --test agent_self_evolution_travel_network --features full-evolution-experimental -- --nocapture`
- `cargo test --workspace`

---

## Issue Draft 7 (`AUTO-07`)

### Title

`[EVO26-AUTO-07][P1] Fail-Closed Autonomous Merge and Release Gate For Narrow Safe Lanes`

### Body

## Why

Autonomous merge and release should be the final autonomy stage, not the first. Once bounded autonomous PR delivery exists, the remaining gap is a release lane that only operates for the narrowest explicitly approved low-risk classes under strong governance and immediate stop conditions.

This issue defines that final lane without widening into unconstrained autonomous release.

## Scope

- define merge gate, release gate, and publish gate contracts for narrow approved task classes
- enforce kill switches, incident stop conditions, and risk-tier policy boundaries
- require complete evidence from intake, planning, proposal, execution, confidence, and PR stages before merge or publish
- add rollback hooks and no-go conditions for post-gate drift
- keep all merge and publish actions fail closed by default

## Definition of Done

- merge or publish is impossible without policy-qualified evidence and explicit class eligibility
- missing or conflicting evidence stops release automation before merge or publish
- release-lane governance is externally inspectable and test-covered
- bounded autonomous release remains restricted to the narrowest safe class set

## Non-goals

- no unconstrained autonomous release
- no high-risk class auto-merge
- no weakening of existing fail-closed semantics
- no hidden publish path outside the explicit release gate

## Required Machine-Readable Outputs

- `merge_gate_result`
- `release_gate_result`
- `publish_gate_result`
- `kill_switch_state`
- `rollback_plan`
- `reason_code`
- `fail_closed`

## Minimum Validation

- `cargo test -p oris-orchestrator autonomous_release_ -- --nocapture`
- `cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental -- --nocapture`
- `cargo test --workspace`