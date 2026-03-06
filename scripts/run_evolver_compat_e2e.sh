#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

ORIS_SERVER_ADDR="${ORIS_SERVER_ADDR:-127.0.0.1:18080}"
ORIS_BASE_URL="${ORIS_BASE_URL:-http://${ORIS_SERVER_ADDR}}"
ORIS_SQLITE_DB="${ORIS_SQLITE_DB:-/tmp/oris-evolver-compat-e2e.db}"
RUNTIME_LOG_PATH="${RUNTIME_LOG_PATH:-/tmp/oris-evolver-compat-runtime.log}"

EVOLVER_SRC="${EVOLVER_SRC:-/tmp/evolver-latest}"
EVOLVER_REPO_URL="${EVOLVER_REPO_URL:-https://github.com/autogame-17/evolver.git}"
EVOLVER_REF="${EVOLVER_REF:-9c915013a89a4d24fba5dc79a989f18b88f9d2f5}"

A2A_NODE_ID="${A2A_NODE_ID:-oris-evolver-e2e-node}"
PROTOCOL_VERSION="${PROTOCOL_VERSION:-1.0.0}"

RUNTIME_PID=""
CLONED_EVOLVER="0"

cleanup() {
  local exit_code=$?
  if [[ -n "${RUNTIME_PID}" ]] && kill -0 "${RUNTIME_PID}" 2>/dev/null; then
    kill "${RUNTIME_PID}" 2>/dev/null || true
    wait "${RUNTIME_PID}" 2>/dev/null || true
  fi
  rm -f "${ORIS_SQLITE_DB}" || true
  if [[ "${CLONED_EVOLVER}" == "1" ]]; then
    rm -rf "${EVOLVER_SRC}" || true
  fi
  if [[ ${exit_code} -ne 0 ]]; then
    echo "Evolver compatibility e2e failed. Runtime log tail:" >&2
    tail -n 120 "${RUNTIME_LOG_PATH}" >&2 || true
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

ensure_evolver_source() {
  if [[ -d "${EVOLVER_SRC}/.git" ]]; then
    git -C "${EVOLVER_SRC}" fetch --depth 1 origin "${EVOLVER_REF}" >/dev/null 2>&1 || true
    git -C "${EVOLVER_SRC}" checkout --quiet "${EVOLVER_REF}" >/dev/null 2>&1 || true
    return 0
  fi

  git clone --depth 1 "${EVOLVER_REPO_URL}" "${EVOLVER_SRC}"
  CLONED_EVOLVER="1"
  git -C "${EVOLVER_SRC}" fetch --depth 1 origin "${EVOLVER_REF}" >/dev/null 2>&1 || true
  git -C "${EVOLVER_SRC}" checkout --quiet "${EVOLVER_REF}" >/dev/null 2>&1 || true
}

require_cmd cargo
require_cmd curl
require_cmd node
require_cmd git

rm -f "${RUNTIME_LOG_PATH}" "${ORIS_SQLITE_DB}"
ensure_evolver_source

(
  cd "${ROOT_DIR}"
  ORIS_SERVER_ADDR="${ORIS_SERVER_ADDR}" \
  ORIS_SQLITE_DB="${ORIS_SQLITE_DB}" \
  cargo run -p oris-runtime --example execution_server --features "full-evolution-experimental execution-server sqlite-persistence" \
    >"${RUNTIME_LOG_PATH}" 2>&1
) &
RUNTIME_PID=$!

if ! wait_for_runtime; then
  echo "Runtime did not become healthy at ${ORIS_BASE_URL}/healthz" >&2
  exit 1
fi

export A2A_HUB_URL="${ORIS_BASE_URL}"
export A2A_NODE_ID="${A2A_NODE_ID}"
export EVOLVER_SRC="${EVOLVER_SRC}"
export PROTOCOL_VERSION="${PROTOCOL_VERSION}"

node - <<'NODE'
const path = require('path');

const hubUrl = process.env.A2A_HUB_URL;
const nodeId = process.env.A2A_NODE_ID;
const evolverSrc = process.env.EVOLVER_SRC;
const protocolVersion = process.env.PROTOCOL_VERSION || '1.0.0';
const protocol = require(path.join(evolverSrc, 'src/gep/a2aProtocol'));

async function distributeTask(taskId, taskSummary, dispatchId) {
  const response = await fetch(hubUrl.replace(/\/+$/, '') + '/a2a/tasks/distribute', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      sender_id: nodeId,
      protocol_version: protocolVersion,
      task_id: taskId,
      task_summary: taskSummary,
      dispatch_id: dispatchId,
    }),
  });
  if (!response.ok) {
    const body = await response.text().catch(() => '');
    throw new Error(`distributeTask failed for ${taskId}: ${response.status} ${body}`);
  }
}

async function main() {
  const hello = await protocol.sendHelloToHub();
  if (!hello || !hello.ok) {
    throw new Error('hello failed: ' + JSON.stringify(hello));
  }
  const secret = protocol.getHubNodeSecret();
  if (!secret || !/^[a-f0-9]{64}$/i.test(secret)) {
    throw new Error('hello did not persist a 64-hex node_secret');
  }

  const taskReceiver = require(path.join(evolverSrc, 'src/gep/taskReceiver'));
  const transport = protocol.getTransport('http');

  const bootstrapFetch = await transport.receive({ hubUrl, signals: ['docs.rewrite'] });
  if (!Array.isArray(bootstrapFetch) || bootstrapFetch.length === 0) {
    throw new Error('fetch did not return bootstrap results for docs.rewrite');
  }

  await distributeTask('evolver-e2e-task-a', 'evolver e2e task A', 'dispatch-evolver-e2e-a');
  await distributeTask('evolver-e2e-task-b', 'evolver e2e task B', 'dispatch-evolver-e2e-b');

  const tasks = await taskReceiver.fetchTasks();
  if (!tasks || !Array.isArray(tasks.tasks) || tasks.tasks.length < 2) {
    throw new Error('fetchTasks did not return at least 2 distributed tasks');
  }

  const taskA = tasks.tasks[0];
  const taskAId = taskA.id || taskA.task_id;
  if (!taskAId) {
    throw new Error('first fetched task is missing id/task_id');
  }
  const claimedTask = await taskReceiver.claimTask(taskAId);
  if (!claimedTask) {
    throw new Error('claimTask returned false');
  }
  const completedTask = await taskReceiver.completeTask(taskAId, 'asset-e2e-task');
  if (!completedTask) {
    throw new Error('completeTask returned false');
  }

  const hb = await protocol.sendHeartbeat();
  if (!hb || !hb.ok) {
    throw new Error('heartbeat failed: ' + JSON.stringify(hb));
  }
  const availableWork = protocol.consumeAvailableWork();
  if (!Array.isArray(availableWork) || availableWork.length === 0) {
    throw new Error('heartbeat did not return available_work for worker flow');
  }

  const taskB = availableWork[0];
  const taskBId = taskB.id || taskB.task_id;
  if (!taskBId) {
    throw new Error('worker task missing id/task_id');
  }
  const assignment = await taskReceiver.claimWorkerTask(taskBId);
  if (!assignment) {
    throw new Error('claimWorkerTask returned null');
  }
  const assignmentId = assignment.id || assignment.assignment_id;
  if (!assignmentId) {
    throw new Error('worker assignment missing id/assignment_id');
  }
  const workerComplete = await taskReceiver.completeWorkerTask(
    assignmentId,
    'asset-e2e-worker',
  );
  if (!workerComplete) {
    throw new Error('completeWorkerTask returned false');
  }

  console.log('evolver-compat-e2e: PASS');
}

main().catch((err) => {
  console.error(err && err.stack ? err.stack : err);
  process.exit(1);
});
NODE

echo "Evolver compatibility e2e completed successfully."
