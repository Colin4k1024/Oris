#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

DEMO_RUN_ID="${DEMO_RUN_ID:-external-$(date +%Y%m%d-%H%M%S)}"
DEMO_RUNS_ROOT="${DEMO_RUNS_ROOT:-${ROOT_DIR}/docs/evokernel/demo_runs}"
LATEST_STORE_ROOT="${LATEST_STORE_ROOT:-${ROOT_DIR}/docs/evokernel/latest-store}"
FEATURES="${FEATURES:-full-evolution-experimental}"

RUN_ROOT="${DEMO_RUNS_ROOT}/${DEMO_RUN_ID}"
ASSET_ROOT="${RUN_ROOT}/experience_assets"

ts_now() {
  python3 - <<'PY'
from datetime import datetime
print(datetime.now().astimezone().isoformat(timespec="milliseconds"))
PY
}

ts_echo() {
  echo "[$(ts_now)] $*"
}

prefix_ts_stream() {
  python3 -c '
import sys
import re
from datetime import datetime

already_ts = re.compile(r"^\[\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}[+-]\d{2}:\d{2}\](?:\s|$)")

for raw in sys.stdin.buffer:
    line = raw.decode("utf-8", errors="replace")
    if already_ts.match(line):
        sys.stdout.write(line)
    else:
        ts = datetime.now().astimezone().isoformat(timespec="milliseconds")
        sys.stdout.write(f"[{ts}] {line}")
    sys.stdout.flush()
'
}

require_cmd() {
  local cmd="$1"
  if ! command -v "${cmd}" >/dev/null 2>&1; then
    ts_echo "Missing required command: ${cmd}" >&2
    exit 1
  fi
}

require_cmd cargo
require_cmd python3

if [[ -z "${QWEN_API_KEY:-}" ]]; then
  ts_echo "QWEN_API_KEY is required for this external demo." >&2
  exit 1
fi

ts_echo "== Travel Network External Demo =="
ts_echo "run_id=${DEMO_RUN_ID}"
ts_echo "demo_runs_root=${DEMO_RUNS_ROOT}"
ts_echo "latest_store_root=${LATEST_STORE_ROOT}"

(
  cd "${ROOT_DIR}"
  ORIS_TRAVEL_DEMO_RUN_ID="${DEMO_RUN_ID}" \
  ORIS_TRAVEL_DEMO_ROOT="${DEMO_RUNS_ROOT}" \
  ORIS_TRAVEL_LATEST_STORE_ROOT="${LATEST_STORE_ROOT}" \
  cargo run \
    -p oris-runtime \
    --example agent_self_evolution_travel_network \
    --features "${FEATURES}" \
    2>&1
) | prefix_ts_stream

python3 - "${RUN_ROOT}" "${ASSET_ROOT}" <<'PY' 2>&1 | prefix_ts_stream
import json
import sys
from pathlib import Path

run_root = Path(sys.argv[1])
asset_root = Path(sys.argv[2])

required = [
    "gene.json",
    "capsule.json",
    "evolution_events.jsonl",
    "mutation.json",
    "validation_report.json",
    "memory_graph_events.jsonl",
    "reuse_verification.json",
    "self_repair_trace.json",
    "asset_manifest.json",
]
run_root_required = [
    "producer_llm_output_draft_v1.md",
    "producer_llm_output_repair_v2.md",
    "consumer_llm_output_task_b.md",
    "consumer-workspace/docs/evolution/travel-beijing-shanghai-experience.md",
]
missing = [name for name in required if not (asset_root / name).exists()]
missing.extend(
    [name for name in run_root_required if not (run_root / name).exists()]
)
if missing:
    raise SystemExit(f"missing required assets: {missing}")

consumer_experience_doc = (
    run_root / "consumer-workspace/docs/evolution/travel-beijing-shanghai-experience.md"
)
consumer_doc_text = consumer_experience_doc.read_text(encoding="utf-8")
if "## consumer_llm_output_full" not in consumer_doc_text:
    raise SystemExit(
        "consumer experience doc is not persisted with full output marker: "
        f"{consumer_experience_doc}"
    )

reuse = json.loads((asset_root / "reuse_verification.json").read_text(encoding="utf-8"))
repair = json.loads((asset_root / "self_repair_trace.json").read_text(encoding="utf-8"))
manifest = json.loads((asset_root / "asset_manifest.json").read_text(encoding="utf-8"))

checks = {
    "initial_failure_detected": repair.get("initial_failure_detected") is True,
    "repair_success": repair.get("repair_success") is True,
    "import_accepted": reuse.get("import_accepted") is True,
    "imported_asset_count>0": int(reuse.get("imported_asset_count", 0)) > 0,
    "used_capsule": reuse.get("used_capsule") is True,
    "fallback_to_planner=false": reuse.get("fallback_to_planner") is False,
    "capsule_reused_event_detected": reuse.get("capsule_reused_event_detected") is True,
    "final_reuse_verdict": reuse.get("final_reuse_verdict") is True,
    "repair_reuse_verdict": reuse.get("repair_reuse_verdict") is True,
    "missing_assets_empty": len(manifest.get("missing_assets", [])) == 0,
    "manifest_has_self_repair_trace": bool(manifest.get("assets", {}).get("self_repair_trace")),
    "manifest_self_repair_trace_exists": manifest.get("assets", {})
    .get("self_repair_trace", {})
    .get("exists")
    is True,
}
failed = [name for name, ok in checks.items() if not ok]
if failed:
    raise SystemExit(f"verification failed: {failed}")

print("External demo verification: PASS")
print(f"run_root={run_root}")
print(f"asset_root={asset_root}")
print(f"validation_report={run_root / 'validation_report.md'}")
print(f"producer_failed_plan={run_root / 'producer_plan_failed_v1.md'}")
print(f"producer_repaired_plan={run_root / 'producer_plan.md'}")
print(f"producer_llm_output_draft_v1={run_root / 'producer_llm_output_draft_v1.md'}")
print(f"producer_llm_output_repair_v2={run_root / 'producer_llm_output_repair_v2.md'}")
print(f"consumer_llm_output_task_b={run_root / 'consumer_llm_output_task_b.md'}")
print(f"consumer_experience_doc={consumer_experience_doc}")
print(
    "repair_trace_summary="
    + json.dumps(
        {
            "initial_failure_detected": repair.get("initial_failure_detected"),
            "failed_checks": repair.get("failed_checks"),
            "failure_reason": repair.get("failure_reason"),
            "repair_applied": repair.get("repair_applied"),
            "repair_success": repair.get("repair_success"),
        },
        ensure_ascii=False,
    )
)
print(
    "manifest_self_repair_trace="
    + json.dumps(manifest.get("assets", {}).get("self_repair_trace", {}), ensure_ascii=False)
)
print(
    "reuse_summary="
    + json.dumps(
        {
            "final_reuse_verdict": reuse.get("final_reuse_verdict"),
            "repair_reuse_verdict": reuse.get("repair_reuse_verdict"),
            "replay_reason": reuse.get("replay_reason"),
            "reused_capsule_id": reuse.get("reused_capsule_id"),
        },
        ensure_ascii=False,
    )
)
PY

ts_echo "PASS: travel network external demo completed"
