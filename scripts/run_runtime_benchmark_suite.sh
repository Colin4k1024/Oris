#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"

output_path="${1:-target/runtime-benchmark-latest.json}"
sample_size="${2:-5}"

mkdir -p "$(dirname "${output_path}")"

cargo run -p oris-runtime \
  --example runtime_benchmark_suite \
  --features "sqlite-persistence,execution-server" \
  --quiet \
  -- "${output_path}" "${sample_size}"
