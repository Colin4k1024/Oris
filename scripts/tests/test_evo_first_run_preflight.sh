#!/usr/bin/env bash
set -euo pipefail

OUT="$(EVO_FIRST_RUN_FORCE_ENV_FAIL=1 bash scripts/evo_first_run.sh || true)"

printf '%s' "${OUT}" | rg 'E_ENV'
printf '%s' "${OUT}" | rg 'rustup toolchain install stable|cargo --version'
