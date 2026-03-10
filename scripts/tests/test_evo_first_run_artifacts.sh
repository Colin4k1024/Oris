#!/usr/bin/env bash
set -euo pipefail

rm -rf target/evo_first_run

EVO_FIRST_RUN_SKIP_BUILD_CHECK=1 \
EVO_FIRST_RUN_SCENARIO_CMD='echo mock-evo-first-run' \
  bash scripts/evo_first_run.sh

test -f target/evo_first_run/summary.json
test -f target/evo_first_run/run.log

rg -n '"status"|"duration_ms"|"scenario"|"timestamp"|"artifact_paths"' target/evo_first_run/summary.json
