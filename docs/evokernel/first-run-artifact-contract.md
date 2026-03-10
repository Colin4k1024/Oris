# First Run Artifact Contract

Date: 2026-03-10

This contract defines the minimum observable output required by the canonical
first-run flow.

## Required output files

- `target/evo_first_run/summary.json`
- `target/evo_first_run/run.log`

## Required `summary.json` keys

- `status`
- `duration_ms`
- `scenario`
- `timestamp`
- `artifact_paths`

## Pass/fail markers

- `FIRST_RUN_PASS`
- `FIRST_RUN_FAIL`

## Failure taxonomy

- `E_ENV`
- `E_BUILD`
- `E_RUNTIME`
- `E_OUTPUT`
