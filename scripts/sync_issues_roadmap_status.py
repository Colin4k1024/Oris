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
ALL_TRACKS = "all"
IssueRecord = Dict[str, str]


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


def fetch_issues(repo: str, limit: int) -> List[IssueRecord]:
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
            "number,state,title,url",
        ]
    )

    payload = json.loads(output)
    issues: List[IssueRecord] = []
    for item in payload:
        number = str(item.get("number", "")).strip()
        if not number:
            continue

        issues.append(
            {
                "number": number,
                "state": str(item.get("state", "")).strip().upper(),
                "title": str(item.get("title", "")).strip(),
                "url": str(item.get("url", "")).strip(),
            }
        )

    return issues


def build_issue_maps(issues: Sequence[IssueRecord]) -> Tuple[Dict[str, IssueRecord], Dict[str, List[IssueRecord]]]:
    issue_map: Dict[str, IssueRecord] = {}
    issues_by_title: Dict[str, List[IssueRecord]] = {}
    for issue in issues:
        number = (issue.get("number") or "").strip()
        if not number:
            continue
        issue_map[number] = dict(issue)
        title = (issue.get("title") or "").strip()
        if title:
            issues_by_title.setdefault(title, []).append(dict(issue))
    return issue_map, issues_by_title


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


def parse_track_filter(raw_track: str) -> Optional[str]:
    normalized = (raw_track or "").strip()
    if not normalized or normalized.lower() == ALL_TRACKS:
        return None
    return normalized.lower()


def roadmap_status_for_issue_state(issue_state: str, current_status: str) -> str:
    state = (issue_state or "").strip().upper()
    if state == "OPEN":
        return ACTIVE_STATUS
    if state == "CLOSED":
        return ARCHIVED_STATUS
    return current_status


def sync_rows(
    rows: List[Dict[str, str]],
    issue_map: Dict[str, IssueRecord],
    issues_by_title: Optional[Dict[str, List[IssueRecord]]] = None,
    only_issue_numbers: Optional[Iterable[str]] = None,
    only_track: Optional[str] = None,
    backfill_by_title: bool = True,
) -> Tuple[int, List[str], List[str]]:
    allowed: Optional[Set[str]] = set(only_issue_numbers) if only_issue_numbers is not None else None
    track_filter = parse_track_filter(only_track or "")
    missing: List[str] = []
    seen_missing: Set[str] = set()
    ambiguous_titles: List[str] = []
    seen_ambiguous_titles: Set[str] = set()
    changed_count = 0

    for row in rows:
        if track_filter is not None:
            row_track = (row.get("roadmap_track") or "").strip().lower()
            if row_track != track_filter:
                continue

        row_changed = False
        issue_number = (row.get("issue_number") or "").strip()
        if not issue_number and backfill_by_title:
            title = (row.get("title") or "").strip()
            if title and issues_by_title is not None:
                matches = issues_by_title.get(title, [])
                if len(matches) == 1:
                    backfilled_number = (matches[0].get("number") or "").strip()
                    if backfilled_number:
                        issue_number = backfilled_number
                        if (row.get("issue_number") or "").strip() != issue_number:
                            row["issue_number"] = issue_number
                            row_changed = True
                elif len(matches) > 1:
                    if title not in seen_ambiguous_titles:
                        seen_ambiguous_titles.add(title)
                        ambiguous_titles.append(title)
                    continue

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

    return changed_count, missing, ambiguous_titles


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
        "--track",
        default=ALL_TRACKS,
        help="roadmap_track value to sync (default: all)",
    )
    parser.add_argument(
        "--backfill-by-title",
        dest="backfill_by_title",
        action="store_true",
        default=True,
        help="Backfill issue_number by exact title match when issue_number is empty",
    )
    parser.add_argument(
        "--no-backfill-by-title",
        dest="backfill_by_title",
        action="store_false",
        help=argparse.SUPPRESS,
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
        issues = fetch_issues(repo, args.limit)
        issue_map, issues_by_title = build_issue_maps(issues)
        changed, missing, ambiguous_titles = sync_rows(
            rows,
            issue_map,
            issues_by_title=issues_by_title,
            only_issue_numbers=issue_filter,
            only_track=args.track,
            backfill_by_title=args.backfill_by_title,
        )
    except RuntimeError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1

    if missing:
        print(
            "warning: missing issues in GitHub response: " + ", ".join(f"#{number}" for number in missing),
            file=sys.stderr,
        )
    if ambiguous_titles:
        print(
            "warning: ambiguous title matches skipped: " + ", ".join(ambiguous_titles),
            file=sys.stderr,
        )

    if args.dry_run:
        print(
            f"dry-run: repo={repo} csv={args.csv} changed_rows={changed} "
            f"scoped_issues={len(issue_filter) if issue_filter is not None else 'all'} "
            f"track={args.track} backfill_by_title={args.backfill_by_title}"
        )
        return 0

    if changed > 0:
        write_csv_rows(args.csv, fieldnames, rows)

    print(
        f"synced: repo={repo} csv={args.csv} changed_rows={changed} "
        f"scoped_issues={len(issue_filter) if issue_filter is not None else 'all'} "
        f"track={args.track} backfill_by_title={args.backfill_by_title}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
