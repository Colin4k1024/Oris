#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Bulk-create GitHub issues from a CSV file.

Usage:
  bash scripts/import_issues_from_csv.sh [options]

Options:
  --csv <path>             CSV file path (default: docs/issues-roadmap.csv)
  --repo <owner/name>      Target GitHub repo (default: gh current repo)
  --dry-run                Print planned actions without creating issues
  --create-milestones      Auto-create missing milestones
  --create-labels          Auto-create missing labels
  --no-skip-existing       Do not skip when same title already exists
  --help                   Show this help

CSV columns:
  title, body, labels, milestone

Examples:
  bash scripts/import_issues_from_csv.sh --dry-run
  bash scripts/import_issues_from_csv.sh --repo Colin4k1024/Oris --create-milestones --create-labels
EOF
}

csv_path="docs/issues-roadmap.csv"
repo=""
dry_run="false"
create_milestones="false"
create_labels="false"
skip_existing="true"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --csv)
      csv_path="${2:-}"
      shift 2
      ;;
    --repo)
      repo="${2:-}"
      shift 2
      ;;
    --dry-run)
      dry_run="true"
      shift
      ;;
    --create-milestones)
      create_milestones="true"
      shift
      ;;
    --create-labels)
      create_labels="true"
      shift
      ;;
    --no-skip-existing)
      skip_existing="false"
      shift
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      echo "unknown option: $1" >&2
      usage
      exit 1
      ;;
  esac
done

if [[ ! -f "$csv_path" ]]; then
  echo "csv not found: $csv_path" >&2
  exit 1
fi

if ! command -v gh >/dev/null 2>&1; then
  echo "gh is required but not found in PATH" >&2
  exit 1
fi

if ! command -v python3 >/dev/null 2>&1; then
  echo "python3 is required but not found in PATH" >&2
  exit 1
fi

if [[ -z "$repo" ]]; then
  repo="$(gh repo view --json nameWithOwner -q .nameWithOwner 2>/dev/null || true)"
fi

if [[ -z "$repo" ]]; then
  echo "failed to resolve repository; pass --repo <owner/name>" >&2
  exit 1
fi

auth_available="false"
if gh auth status >/dev/null 2>&1; then
  auth_available="true"
fi

if [[ "$dry_run" != "true" && "$auth_available" != "true" ]]; then
  echo "gh auth is required. Run: gh auth login" >&2
  exit 1
fi

if [[ "$dry_run" == "true" && "$auth_available" != "true" ]]; then
  echo "dry-run without gh auth: remote lookups (existing issues/labels/milestones) are skipped"
  skip_existing="false"
fi

b64_decode_opt="-d"
if ! printf 'dGVzdA==' | base64 -d >/dev/null 2>&1; then
  b64_decode_opt="-D"
fi

decode_b64() {
  printf '%s' "$1" | base64 "$b64_decode_opt"
}

trim() {
  local s="$1"
  s="${s#"${s%%[![:space:]]*}"}"
  s="${s%"${s##*[![:space:]]}"}"
  printf '%s' "$s"
}

has_line() {
  local needle="$1"
  local file="$2"
  [[ -n "$needle" ]] && grep -Fxq -- "$needle" "$file"
}

append_line_if_missing() {
  local line="$1"
  local file="$2"
  if ! has_line "$line" "$file"; then
    printf '%s\n' "$line" >> "$file"
  fi
}

tmp_titles="$(mktemp)"
tmp_milestones="$(mktemp)"
tmp_labels="$(mktemp)"
trap 'rm -f "$tmp_titles" "$tmp_milestones" "$tmp_labels"' EXIT

: > "$tmp_titles"
: > "$tmp_milestones"
: > "$tmp_labels"

if [[ "$auth_available" == "true" ]]; then
  if [[ "$skip_existing" == "true" ]]; then
    gh issue list --repo "$repo" --state all --limit 1000 --json title -q '.[].title' > "$tmp_titles" || true
  fi
  gh api "repos/$repo/milestones?state=all&per_page=100" --jq '.[].title' > "$tmp_milestones" || true
  gh label list --repo "$repo" --limit 1000 --json name -q '.[].name' > "$tmp_labels" || true
fi

created=0
skipped=0
failed=0

while IFS=$'\t' read -r title_b64 body_b64 labels_b64 milestone_b64; do
  title="$(decode_b64 "$title_b64")"
  body="$(decode_b64 "$body_b64")"
  labels_csv="$(decode_b64 "$labels_b64")"
  milestone="$(decode_b64 "$milestone_b64")"

  if [[ -z "$title" ]]; then
    continue
  fi

  if [[ "$skip_existing" == "true" ]] && has_line "$title" "$tmp_titles"; then
    echo "skip existing: $title"
    skipped=$((skipped + 1))
    continue
  fi

  if [[ -n "$milestone" ]] && ! has_line "$milestone" "$tmp_milestones"; then
    if [[ "$create_milestones" == "true" ]]; then
      if [[ "$dry_run" == "true" ]]; then
        echo "dry-run create milestone: $milestone"
      else
        gh api --method POST "repos/$repo/milestones" -f "title=$milestone" >/dev/null
      fi
      append_line_if_missing "$milestone" "$tmp_milestones"
    else
      echo "missing milestone (skip issue): $title :: $milestone" >&2
      failed=$((failed + 1))
      continue
    fi
  fi

  label_args=()
  if [[ -n "$labels_csv" ]]; then
    IFS=',' read -r -a labels_arr <<< "$labels_csv"
    for raw_label in "${labels_arr[@]}"; do
      label="$(trim "$raw_label")"
      [[ -z "$label" ]] && continue
      if ! has_line "$label" "$tmp_labels"; then
        if [[ "$create_labels" == "true" ]]; then
          if [[ "$dry_run" == "true" ]]; then
            echo "dry-run create label: $label"
          else
            gh label create "$label" --repo "$repo" --color "0052cc" --description "Roadmap label" >/dev/null
          fi
          append_line_if_missing "$label" "$tmp_labels"
        else
          echo "missing label (skip issue): $title :: $label" >&2
          failed=$((failed + 1))
          continue 2
        fi
      fi
      label_args+=("--label" "$label")
    done
  fi

  cmd=(gh issue create --repo "$repo" --title "$title" --body "$body")
  if [[ -n "$milestone" ]]; then
    cmd+=("--milestone" "$milestone")
  fi
  cmd+=("${label_args[@]}")

  if [[ "$dry_run" == "true" ]]; then
    printf 'dry-run create issue: '
    printf '%q ' "${cmd[@]}"
    printf '\n'
    created=$((created + 1))
    continue
  fi

  if output="$("${cmd[@]}" 2>&1)"; then
    echo "created: $title -> $output"
    append_line_if_missing "$title" "$tmp_titles"
    created=$((created + 1))
  else
    echo "failed: $title :: $output" >&2
    failed=$((failed + 1))
  fi
done < <(
  python3 - "$csv_path" <<'PY'
import base64
import csv
import sys

path = sys.argv[1]
with open(path, "r", encoding="utf-8", newline="") as f:
    reader = csv.DictReader(f)
    for row in reader:
        cols = []
        for key in ("title", "body", "labels", "milestone"):
            value = row.get(key, "") or ""
            cols.append(base64.b64encode(value.encode("utf-8")).decode("ascii"))
        print("\t".join(cols))
PY
)

echo "summary: planned_or_created=$created skipped=$skipped failed=$failed repo=$repo"
