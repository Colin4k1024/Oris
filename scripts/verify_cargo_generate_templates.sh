#!/usr/bin/env bash
set -euo pipefail

# Validate the same template sources used by `cargo-generate` via the
# repository-local compatibility renderer.

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_root="$(mktemp -d "${TMPDIR:-/tmp}/oris-cargo-generate-XXXXXX")"

cleanup() {
  rm -rf "${tmp_root}"
}
trap cleanup EXIT

templates=(
  axum_service
  worker_only
  operator_cli
)

for template in "${templates[@]}"; do
  target_dir="${tmp_root}/${template}"
  bash "${repo_root}/scripts/scaffold_example_template.sh" "${template}" "${target_dir}" >/dev/null

  if grep -q '^oris-runtime = ' "${target_dir}/Cargo.toml"; then
    sed -i.bak \
      "s|^oris-runtime = .*|oris-runtime = { path = \"${repo_root}/crates/oris-runtime\", features = [\"sqlite-persistence\", \"execution-server\"] }|" \
      "${target_dir}/Cargo.toml"
    rm -f "${target_dir}/Cargo.toml.bak"
  fi

  cargo check --manifest-path "${target_dir}/Cargo.toml" --offline >/dev/null
  printf 'verified template: %s\n' "${template}"
done
