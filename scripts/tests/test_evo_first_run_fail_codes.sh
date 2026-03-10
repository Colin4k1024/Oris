#!/usr/bin/env bash
set -euo pipefail

rg -n 'E_ENV|E_BUILD|E_RUNTIME|E_OUTPUT' scripts/evo_first_run.sh
rg -n 'FIRST_RUN_PASS|FIRST_RUN_FAIL' scripts/evo_first_run.sh
