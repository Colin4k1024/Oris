#!/usr/bin/env bash
set -euo pipefail

ARTIFACT_DIR="target/evomap_release_gate"
SUMMARY_FILE="target/evomap-release-evidence.json"
LOG_FILE="${ARTIFACT_DIR}/run.log"
SNAPSHOT_BASELINE_FILE="crates/oris-runtime/examples/assets/experience_assets_snapshot.json"
SNAPSHOT_CURRENT_FILE="${ARTIFACT_DIR}/experience_assets_snapshot.current.json"
SNAPSHOT_CURRENT_MD_FILE="${ARTIFACT_DIR}/experience_assets_snapshot.current.md"
SNAPSHOT_DIFF_FILE="${ARTIFACT_DIR}/experience_snapshot_diff.json"
SNAPSHOT_DIFF_STATUS="not-run"
SNAPSHOT_ADDED_COUNT=0
SNAPSHOT_REMOVED_COUNT=0
SNAPSHOT_CHANGED_COUNT=0
RELEASE_GATE_EVIDENCE_FILE="${ARTIFACT_DIR}/self_evolution_release_gate.json"
RELEASE_GATE_EVIDENCE_OUT_FILE="$(pwd)/${RELEASE_GATE_EVIDENCE_FILE}"
RELEASE_GATE_STATUS="not-run"
RELEASE_GATE_FAILED_CHECKS_COUNT=0

mkdir -p "${ARTIFACT_DIR}"
: > "${LOG_FILE}"

if [[ -n "${ORIS_TEST_POSTGRES_URL:-}" ]]; then
  BACKEND_PARITY_MODE="sqlite+postgres"
else
  BACKEND_PARITY_MODE="sqlite+postgres-env-gated"
fi

CURRENT_STEP="bootstrap"
STATUS="pass"

update_snapshot_diff_metrics() {
  if [[ ! -f "${SNAPSHOT_DIFF_FILE}" ]]; then
    return 0
  fi

  local values
  values="$(
    python3 - "${SNAPSHOT_DIFF_FILE}" <<'PY'
import json
import sys

path = sys.argv[1]
with open(path, "r", encoding="utf-8") as f:
    payload = json.load(f)

status = str(payload.get("status", "unknown"))
summary = payload.get("summary")
if not isinstance(summary, dict):
    summary = {}

added = int(summary.get("added_count", 0))
removed = int(summary.get("removed_count", 0))
changed = int(summary.get("changed_count", 0))
print(f"{status}|{added}|{removed}|{changed}")
PY
  )"

  IFS='|' read -r SNAPSHOT_DIFF_STATUS SNAPSHOT_ADDED_COUNT SNAPSHOT_REMOVED_COUNT SNAPSHOT_CHANGED_COUNT <<< "${values}"
}

update_release_gate_metrics() {
  if [[ ! -f "${RELEASE_GATE_EVIDENCE_OUT_FILE}" ]]; then
    return 0
  fi

  local values
  values="$(
    python3 - "${RELEASE_GATE_EVIDENCE_OUT_FILE}" <<'PY'
import json
import sys

path = sys.argv[1]
with open(path, "r", encoding="utf-8") as f:
    payload = json.load(f)

status = str(payload.get("status", "unknown"))
failed_checks = payload.get("failed_checks")
if not isinstance(failed_checks, list):
    failed_checks = []

print(f"{status}|{len(failed_checks)}")
PY
  )"

  IFS='|' read -r RELEASE_GATE_STATUS RELEASE_GATE_FAILED_CHECKS_COUNT <<< "${values}"
}

write_summary() {
  update_snapshot_diff_metrics
  update_release_gate_metrics

  local generated_at
  generated_at="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
  cat > "${SUMMARY_FILE}" <<JSON
{
  "gate": "evomap-release-hardening",
  "status": "${STATUS}",
  "backend_parity_mode": "${BACKEND_PARITY_MODE}",
  "failed_step": "${CURRENT_STEP}",
  "generated_at": "${generated_at}",
  "log_file": "${LOG_FILE}",
  "snapshot_baseline_file": "${SNAPSHOT_BASELINE_FILE}",
  "snapshot_current_file": "${SNAPSHOT_CURRENT_FILE}",
  "snapshot_diff_file": "${SNAPSHOT_DIFF_FILE}",
  "snapshot_diff_status": "${SNAPSHOT_DIFF_STATUS}",
  "snapshot_diff_added_count": ${SNAPSHOT_ADDED_COUNT},
  "snapshot_diff_removed_count": ${SNAPSHOT_REMOVED_COUNT},
  "snapshot_diff_changed_count": ${SNAPSHOT_CHANGED_COUNT},
  "release_gate_evidence_file": "${RELEASE_GATE_EVIDENCE_OUT_FILE}",
  "release_gate_status": "${RELEASE_GATE_STATUS}",
  "release_gate_failed_checks_count": ${RELEASE_GATE_FAILED_CHECKS_COUNT}
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

run_step "runtime-self-evolution-release-gate" \
  env ORIS_RELEASE_GATE_EVIDENCE_OUT="${RELEASE_GATE_EVIDENCE_OUT_FILE}" \
  cargo test -p oris-runtime --test agent_self_evolution_travel_network \
  --features full-evolution-experimental -- --nocapture

run_step "runtime-self-evolution-release-gate-enforce-pass" \
  python3 - "${RELEASE_GATE_EVIDENCE_OUT_FILE}" <<'PY'
import json
import sys

path = sys.argv[1]
with open(path, "r", encoding="utf-8") as f:
    payload = json.load(f)

status = str(payload.get("status", "unknown"))
if status != "pass":
    failed_checks = payload.get("failed_checks")
    if not isinstance(failed_checks, list):
        failed_checks = []
    summary = str(payload.get("summary", ""))
    print(
        f"[evomap-release-gate] release gate blocked publish status={status} "
        f"failed_checks={failed_checks} summary={summary}",
        file=sys.stderr,
    )
    sys.exit(1)
PY

run_step "experience-snapshot-export" \
  python3 scripts/export_experience_assets_snapshot.py \
  --json-out "${SNAPSHOT_CURRENT_FILE}" \
  --md-out "${SNAPSHOT_CURRENT_MD_FILE}"

run_step "experience-snapshot-diff" \
  python3 scripts/compare_experience_snapshots.py \
  --baseline "${SNAPSHOT_BASELINE_FILE}" \
  --current "${SNAPSHOT_CURRENT_FILE}" \
  --json-out "${SNAPSHOT_DIFF_FILE}"

run_step "execution-runtime-backend-parity" \
  cargo test -p oris-execution-runtime --features "sqlite-persistence,kernel-postgres" \
  runtime_repository_semantic_contract_ -- --nocapture --test-threads=1

CURRENT_STEP="completed"
