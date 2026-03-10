#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

DEMO_ROOT="${DEMO_ROOT:-${ROOT_DIR}/crates/oris-runtime/examples/demo_store/travel_bj_sh}"
ORIS_SERVER_ADDR="${ORIS_SERVER_ADDR:-127.0.0.1:18081}"
ORIS_BASE_URL="${ORIS_BASE_URL:-http://${ORIS_SERVER_ADDR}}"
ORIS_SQLITE_DB="${ORIS_SQLITE_DB:-${DEMO_ROOT}/oris_execution_server.db}"
RUNTIME_LOG_PATH="${RUNTIME_LOG_PATH:-${DEMO_ROOT}/runtime.log}"
TRAVEL_MODEL="${TRAVEL_MODEL:-qwen:qwen3-max}"

SNAPSHOT_JSON="${ROOT_DIR}/crates/oris-runtime/examples/assets/experience_assets_snapshot.travel_bj_sh.json"
SNAPSHOT_MD="${ROOT_DIR}/crates/oris-runtime/examples/assets/experience_assets_snapshot.travel_bj_sh.md"

RUNTIME_PID=""
RUNTIME_STARTED="0"

cleanup() {
  local exit_code=$?
  if [[ -n "${RUNTIME_PID}" ]] && kill -0 "${RUNTIME_PID}" 2>/dev/null; then
    kill "${RUNTIME_PID}" 2>/dev/null || true
    wait "${RUNTIME_PID}" 2>/dev/null || true
  fi
  if [[ ${exit_code} -ne 0 && "${RUNTIME_STARTED}" == "1" ]]; then
    echo "travel experience demo failed; runtime log tail:" >&2
    if [[ -f "${RUNTIME_LOG_PATH}" ]]; then
      tail -n 120 "${RUNTIME_LOG_PATH}" >&2 || true
    else
      echo "runtime log not found: ${RUNTIME_LOG_PATH}" >&2
    fi
  fi
  exit ${exit_code}
}
trap cleanup EXIT

require_cmd() {
  local cmd="$1"
  if ! command -v "${cmd}" >/dev/null 2>&1; then
    echo "Missing required command: ${cmd}" >&2
    exit 1
  fi
}

wait_for_runtime() {
  local max_attempts=60
  local i
  for ((i=1; i<=max_attempts; i++)); do
    if curl -fsS "${ORIS_BASE_URL}/healthz" >/dev/null 2>&1; then
      return 0
    fi
    sleep 1
  done
  return 1
}

require_cmd cargo
require_cmd curl
require_cmd python3

if [[ -z "${QWEN_API_KEY:-}" ]]; then
  echo "QWEN_API_KEY is required for this online demo." >&2
  exit 1
fi

echo "Preparing demo store at: ${DEMO_ROOT}"
rm -rf "${DEMO_ROOT}"
mkdir -p "${DEMO_ROOT}"
mkdir -p "$(dirname "${SNAPSHOT_JSON}")"

echo "Starting execution_server on ${ORIS_SERVER_ADDR}"
(
  cd "${DEMO_ROOT}"
  ORIS_SERVER_ADDR="${ORIS_SERVER_ADDR}" \
  ORIS_SQLITE_DB="${ORIS_SQLITE_DB}" \
  cargo run \
    --manifest-path "${ROOT_DIR}/Cargo.toml" \
    -p oris-runtime \
    --example execution_server \
    --features "full-evolution-experimental execution-server sqlite-persistence" \
    >"${RUNTIME_LOG_PATH}" 2>&1
) &
RUNTIME_PID=$!
RUNTIME_STARTED="1"

if ! wait_for_runtime; then
  echo "Runtime did not become healthy at ${ORIS_BASE_URL}/healthz" >&2
  exit 1
fi

echo "Running A/B travel experience share scenario"
(
  cd "${ROOT_DIR}"
  ORIS_BASE_URL="${ORIS_BASE_URL}" \
  TRAVEL_MODEL="${TRAVEL_MODEL}" \
  cargo run \
    -p oris-runtime \
    --example a2a_travel_experience_share \
    --features "full-evolution-experimental"
)

echo "Exporting experience snapshot"
(
  cd "${ROOT_DIR}"
  python3 scripts/export_experience_assets_snapshot.py \
    --store-dir "crates/oris-runtime/examples/demo_store/travel_bj_sh/.oris/evolution" \
    --json-out "crates/oris-runtime/examples/assets/experience_assets_snapshot.travel_bj_sh.json" \
    --md-out "crates/oris-runtime/examples/assets/experience_assets_snapshot.travel_bj_sh.md"
)

python3 - <<'PY'
import json
from pathlib import Path

task_class = "travel.itinerary.cn.beijing-shanghai"
snapshot = Path("crates/oris-runtime/examples/assets/experience_assets_snapshot.travel_bj_sh.json")
if not snapshot.exists():
    raise SystemExit(f"snapshot missing: {snapshot}")

data = json.loads(snapshot.read_text(encoding="utf-8"))
finalized = data.get("finalized_experiences", [])
has_travel_gene = any(
    str(item.get("freeze_id", "")).startswith("a2a-gene-")
    and item.get("task_class") == task_class
    for item in finalized
)
if not has_travel_gene:
    raise SystemExit("no finalized a2a-gene-* for travel task class found in snapshot")

print("Snapshot validation: PASS")
print(f"asset_count={data.get('asset_count')} finalized_count={data.get('finalized_count')}")
print(f"snapshot_json={snapshot.resolve()}")
print(
    "snapshot_md="
    + str(Path("crates/oris-runtime/examples/assets/experience_assets_snapshot.travel_bj_sh.md").resolve())
)
PY

echo "PASS: travel experience share demo completed"
echo "store_dir=${DEMO_ROOT}/.oris/evolution"
echo "runtime_log=${RUNTIME_LOG_PATH}"
