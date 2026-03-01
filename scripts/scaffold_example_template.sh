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

escape_sed_replacement() {
  printf '%s' "$1" | sed -e 's/[\/&|]/\\&/g'
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

project_name="$(basename "${target_dir}")"
crate_name="$(printf '%s' "${project_name}" | tr '[:upper:]-' '[:lower:]_' | tr -c '[:alnum:]_' '_')"
project_name_escaped="$(escape_sed_replacement "${project_name}")"
crate_name_escaped="$(escape_sed_replacement "${crate_name}")"
mkdir -p "${target_dir}"

while IFS= read -r -d '' src; do
  rel="${src#${template_dir}/}"
  dst_rel="${rel}"
  if [[ "${rel}" == *.tmpl ]]; then
    dst_rel="${rel%.tmpl}"
  fi
  dst="${target_dir}/${dst_rel}"
  mkdir -p "$(dirname "${dst}")"
  sed \
    -e "s|__CRATE_NAME__|${crate_name_escaped}|g" \
    -e "s|{{project-name}}|${project_name_escaped}|g" \
    -e "s|{{crate_name}}|${crate_name_escaped}|g" \
    "${src}" > "${dst}"
done < <(find "${template_dir}" -type f -print0)

cat <<EOF
Scaffold created:
  template: ${template}
  target:   ${target_dir}
  project:  ${project_name}
  crate:    ${crate_name}

Next steps:
  cd ${target_dir}
  cargo run
EOF
