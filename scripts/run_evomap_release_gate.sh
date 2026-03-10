#!/usr/bin/env bash
set -euo pipefail

ARTIFACT_DIR="target/evomap_release_gate"
SUMMARY_FILE="target/evomap-release-evidence.json"
LOG_FILE="${ARTIFACT_DIR}/run.log"

mkdir -p "${ARTIFACT_DIR}"
: > "${LOG_FILE}"

if [[ -n "${ORIS_TEST_POSTGRES_URL:-}" ]]; then
  BACKEND_PARITY_MODE="sqlite+postgres"
else
  BACKEND_PARITY_MODE="sqlite+postgres-env-gated"
fi

CURRENT_STEP="bootstrap"
STATUS="pass"

write_summary() {
  local generated_at
  generated_at="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
  cat > "${SUMMARY_FILE}" <<JSON
{
  "gate": "evomap-release-hardening",
  "status": "${STATUS}",
  "backend_parity_mode": "${BACKEND_PARITY_MODE}",
  "failed_step": "${CURRENT_STEP}",
  "generated_at": "${generated_at}",
  "log_file": "${LOG_FILE}"
}
JSON
}

on_exit() {
  local code=$?
  if [[ ${code} -ne 0 ]]; then
    STATUS="fail"
  fi
  write_summary
}
trap on_exit EXIT

run_step() {
  local step="$1"
  shift
  CURRENT_STEP="${step}"
  echo "[evomap-release-gate] ${step}" | tee -a "${LOG_FILE}"
  "$@" 2>&1 | tee -a "${LOG_FILE}"
}

run_step "orchestrator-evidence-gate" \
  cargo test -p oris-orchestrator evidence_gate -- --nocapture

run_step "orchestrator-coordinator-gate" \
  cargo test -p oris-orchestrator coordinator_flow -- --nocapture

run_step "runtime-semantic-e2e" \
  cargo test -p oris-runtime --features "full-evolution-experimental execution-server sqlite-persistence" \
  execution_server::api_handlers::tests::evomap_semantic_contract_e2e_covers_protocol_task_asset_and_governance_flows \
  -- --nocapture --test-threads=1

run_step "runtime-audit-core-actions" \
  cargo test -p oris-runtime --features "full-evolution-experimental execution-server sqlite-persistence" \
  execution_server::api_handlers::tests::audit_logs_capture_semantic_protocol_core_actions \
  -- --nocapture --test-threads=1

run_step "execution-runtime-backend-parity" \
  cargo test -p oris-execution-runtime --features "sqlite-persistence,kernel-postgres" \
  runtime_repository_semantic_contract_ -- --nocapture --test-threads=1

CURRENT_STEP="completed"
