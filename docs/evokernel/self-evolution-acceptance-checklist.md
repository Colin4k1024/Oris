# Evo Self-Evolution Acceptance Checklist

> **Status: Active Baseline**
>
> This checklist defines what "self-evolution works" means for the current checked-in Oris Evo implementation.
> It is intentionally scoped to the behavior that exists today: replay-first learning from verified mutations.
> It does **not** claim that Oris already implements a fully autonomous, always-on self-development loop.

## 1. Scope

Current acceptance is limited to:

- successful mutation capture into the evolution store
- later replay reuse for the same or equivalent task signals
- controlled remote asset reuse after local validation
- replay safety under failure, sandbox, and governor constraints
- stable metrics and replay behavior across repeated runs and restarts

Current acceptance explicitly excludes:

- autonomous issue intake
- autonomous task planning
- autonomous branch / PR / release orchestration
- automatic mutation proposal generation without a caller
- fully automatic reinjection of replay hints back into a coding agent loop

## 2. Current Acceptance Statement

Oris may be considered to have passed the current self-evolution baseline only if all of the following are true:

- it learns from at least one successful mutation
- the same task can be replayed on a later run without re-solving from scratch
- repeated learned tasks increase replay utilization
- unrelated tasks do not produce false replay hits
- failed replay attempts stop immediate reuse
- remote assets remain quarantined until a local replay validates them
- replay behavior survives process restart when using the same persistent evolution store
- metrics remain consistent with the event stream

At this stage, the correct product statement is:

> Oris supports **constrained replay-driven self-evolution**.

It is **not yet** accurate to claim:

> Oris is a closed-loop autonomous self-improving development system.

## 3. Acceptance Checklist

### A. Learning Loop

- [x] First encounter of a task may fall back when no matching gene exists.
- [x] After a successful mutation is captured, the same task replays on a later run.
- [x] Repeated executions of the same learned task shift from fallback to replay.
- [x] Deterministic replay selection remains stable for repeated identical inputs.
- [x] Normalized signal variants that remain within the current supported equivalence boundary can still replay the learned capsule.

### B. Safety and Correctness

- [x] Unrelated tasks do not falsely match a previously learned capsule.
- [x] Mixed task sequences replay only for learned signals and continue to fall back for unrelated signals.
- [x] Failed replay validation stops immediate reuse of the failing capsule.
- [x] Sandbox boundaries block out-of-scope patches.
- [x] Governor constraints can block promotion or replay eligibility when policy requires it.

### C. Distributed Evolution

- [x] Locally promoted assets can be exported for remote reuse.
- [x] Remote imported assets enter `Quarantined` state first.
- [x] Remote assets become locally reusable only after a successful local replay validation.
- [x] Remote assets become shareable from the receiving node only after local validation promotes them.
- [x] Distributed replay remains functional after process restart when the same store is reused.

### D. Observability

- [x] `CapsuleReused` events are emitted for successful replay.
- [x] Replay metrics align with the event stream under repeated successful reuse.
- [x] Replay success totals remain stable across long repeated sequences.
- [x] Replay counters in mixed task sequences count only real replay attempts.

## 4. Minimum Test Gate

The current minimum acceptance gate is:

```bash
cargo fmt --all -- --check
cargo test -p oris-evokernel --test evolution_lifecycle_regression
```

The current checked-in regression suite covers:

- single-task learning and second-run replay
- repeated-task replay rate shift
- normalized signal replay
- long repeated replay metric stability
- remote quarantine and local promotion
- distributed replay after restart
- false-positive prevention for unrelated tasks
- mixed learned and unrelated task sequences

## 5. Recommended Quantitative Exit Criteria

For the current baseline, the following thresholds should hold in the test harness:

- repeated learned tasks achieve replay hit rate `>= 80%`
- unrelated tasks maintain false replay hit rate `= 0%`
- replay metrics remain exactly consistent with `CapsuleReused` event counts
- failed replay causes the next immediate attempt to stop reusing the same capsule
- remote imported assets remain non-shareable before local validation and become shareable after local validation

## 6. Current Gaps

The following are still outside the present acceptance envelope:

- no dedicated runtime-owned detect/select/mutate pipeline as separate always-on stages
- no autonomous mutation proposal generation
- no autonomous planner loop
- no autonomous issue-to-release closed loop
- no shadow-mode confidence lifecycle that continuously revalidates assets in the background

The following has been addressed: deterministic task-class replay now includes richer signal extraction (intent key phrases, Rust error codes from validation logs) and a regression test proves multiple semantically adjacent signal variants replay the same learned capsule.

## 7. Overall Evolution Direction

The recommended evolution direction is staged, not monolithic.

### Stage 1. Harden Replay Memory (Now)

Goal:

- make replay learning deterministic, auditable, and safe

Focus:

- event integrity
- projection correctness
- replay determinism
- import/export correctness
- restart durability
- metric fidelity

Definition of done:

- replay is reliable enough to be trusted as a real optimization path

### Stage 2. Expand Task-Class Generalization (Shipped)

Goal:

- evolve from exact or normalized signal replay into broader task-class reuse

Focus:

- richer signal extraction from logs, diffs, and validation output
- stronger equivalence matching across paraphrases and adjacent failure signatures
- ranking improvements that distinguish near-match from true no-match
- negative controls that keep false positives at zero or near zero

Current deterministic boundary:

- reordered or superset multi-token signal phrases can match when at least two normalized tokens align
- isolated single-token overlap does not qualify as task-class replay on its own
- signal extraction includes intent key phrases and Rust error codes from validation logs

Definition of done:

- one learned fix can help multiple semantically equivalent task variants, not just near-identical ones (regression: `multiple_semantically_adjacent_signal_variants_replay_same_capsule`)

### Stage 3. Introduce Continuous Confidence Control (Shipped)

Goal:

- move from one-shot promotion to continuously managed asset trust

Focus:

- confidence decay
- periodic revalidation
- shadow replay
- automatic demotion / revocation on drift
- freshness-aware candidate ranking

Current deterministic boundary:

- replay ranking now multiplies signal quality by decayed confidence
- stale promoted assets can be lazily demoted back to `Quarantined` on replay lookup
- `oris_evolution_confidence_revalidations_total` exposes confidence-driven revalidation events

Definition of done:

- stale or environment-diverged assets lose priority automatically before they become harmful (regression: `stale_confidence_forces_revalidation_before_replay`, `env_divergence_reduces_replay_eligibility`)

### Stage 4. Close the Agent Feedback Loop (Shipped)

Goal:

- connect replay results back into the coding workflow so the system actually accelerates future implementation

Focus:

- structured replay hints back to the caller
- explicit fallback reasons the planner can consume
- capture from real task / proposal flows
- measuring reasoning calls avoided per task

Definition of done:

- replay measurably reduces agent reasoning and implementation latency on repeated work (regression: `replay_feedback_surfaces_planner_hints_and_reasoning_savings`; example: evo_oris_repo consumes `replay_feedback_for_agent`)

### Stage 5. Move Toward Autonomous DEVLOOP (Shipped)

Goal:

- evolve from reusable execution memory into a supervised self-improving development loop

Focus:

- issue intake
- task classification
- mutation proposal generation
- gated validation pipelines
- branch / PR orchestration under policy
- human approval boundaries

Definition of done:

- Oris can execute a bounded subset of development work end-to-end under supervision (regression: `supervised_devloop_executes_bounded_docs_task_after_approval`, `supervised_devloop_stops_before_execution_without_human_approval`, `supervised_devloop_rejects_out_of_scope_tasks_without_bypassing_policy`)

### Stage 6. Mature Federated Evolution (Shipped)

Goal:

- make multi-node sharing reliable, attributable, and economically stable

Focus:

- publisher attribution fidelity
- remote reputation weighting
- anti-spam economics
- safe revocation propagation
- consistency under duplicate import, restart, and partial network failure

Definition of done:

- remote learning becomes a trustworthy multiplier rather than a contamination risk (regression: `remote_learning_requires_local_validation_before_becoming_shareable`, `distributed_learning_survives_restart_and_replays_again`, `duplicate_remote_import_does_not_requarantine_locally_validated_assets`, `retry_remote_import_after_partial_failure_only_imports_missing_assets`; lib: revocation and reputation bias coverage)

## 8. North-Star Outcome

The long-term target is:

```text
Task
-> Detect
-> Replay if trusted
-> Mutate only when needed
-> Validate
-> Capture
-> Reuse across future tasks
-> Reduce reasoning over time
```

The correct strategic direction is:

- first make replay trustworthy
- then make matching broader
- then make confidence continuous
- then connect replay into the agent loop
- only after that pursue supervised autonomy

This order matters.
If replay trust is weak, every later stage compounds error instead of compounding learning.
