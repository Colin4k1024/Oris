#!/usr/bin/env python3
"""Sync issue_state/roadmap_status columns in docs/issues-roadmap.csv from GitHub."""

from __future__ import annotations

import argparse
import csv
import json
import subprocess
import sys
from typing import Dict, Iterable, List, Optional, Sequence, Set, Tuple

ACTIVE_STATUS = "active"
ARCHIVED_STATUS = "archived"


def run_command(cmd: Sequence[str]) -> str:
    result = subprocess.run(cmd, capture_output=True, text=True, check=False)
    if result.returncode != 0:
        stderr = result.stderr.strip()
        stdout = result.stdout.strip()
        message = stderr or stdout or "unknown command failure"
        raise RuntimeError(f"command failed ({' '.join(cmd)}): {message}")
    return result.stdout


def resolve_repo(explicit_repo: str) -> str:
    if explicit_repo:
        return explicit_repo

    output = run_command(["gh", "repo", "view", "--json", "nameWithOwner", "-q", ".nameWithOwner"])
    repo = output.strip()
    if not repo:
        raise RuntimeError("failed to resolve repository; pass --repo <owner/name>")
    return repo


def fetch_issue_map(repo: str, limit: int) -> Dict[str, Dict[str, str]]:
    output = run_command(
        [
            "gh",
            "issue",
            "list",
            "--repo",
            repo,
            "--state",
            "all",
            "--limit",
            str(limit),
            "--json",
            "number,state,url",
        ]
    )

    payload = json.loads(output)
    issue_map: Dict[str, Dict[str, str]] = {}
    for item in payload:
        number = str(item.get("number", "")).strip()
        if not number:
            continue

        state = str(item.get("state", "")).strip().upper()
        url = str(item.get("url", "")).strip()
        issue_map[number] = {"state": state, "url": url}

    return issue_map


def parse_issue_filter(raw_issues: str) -> Optional[Set[str]]:
    if not raw_issues:
        return None

    issues: Set[str] = set()
    for part in raw_issues.split(","):
        token = part.strip()
        if not token:
            continue
        if token.startswith("#"):
            token = token[1:]
        if not token.isdigit():
            raise ValueError(f"invalid issue number in --issues: {part!r}")
        issues.add(token)

    return issues or None


def roadmap_status_for_issue_state(issue_state: str, current_status: str) -> str:
    state = (issue_state or "").strip().upper()
    if state == "OPEN":
        return ACTIVE_STATUS
    if state == "CLOSED":
        return ARCHIVED_STATUS
    return current_status


def sync_rows(
    rows: List[Dict[str, str]],
    issue_map: Dict[str, Dict[str, str]],
    only_issue_numbers: Optional[Iterable[str]] = None,
) -> Tuple[int, List[str]]:
    allowed: Optional[Set[str]] = set(only_issue_numbers) if only_issue_numbers is not None else None
    missing: List[str] = []
    seen_missing: Set[str] = set()
    changed_count = 0

    for row in rows:
        issue_number = (row.get("issue_number") or "").strip()
        if not issue_number:
            continue
        if allowed is not None and issue_number not in allowed:
            continue

        issue = issue_map.get(issue_number)
        if issue is None:
            if issue_number not in seen_missing:
                seen_missing.add(issue_number)
                missing.append(issue_number)
            continue

        next_state = (issue.get("state") or "").strip().upper()
        next_url = (issue.get("url") or "").strip()
        next_roadmap_status = roadmap_status_for_issue_state(next_state, row.get("roadmap_status", ""))

        row_changed = False
        if (row.get("issue_state") or "").strip().upper() != next_state:
            row["issue_state"] = next_state
            row_changed = True

        current_url = (row.get("issue_url") or "").strip()
        if next_url and current_url != next_url:
            row["issue_url"] = next_url
            row_changed = True

        if (row.get("roadmap_status") or "") != next_roadmap_status:
            row["roadmap_status"] = next_roadmap_status
            row_changed = True

        if row_changed:
            changed_count += 1

    return changed_count, missing


def read_csv_rows(csv_path: str) -> Tuple[List[str], List[Dict[str, str]]]:
    with open(csv_path, "r", encoding="utf-8", newline="") as file:
        reader = csv.DictReader(file)
        if reader.fieldnames is None:
            raise RuntimeError(f"csv missing header: {csv_path}")
        rows = list(reader)
        return list(reader.fieldnames), rows


def write_csv_rows(csv_path: str, fieldnames: List[str], rows: List[Dict[str, str]]) -> None:
    with open(csv_path, "w", encoding="utf-8", newline="") as file:
        writer = csv.DictWriter(file, fieldnames=fieldnames, quoting=csv.QUOTE_ALL)
        writer.writeheader()
        writer.writerows(rows)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Sync docs/issues-roadmap.csv issue state columns from GitHub issue state."
    )
    parser.add_argument("--csv", default="docs/issues-roadmap.csv", help="Path to issues roadmap CSV")
    parser.add_argument("--repo", default="", help="GitHub repository owner/name (default: current repo)")
    parser.add_argument(
        "--issues",
        default="",
        help="Comma-separated issue numbers to sync (example: 86,87,88)",
    )
    parser.add_argument(
        "--limit",
        type=int,
        default=1000,
        help="Maximum issue count fetched from GitHub (default: 1000)",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Show sync summary without writing the CSV",
    )
    return parser


def ensure_required_columns(fieldnames: List[str]) -> None:
    required = {"issue_number", "issue_state", "roadmap_status", "issue_url"}
    missing = sorted(required.difference(fieldnames))
    if missing:
        raise RuntimeError(f"csv missing required columns: {', '.join(missing)}")


def main(argv: Optional[Sequence[str]] = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)

    try:
        issue_filter = parse_issue_filter(args.issues)
    except ValueError as error:
        print(f"error: {error}", file=sys.stderr)
        return 2

    try:
        repo = resolve_repo(args.repo)
        fieldnames, rows = read_csv_rows(args.csv)
        ensure_required_columns(fieldnames)
        issue_map = fetch_issue_map(repo, args.limit)
        changed, missing = sync_rows(rows, issue_map, issue_filter)
    except RuntimeError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1

    if missing:
        print(
            "warning: missing issues in GitHub response: " + ", ".join(f"#{number}" for number in missing),
            file=sys.stderr,
        )

    if args.dry_run:
        print(
            f"dry-run: repo={repo} csv={args.csv} changed_rows={changed} "
            f"scoped_issues={len(issue_filter) if issue_filter is not None else 'all'}"
        )
        return 0

    if changed > 0:
        write_csv_rows(args.csv, fieldnames, rows)

    print(
        f"synced: repo={repo} csv={args.csv} changed_rows={changed} "
        f"scoped_issues={len(issue_filter) if issue_filter is not None else 'all'}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
