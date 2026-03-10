#!/usr/bin/env bash
set -euo pipefail

rg -n '^# First Run Artifact Contract$' docs/evokernel/first-run-artifact-contract.md
rg -n '`status`|`duration_ms`|`scenario`|`timestamp`|`artifact_paths`' docs/evokernel/first-run-artifact-contract.md
