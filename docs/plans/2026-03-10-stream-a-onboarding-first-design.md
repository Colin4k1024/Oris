# Stream A Onboarding-First Design

Date: 2026-03-10
Status: Approved for planning

## 1. Objective

Make first-time contributors complete an Evo first run within 30 minutes with
observable artifacts, using one canonical path and deterministic failure
classification.

## 2. Chosen Direction

Chosen approach: Onboarding-first.

Why:

- fastest path to improving first-run success rate
- keeps scope focused on one entry path before broadening examples
- supports measurable experience outcomes in Stream A

## 3. First-Run Success Contract

A first run is considered successful only when all are true:

1. first-run command exits successfully
2. terminal prints explicit `FIRST_RUN_PASS`
3. artifacts are generated:
   - `target/evo_first_run/summary.json`
   - `target/evo_first_run/run.log`

## 4. Scope

In scope:

- one canonical first-run path
- lightweight preflight checks for local setup
- standardized first-run artifact output
- docs alignment to one entry path

Out of scope:

- full coverage of all example binaries in first-run flow
- external API-key-dependent paths as first-run prerequisites
- network exchange and benchmark as first-run mandatory steps

## 5. User Flow

1. contributor runs a single entry command (`scripts/evo_first_run.sh`)
2. preflight checks validate local runtime prerequisites
3. script runs one minimal local Evo scenario
4. script writes deterministic artifacts under `target/evo_first_run/`
5. terminal prints `FIRST_RUN_PASS` or `FIRST_RUN_FAIL`

## 6. Failure Taxonomy

Fail states are explicit and actionable:

- `E_ENV`: environment/toolchain prerequisite failure
- `E_BUILD`: compile/build failure
- `E_RUNTIME`: scenario runtime failure
- `E_OUTPUT`: required artifacts missing or malformed

Each fail path must:

- keep `run.log`
- print one concrete next command for recovery

## 7. Architecture and Responsibilities

- `scripts/evo_first_run.sh`:
  - owns preflight orchestration and result packaging
- `examples/evo_oris_repo`:
  - remains scenario source for first-run execution target
- docs (`README`, onboarding, examples):
  - expose exactly one first-run primary path
- CI:
  - verifies first-run script execution and output schema without external keys

## 8. Acceptance Criteria

1. New contributors can follow one path with <= 8 documented steps.
2. Running first-run entry always produces pass/fail marker and log artifact.
3. `summary.json` includes:
   - `status`
   - `duration_ms`
   - `scenario`
   - `timestamp`
   - `artifact_paths`
4. CI enforces script executability and artifact shape checks.

## 9. Milestones (2 Weeks)

Week 1 (2026-03-10 to 2026-03-16):

- implement first-run script and artifact contract
- align docs entrypoints to canonical first-run path

Week 2 (2026-03-17 to 2026-03-23):

- wire CI first-run gate
- run newcomer-perspective rehearsal and tune failure guidance

## 10. Risks and Mitigations

Risk: first-run path drifts from evolving examples.

- mitigation: CI first-run gate validates path continuously.

Risk: artifacts become unstable and hard to parse.

- mitigation: keep minimal fixed JSON schema and fail with `E_OUTPUT` on drift.

Risk: docs reintroduce multiple competing entry paths.

- mitigation: designate one canonical first-run section and demote others to
  advanced tracks.
