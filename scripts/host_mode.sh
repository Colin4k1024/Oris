#!/usr/bin/env bash
set -euo pipefail

# ═══════════════════════════════════════════════════════════
#  Oris Host Mode
#  Claude Code orchestrates OpenCode as a coding worker.
#  No human confirmation needed.
# ═══════════════════════════════════════════════════════════

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# ── Defaults ──
OPENCODE_MODEL="${OPENCODE_MODEL:-anthropic/claude-sonnet-4}"
HOST_MODE_MAX_RETRIES="${HOST_MODE_MAX_RETRIES:-1}"
HOST_MODE_TIMEOUT="${HOST_MODE_TIMEOUT:-300}"
LOG_DIR="${PROJECT_ROOT}/.host-mode-logs"

# ── Usage ──
usage() {
    cat <<EOF
Oris Host Mode — Claude Code orchestrates OpenCode

Usage:
  $(basename "$0") [options]

Modes:
  --issue <N>       Process a specific GitHub issue
  --issue-loop      Continuously process open issues (default)
  --task "desc"     Process a free-form coding task

Options:
  --model <m>       OpenCode model (default: $OPENCODE_MODEL)
  --retries <n>     Max retries on validation failure (default: $HOST_MODE_MAX_RETRIES)
  --timeout <s>     OpenCode timeout in seconds (default: $HOST_MODE_TIMEOUT)
  -h, --help        Show this help

Environment Variables:
  OPENCODE_MODEL           Override default model
  OPENCODE_SERVER_PORT     If set, use OpenCode headless server at this port
  HOST_MODE_MAX_RETRIES    Override default max retries
  HOST_MODE_TIMEOUT        Override default timeout

Examples:
  $(basename "$0") --issue 42
  $(basename "$0") --issue-loop
  $(basename "$0") --task "Fix unused imports in memory_graph.rs"
  OPENCODE_MODEL=openai/gpt-4o $(basename "$0") --issue 42
EOF
}

# ── Parse args ──
MODE="issue-loop"
ISSUE_NUMBER=""
TASK_DESC=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --issue)
            MODE="issue"
            ISSUE_NUMBER="${2:-}"
            [[ -z "$ISSUE_NUMBER" ]] && { echo "ERROR: --issue requires a number"; exit 1; }
            shift 2
            ;;
        --issue-loop)
            MODE="issue-loop"
            shift
            ;;
        --task)
            MODE="task"
            TASK_DESC="${2:-}"
            [[ -z "$TASK_DESC" ]] && { echo "ERROR: --task requires a description"; exit 1; }
            shift 2
            ;;
        --model)
            OPENCODE_MODEL="${2:-}"
            shift 2
            ;;
        --retries)
            HOST_MODE_MAX_RETRIES="${2:-}"
            shift 2
            ;;
        --timeout)
            HOST_MODE_TIMEOUT="${2:-}"
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            usage
            exit 1
            ;;
    esac
done

# ── Preflight ──
echo "[host-mode] Preflight checks..."

if ! command -v claude &>/dev/null; then
    echo "[host-mode] ERROR: 'claude' CLI not found. Install Claude Code first." >&2
    exit 1
fi

if ! command -v opencode &>/dev/null; then
    echo "[host-mode] ERROR: 'opencode' CLI not found. Install OpenCode first." >&2
    exit 1
fi

if [[ "$MODE" == "issue" || "$MODE" == "issue-loop" ]]; then
    if ! gh auth status &>/dev/null 2>&1; then
        echo "[host-mode] ERROR: GitHub CLI not authenticated. Run 'gh auth login'." >&2
        exit 1
    fi
fi

cd "$PROJECT_ROOT"

# Check for uncommitted changes (warning only)
if [[ -n "$(git diff --name-only HEAD 2>/dev/null)" ]]; then
    echo "[host-mode] WARNING: Uncommitted changes detected. Proceeding anyway."
fi

# ── Setup logging ──
mkdir -p "$LOG_DIR"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
LOG_FILE="${LOG_DIR}/run_${TIMESTAMP}.log"

# ── Export env for Claude Code agent ──
export OPENCODE_MODEL HOST_MODE_MAX_RETRIES HOST_MODE_TIMEOUT

# ── Build prompt ──
case "$MODE" in
    issue)
        PROMPT="Run host-mode issue workflow for issue #${ISSUE_NUMBER}.
Use OpenCode (model: ${OPENCODE_MODEL}) as the coding worker.
Max retries on validation failure: ${HOST_MODE_MAX_RETRIES}. OpenCode timeout: ${HOST_MODE_TIMEOUT}s.
After validation passes, commit, push, create a PR, and close the issue.
No human confirmation needed at any step."
        ;;
    issue-loop)
        PROMPT="Run host-mode continuous loop workflow.
Select open issues by priority and process them one by one until no open issues remain.
Use OpenCode (model: ${OPENCODE_MODEL}) as the coding worker.
Max retries on validation failure: ${HOST_MODE_MAX_RETRIES}. OpenCode timeout: ${HOST_MODE_TIMEOUT}s.
For each issue: implement via OpenCode, validate, commit, push, create PR, close issue, then continue to the next.
No human confirmation needed at any step."
        ;;
    task)
        PROMPT="Run host-mode task workflow for the following task:
${TASK_DESC}

Use OpenCode (model: ${OPENCODE_MODEL}) as the coding worker.
Max retries on validation failure: ${HOST_MODE_MAX_RETRIES}. OpenCode timeout: ${HOST_MODE_TIMEOUT}s.
No human confirmation needed at any step."
        ;;
esac

# ── Launch ──
echo "[host-mode] ──────────────────────────────────"
echo "[host-mode] Mode:    $MODE"
echo "[host-mode] Model:   $OPENCODE_MODEL"
echo "[host-mode] Retries: $HOST_MODE_MAX_RETRIES"
echo "[host-mode] Timeout: ${HOST_MODE_TIMEOUT}s"
echo "[host-mode] Log:     $LOG_FILE"
echo "[host-mode] ──────────────────────────────────"

claude -p "$PROMPT" \
    --agent oris-host \
    --dangerously-skip-permissions \
    --allowedTools "Bash,Read,Write,Edit,Glob,Grep" \
    2>&1 | tee "$LOG_FILE"

EXIT_CODE=${PIPESTATUS[0]}

if [[ $EXIT_CODE -eq 0 ]]; then
    echo "[host-mode] Completed successfully."
else
    echo "[host-mode] Exited with code $EXIT_CODE. Check log: $LOG_FILE"
fi

exit $EXIT_CODE
