#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  bash scripts/scaffold_example_template.sh <template> <target-dir>

Templates:
  axum_service
  worker_only
  operator_cli
EOF
}

if [[ $# -ne 2 ]]; then
  usage
  exit 1
fi

template="$1"
target_dir="$2"
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
template_dir="${repo_root}/examples/templates/${template}"

if [[ ! -d "${template_dir}" ]]; then
  echo "error: template not found: ${template}" >&2
  usage
  exit 1
fi

if [[ -e "${target_dir}" ]]; then
  echo "error: target already exists: ${target_dir}" >&2
  exit 1
fi

crate_name="$(basename "${target_dir}")"
mkdir -p "${target_dir}"

while IFS= read -r -d '' src; do
  rel="${src#${template_dir}/}"
  dst_rel="${rel}"
  use_template=0
  if [[ "${rel}" == *.tmpl ]]; then
    dst_rel="${rel%.tmpl}"
    use_template=1
  fi
  dst="${target_dir}/${dst_rel}"
  mkdir -p "$(dirname "${dst}")"

  if [[ ${use_template} -eq 1 ]]; then
    sed "s/__CRATE_NAME__/${crate_name}/g" "${src}" > "${dst}"
  else
    cp "${src}" "${dst}"
  fi
done < <(find "${template_dir}" -type f -print0)

cat <<EOF
Scaffold created:
  template: ${template}
  target:   ${target_dir}
  crate:    ${crate_name}

Next steps:
  cd ${target_dir}
  cargo run
EOF
