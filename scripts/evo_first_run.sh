#!/usr/bin/env bash
set -euo pipefail

OUT_DIR="target/evo_first_run"
SUMMARY="${OUT_DIR}/summary.json"
LOG="${OUT_DIR}/run.log"

SCENARIO_CMD="${EVO_FIRST_RUN_SCENARIO_CMD:-cargo run -p evo_oris_repo}"

mkdir -p "${OUT_DIR}"
: > "${LOG}"

START_S="$(date +%s)"

json_quote() {
  local s="$1"
  s="${s//\\/\\\\}"
  s="${s//\"/\\\"}"
  printf '"%s"' "${s}"
}

duration_ms() {
  local end_s
  end_s="$(date +%s)"
  printf '%s' "$(( (end_s - START_S) * 1000 ))"
}

write_summary() {
  local status="$1"
  local code="$2"
  local message="$3"
  local hint="$4"
  local d_ms="$5"
  local now_utc
  now_utc="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"

  local code_json="null"
  local message_json="null"
  local hint_json="null"
  if [[ -n "${code}" ]]; then
    code_json="$(json_quote "${code}")"
  fi
  if [[ -n "${message}" ]]; then
    message_json="$(json_quote "${message}")"
  fi
  if [[ -n "${hint}" ]]; then
    hint_json="$(json_quote "${hint}")"
  fi

  cat > "${SUMMARY}" <<EOF
{
  "status": "$(printf '%s' "${status}")",
  "duration_ms": ${d_ms},
  "scenario": "$(printf '%s' "${SCENARIO_CMD}")",
  "timestamp": "${now_utc}",
  "error_code": ${code_json},
  "error_message": ${message_json},
  "recovery_hint": ${hint_json},
  "artifact_paths": {
    "summary": "${SUMMARY}",
    "log": "${LOG}"
  }
}
EOF
}

fail_with_code() {
  local code="$1"
  local message="$2"
  local hint="$3"
  local d_ms
  d_ms="$(duration_ms)"
  write_summary "fail" "${code}" "${message}" "${hint}" "${d_ms}"
  printf 'FIRST_RUN_FAIL\n'
  printf '%s: %s\n' "${code}" "${message}"
  printf 'Next: %s\n' "${hint}"
  exit 1
}

if [[ "${EVO_FIRST_RUN_FORCE_ENV_FAIL:-0}" == "1" ]]; then
  fail_with_code \
    "E_ENV" \
    "forced environment failure for test coverage" \
    "Run: rustup toolchain install stable && cargo --version"
fi

if ! command -v cargo >/dev/null 2>&1; then
  fail_with_code \
    "E_ENV" \
    "cargo is not installed or not on PATH" \
    "Run: rustup toolchain install stable && cargo --version"
fi

if [[ ! -f "examples/evo_oris_repo/Cargo.toml" ]]; then
  fail_with_code \
    "E_ENV" \
    "missing examples/evo_oris_repo/Cargo.toml in current workspace" \
    "Run from repo root: cd /path/to/Oris && bash scripts/evo_first_run.sh"
fi

if [[ "${EVO_FIRST_RUN_FORCE_BUILD_FAIL:-0}" == "1" ]]; then
  fail_with_code \
    "E_BUILD" \
    "forced build failure for test coverage" \
    "Run: cargo check -p evo_oris_repo --locked"
fi

if [[ "${EVO_FIRST_RUN_SKIP_BUILD_CHECK:-0}" != "1" ]]; then
  if ! cargo check -p evo_oris_repo --quiet >> "${LOG}" 2>&1; then
    fail_with_code \
      "E_BUILD" \
      "build check failed for evo_oris_repo" \
      "Run: cargo check -p evo_oris_repo --locked"
  fi
else
  printf 'Skipping build check because EVO_FIRST_RUN_SKIP_BUILD_CHECK=1\n' >> "${LOG}"
fi

if ! bash -lc "${SCENARIO_CMD}" >> "${LOG}" 2>&1; then
  fail_with_code \
    "E_RUNTIME" \
    "scenario execution failed for evo first run" \
    "Run: cargo run -p evo_oris_repo --bin supervised_devloop"
fi

write_summary "pass" "" "" "" "$(duration_ms)"

if [[ ! -f "${SUMMARY}" || ! -f "${LOG}" ]]; then
  fail_with_code \
    "E_OUTPUT" \
    "required artifacts were not generated" \
    "Re-run: bash scripts/evo_first_run.sh"
fi

printf 'FIRST_RUN_PASS\n'
printf 'Artifacts:\n- %s\n- %s\n' "${SUMMARY}" "${LOG}"
