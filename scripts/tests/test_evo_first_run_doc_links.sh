#!/usr/bin/env bash
set -euo pipefail

for f in README.md docs/open-source-onboarding-zh.md docs/evokernel/examples.md examples/evo_oris_repo/README.md; do
  rg -n 'scripts/evo_first_run.sh' "${f}"
done
