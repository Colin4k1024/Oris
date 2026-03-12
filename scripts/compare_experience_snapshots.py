#!/usr/bin/env python3
"""Compare builtin experience snapshots and emit release-gate drift evidence."""

from __future__ import annotations

import argparse
import hashlib
import json
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Dict, List, Tuple


@dataclass(frozen=True)
class BuiltinAsset:
    freeze_id: str
    asset_id: str
    state: str
    origin: str
    sources: Tuple[str, ...]

    def as_dict(self) -> dict:
        return {
            "freeze_id": self.freeze_id,
            "asset_id": self.asset_id,
            "state": self.state,
            "origin": self.origin,
            "sources": list(self.sources),
        }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Compare baseline/current experience snapshots using builtin-scoped "
            "freeze_id/asset_id/state keys and emit JSON evidence."
        )
    )
    parser.add_argument("--baseline", required=True, help="Baseline snapshot JSON path")
    parser.add_argument("--current", required=True, help="Current snapshot JSON path")
    parser.add_argument("--json-out", required=True, help="Output diff JSON path")
    return parser.parse_args()


def read_json(path: Path) -> dict:
    if not path.exists():
        raise FileNotFoundError(f"missing snapshot file: {path}")
    payload = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(payload, dict):
        raise ValueError(f"snapshot root must be object: {path}")
    return payload


def normalized_sources(raw_sources: object) -> Tuple[str, ...]:
    if not isinstance(raw_sources, list):
        return tuple()
    out = {
        str(item).strip().lower()
        for item in raw_sources
        if isinstance(item, str) and str(item).strip()
    }
    return tuple(sorted(out))


def parse_builtin_assets(snapshot: dict, label: str) -> Dict[str, BuiltinAsset]:
    raw_assets = snapshot.get("assets")
    if not isinstance(raw_assets, list):
        raise ValueError(f"snapshot '{label}' must contain array field 'assets'")

    out: Dict[str, BuiltinAsset] = {}
    for item in raw_assets:
        if not isinstance(item, dict):
            continue
        freeze_id = str(item.get("freeze_id", "")).strip()
        if not freeze_id:
            continue

        origin = str(item.get("origin", "")).strip().lower()
        sources = normalized_sources(item.get("sources"))
        is_builtin = origin == "builtin" or "builtin" in sources
        if not is_builtin:
            continue

        if freeze_id in out:
            raise ValueError(
                f"snapshot '{label}' has duplicate builtin freeze_id '{freeze_id}'"
            )

        out[freeze_id] = BuiltinAsset(
            freeze_id=freeze_id,
            asset_id=str(item.get("asset_id", "")).strip(),
            state=str(item.get("state", "")).strip(),
            origin=origin or "unknown",
            sources=sources,
        )

    return out


def digest_assets(assets: Dict[str, BuiltinAsset]) -> str:
    ordered = {
        freeze_id: assets[freeze_id].as_dict()
        for freeze_id in sorted(assets.keys())
    }
    canonical = json.dumps(ordered, sort_keys=True, separators=(",", ":"))
    return hashlib.sha256(canonical.encode("utf-8")).hexdigest()


def build_diff(
    baseline: Dict[str, BuiltinAsset],
    current: Dict[str, BuiltinAsset],
) -> Tuple[List[dict], List[dict], List[dict]]:
    baseline_ids = set(baseline.keys())
    current_ids = set(current.keys())

    added = [
        current[freeze_id].as_dict()
        for freeze_id in sorted(current_ids - baseline_ids)
    ]
    removed = [
        baseline[freeze_id].as_dict()
        for freeze_id in sorted(baseline_ids - current_ids)
    ]

    changed: List[dict] = []
    for freeze_id in sorted(baseline_ids & current_ids):
        left = baseline[freeze_id]
        right = current[freeze_id]
        if left.asset_id == right.asset_id and left.state == right.state:
            continue
        changed.append(
            {
                "freeze_id": freeze_id,
                "baseline": {
                    "asset_id": left.asset_id,
                    "state": left.state,
                },
                "current": {
                    "asset_id": right.asset_id,
                    "state": right.state,
                },
            }
        )

    return added, removed, changed


def main() -> int:
    args = parse_args()
    baseline_path = Path(args.baseline)
    current_path = Path(args.current)
    output_path = Path(args.json_out)

    baseline_doc = read_json(baseline_path)
    current_doc = read_json(current_path)

    baseline_assets = parse_builtin_assets(baseline_doc, "baseline")
    current_assets = parse_builtin_assets(current_doc, "current")
    added, removed, changed = build_diff(baseline_assets, current_assets)

    status = "pass" if not added and not removed and not changed else "fail"
    report = {
        "gate": "evomap-release-hardening",
        "scope": "builtin_assets",
        "status": status,
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "baseline_file": str(baseline_path),
        "current_file": str(current_path),
        "summary": {
            "baseline_count": len(baseline_assets),
            "current_count": len(current_assets),
            "added_count": len(added),
            "removed_count": len(removed),
            "changed_count": len(changed),
            "baseline_digest": digest_assets(baseline_assets),
            "current_digest": digest_assets(current_assets),
        },
        "added": added,
        "removed": removed,
        "changed": changed,
    }

    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")

    summary = report["summary"]
    print(
        "status="
        f"{status} "
        f"baseline={summary['baseline_count']} "
        f"current={summary['current_count']} "
        f"added={summary['added_count']} "
        f"removed={summary['removed_count']} "
        f"changed={summary['changed_count']}"
    )

    return 0 if status == "pass" else 2


if __name__ == "__main__":
    raise SystemExit(main())
