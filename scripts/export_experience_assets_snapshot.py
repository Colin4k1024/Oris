#!/usr/bin/env python3
"""Export built-in + runtime experience assets snapshot (JSON + Markdown)."""

from __future__ import annotations

import argparse
import json
import re
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Dict, List, Optional, Sequence, Tuple


@dataclass
class GeneRecord:
    id: str
    signals: List[str]
    strategy: List[str]
    validation: List[str]
    state: str


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Export full experience asset details and finalized freeze IDs."
    )
    parser.add_argument(
        "--repo-root",
        default=str(Path(__file__).resolve().parent.parent),
        help="Repository root path",
    )
    parser.add_argument(
        "--core-file",
        default="crates/oris-evokernel/src/core.rs",
        help="Path (relative to repo root) to oris-evokernel core.rs",
    )
    parser.add_argument(
        "--store-dir",
        default="crates/oris-runtime/.oris/evolution",
        help="Path (relative to repo root) to runtime evolution store directory",
    )
    parser.add_argument(
        "--json-out",
        default="crates/oris-runtime/examples/assets/experience_assets_snapshot.json",
        help="Path (relative to repo root) for JSON snapshot output",
    )
    parser.add_argument(
        "--md-out",
        default="crates/oris-runtime/examples/assets/experience_assets_snapshot.md",
        help="Path (relative to repo root) for Markdown snapshot output",
    )
    return parser.parse_args()


def read_text(path: Path) -> str:
    if not path.exists():
        raise FileNotFoundError(f"missing required file: {path}")
    return path.read_text(encoding="utf-8")


def read_json_array(path: Path) -> List[dict]:
    if not path.exists():
        return []
    raw = path.read_text(encoding="utf-8").strip()
    if not raw:
        return []
    data = json.loads(raw)
    if not isinstance(data, list):
        raise ValueError(f"expected array JSON in {path}")
    return data


def read_events_jsonl(path: Path) -> List[dict]:
    if not path.exists():
        return []
    out: List[dict] = []
    with path.open("r", encoding="utf-8") as f:
        for lineno, line in enumerate(f, start=1):
            line = line.strip()
            if not line:
                continue
            try:
                out.append(json.loads(line))
            except json.JSONDecodeError as exc:
                raise ValueError(f"invalid jsonl line {lineno} in {path}: {exc}") from exc
    return out


def find_matching_brace(text: str, open_index: int) -> int:
    depth = 0
    for i in range(open_index, len(text)):
        ch = text[i]
        if ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
            if depth == 0:
                return i
    raise ValueError("unbalanced braces while parsing Rust source")


def extract_builtin_function_body(core_text: str) -> str:
    marker = "fn built_in_experience_genes() -> Vec<Gene> {"
    start = core_text.find(marker)
    if start < 0:
        raise ValueError("failed to locate built_in_experience_genes()")
    open_brace = core_text.find("{", start)
    close_brace = find_matching_brace(core_text, open_brace)
    return core_text[open_brace + 1 : close_brace]


def extract_gene_blocks(function_body: str) -> List[str]:
    blocks: List[str] = []
    i = 0
    marker = "Gene {"
    while True:
        start = function_body.find(marker, i)
        if start < 0:
            break
        brace_open = function_body.find("{", start)
        brace_close = find_matching_brace(function_body, brace_open)
        blocks.append(function_body[start : brace_close + 1])
        i = brace_close + 1
    return blocks


def parse_into_strings(expr_block: str) -> List[str]:
    return re.findall(r'"([^"]+)"\.into\(\)', expr_block, flags=re.DOTALL)


def parse_field_block(block: str, field_name: str, next_field_name: str) -> str:
    pattern = re.compile(
        rf"{re.escape(field_name)}\s*:\s*vec!\[(.*?)\],\s*{re.escape(next_field_name)}\s*:",
        flags=re.DOTALL,
    )
    match = pattern.search(block)
    if not match:
        raise ValueError(f"failed to parse field '{field_name}'")
    return match.group(1)


def parse_builtin_genes(core_text: str) -> List[GeneRecord]:
    body = extract_builtin_function_body(core_text)
    genes: List[GeneRecord] = []
    for block in extract_gene_blocks(body):
        id_match = re.search(r'id:\s*"([^"]+)"\.into\(\)', block)
        if not id_match:
            raise ValueError("failed to parse gene id from built_in_experience_genes block")
        gene_id = id_match.group(1)

        signals_block = parse_field_block(block, "signals", "strategy")
        strategy_block = parse_field_block(block, "strategy", "validation")
        validation_block = parse_field_block(block, "validation", "state")
        state_match = re.search(r"state:\s*AssetState::([A-Za-z_][A-Za-z0-9_]*)", block)
        if not state_match:
            raise ValueError(f"failed to parse state for gene {gene_id}")

        genes.append(
            GeneRecord(
                id=gene_id,
                signals=parse_into_strings(signals_block),
                strategy=parse_into_strings(strategy_block),
                validation=parse_into_strings(validation_block),
                state=state_match.group(1),
            )
        )
    return genes


def strategy_map(entries: Sequence[str]) -> Dict[str, str]:
    out: Dict[str, str] = {}
    for entry in entries:
        if "=" not in entry:
            continue
        key, value = entry.split("=", 1)
        key = key.strip()
        value = value.strip()
        if key and value and key not in out:
            out[key] = value
    return out


def strategy_values(entries: Sequence[str], key: str) -> List[str]:
    out: List[str] = []
    for entry in entries:
        if "=" not in entry:
            continue
        current_key, current_value = entry.split("=", 1)
        if current_key.strip() == key and current_value.strip():
            out.append(current_value.strip())
    return out


def normalize_gene(raw: dict) -> GeneRecord:
    return GeneRecord(
        id=str(raw.get("id", "")).strip(),
        signals=[str(v) for v in raw.get("signals", []) if str(v).strip()],
        strategy=[str(v) for v in raw.get("strategy", []) if str(v).strip()],
        validation=[str(v) for v in raw.get("validation", []) if str(v).strip()],
        state=str(raw.get("state", "")).strip() or "Unknown",
    )


def extract_last_promoted_events(events: Sequence[dict]) -> Dict[str, Tuple[int, str]]:
    latest: Dict[str, Tuple[int, str]] = {}
    for record in events:
        seq_raw = record.get("seq")
        if not isinstance(seq_raw, int):
            continue
        timestamp = str(record.get("timestamp", "")).strip()
        event = record.get("event")
        if not isinstance(event, dict):
            continue
        kind = str(event.get("kind", "")).strip()
        gene_id: Optional[str] = None
        if kind == "gene_promoted":
            current_gene_id = event.get("gene_id")
            if isinstance(current_gene_id, str):
                gene_id = current_gene_id.strip()
        elif kind == "promotion_evaluated":
            state = str(event.get("state", "")).strip().lower()
            if state == "promoted":
                current_gene_id = event.get("gene_id")
                if isinstance(current_gene_id, str):
                    gene_id = current_gene_id.strip()
        if not gene_id:
            continue
        prev = latest.get(gene_id)
        if prev is None or seq_raw > prev[0]:
            latest[gene_id] = (seq_raw, timestamp)
    return latest


def merge_sources(current: Sequence[str], new_source: str) -> List[str]:
    out = list(current)
    if new_source not in out:
        out.append(new_source)
    # Keep deterministic order: builtin first, runtime_store second.
    rank = {"builtin": 0, "runtime_store": 1}
    out.sort(key=lambda item: rank.get(item, 99))
    return out


def choose_origin(meta: Dict[str, str], sources: Sequence[str]) -> str:
    if "asset_origin" in meta:
        return meta["asset_origin"]
    if "builtin" in sources:
        return "builtin"
    if "runtime_store" in sources:
        return "runtime_store"
    return "unknown"


def merge_gene_records(
    builtin_genes: Sequence[GeneRecord], store_genes: Sequence[GeneRecord]
) -> Dict[str, dict]:
    merged: Dict[str, dict] = {}

    for gene in builtin_genes:
        if not gene.id:
            continue
        merged[gene.id] = {
            "gene": gene,
            "sources": ["builtin"],
        }

    for gene in store_genes:
        if not gene.id:
            continue
        if gene.id in merged:
            prior_gene: GeneRecord = merged[gene.id]["gene"]
            merged_gene = GeneRecord(
                id=gene.id,
                signals=gene.signals or prior_gene.signals,
                strategy=gene.strategy or prior_gene.strategy,
                validation=gene.validation or prior_gene.validation,
                state=gene.state or prior_gene.state,
            )
            merged[gene.id]["gene"] = merged_gene
            merged[gene.id]["sources"] = merge_sources(
                merged[gene.id]["sources"], "runtime_store"
            )
        else:
            merged[gene.id] = {
                "gene": gene,
                "sources": ["runtime_store"],
            }

    return merged


def build_capsule_index(capsules: Sequence[dict]) -> Dict[str, List[dict]]:
    out: Dict[str, List[dict]] = {}
    for capsule in capsules:
        gene_id = str(capsule.get("gene_id", "")).strip()
        capsule_id = str(capsule.get("id", "")).strip()
        if not gene_id or not capsule_id:
            continue
        item = {
            "capsule_id": capsule_id,
            "gene_id": gene_id,
            "mutation_id": capsule.get("mutation_id"),
            "run_id": capsule.get("run_id"),
            "confidence": capsule.get("confidence"),
            "state": capsule.get("state"),
            "outcome_success": (
                capsule.get("outcome", {}).get("success")
                if isinstance(capsule.get("outcome"), dict)
                else None
            ),
            "source_type": "store",
        }
        out.setdefault(gene_id, []).append(item)
    for values in out.values():
        values.sort(key=lambda item: (str(item["capsule_id"]), str(item.get("run_id") or "")))
    return out


def build_snapshot(
    builtin_genes: Sequence[GeneRecord],
    store_genes: Sequence[GeneRecord],
    store_capsules: Sequence[dict],
    events: Sequence[dict],
) -> dict:
    merged = merge_gene_records(builtin_genes, store_genes)
    capsule_index = build_capsule_index(store_capsules)
    promoted_events = extract_last_promoted_events(events)

    assets: List[dict] = []
    for asset_id in sorted(merged.keys()):
        payload = merged[asset_id]
        gene: GeneRecord = payload["gene"]
        sources: List[str] = payload["sources"]
        meta = strategy_map(gene.strategy)
        refs = strategy_values(gene.strategy, "source_capsule")
        capsules = list(capsule_index.get(asset_id, []))

        capsule_ref_only = False
        if not capsules and refs:
            capsule_ref_only = True
            for ref in sorted(set(refs)):
                capsules.append(
                    {
                        "capsule_id": ref,
                        "gene_id": asset_id,
                        "mutation_id": None,
                        "run_id": None,
                        "confidence": None,
                        "state": None,
                        "outcome_success": None,
                        "source_type": "strategy_ref",
                    }
                )

        state = gene.state or "Unknown"
        finalized = state.lower() == "promoted"
        promoted_meta = promoted_events.get(asset_id)

        assets.append(
            {
                "asset_id": asset_id,
                "freeze_id": asset_id,
                "state": state,
                "finalized": finalized,
                "origin": choose_origin(meta, sources),
                "sources": sources,
                "task_class": meta.get("task_class"),
                "task_label": meta.get("task_label"),
                "template_id": meta.get("template_id"),
                "summary": meta.get("summary"),
                "signals": gene.signals,
                "strategy": gene.strategy,
                "validation": gene.validation,
                "capsule_ref_only": capsule_ref_only,
                "capsules": capsules,
                "last_promoted_event_seq": promoted_meta[0] if promoted_meta else None,
                "last_promoted_at": promoted_meta[1] if promoted_meta else None,
            }
        )

    finalized = [
        {
            "freeze_id": asset["freeze_id"],
            "asset_id": asset["asset_id"],
            "task_class": asset["task_class"],
            "origin": asset["origin"],
            "last_promoted_event_seq": asset["last_promoted_event_seq"],
        }
        for asset in assets
        if asset["finalized"]
    ]
    finalized.sort(
        key=lambda item: (
            item["last_promoted_event_seq"] is None,
            -(item["last_promoted_event_seq"] or -1),
            item["asset_id"],
        )
    )

    return {
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "scope": "builtin_plus_runtime_store",
        "freeze_id_rule": "gene.id",
        "asset_count": len(assets),
        "finalized_count": len(finalized),
        "assets": assets,
        "finalized_experiences": finalized,
    }


def markdown_list(items: Sequence[object]) -> str:
    if not items:
        return "-"
    return ", ".join(str(item) for item in items)


def render_markdown(snapshot: dict) -> str:
    lines: List[str] = []
    lines.append("# Experience Assets Snapshot")
    lines.append("")
    lines.append(f"- generated_at: `{snapshot['generated_at']}`")
    lines.append(f"- scope: `{snapshot['scope']}`")
    lines.append(f"- freeze_id_rule: `{snapshot['freeze_id_rule']}`")
    lines.append(f"- asset_count: `{snapshot['asset_count']}`")
    lines.append(f"- finalized_count: `{snapshot['finalized_count']}`")
    lines.append("")
    lines.append("## Finalized Experiences")
    lines.append("")
    lines.append("| freeze_id | asset_id | task_class | origin | last_promoted_event_seq |")
    lines.append("| --- | --- | --- | --- | --- |")
    for item in snapshot["finalized_experiences"]:
        lines.append(
            f"| `{item['freeze_id']}` | `{item['asset_id']}` | `{item.get('task_class') or '-'}` "
            f"| `{item.get('origin') or '-'}` | `{item.get('last_promoted_event_seq')}` |"
        )
    lines.append("")
    lines.append("## Asset Details")
    lines.append("")

    for asset in snapshot["assets"]:
        lines.append(f"### {asset['asset_id']}")
        lines.append("")
        lines.append(f"- freeze_id: `{asset['freeze_id']}`")
        lines.append(f"- state: `{asset['state']}`")
        lines.append(f"- finalized: `{asset['finalized']}`")
        lines.append(f"- origin: `{asset['origin']}`")
        lines.append(f"- sources: {markdown_list(asset['sources'])}")
        lines.append(f"- task_class: `{asset.get('task_class') or '-'}`")
        lines.append(f"- task_label: `{asset.get('task_label') or '-'}`")
        lines.append(f"- template_id: `{asset.get('template_id') or '-'}`")
        lines.append(f"- summary: `{asset.get('summary') or '-'}`")
        lines.append(
            f"- last_promoted_event_seq: `{asset.get('last_promoted_event_seq')}`"
        )
        lines.append(f"- last_promoted_at: `{asset.get('last_promoted_at')}`")
        lines.append(f"- capsule_ref_only: `{asset.get('capsule_ref_only')}`")
        lines.append(f"- signals: {markdown_list(asset['signals'])}")
        lines.append("- strategy:")
        if asset["strategy"]:
            for entry in asset["strategy"]:
                lines.append(f"  - `{entry}`")
        else:
            lines.append("  - `-`")
        lines.append("- validation:")
        if asset["validation"]:
            for entry in asset["validation"]:
                lines.append(f"  - `{entry}`")
        else:
            lines.append("  - `-`")

        lines.append("- capsules:")
        if asset["capsules"]:
            for capsule in asset["capsules"]:
                lines.append(
                    "  - "
                    f"capsule_id=`{capsule.get('capsule_id')}`, "
                    f"source_type=`{capsule.get('source_type')}`, "
                    f"gene_id=`{capsule.get('gene_id')}`, "
                    f"mutation_id=`{capsule.get('mutation_id')}`, "
                    f"run_id=`{capsule.get('run_id')}`, "
                    f"confidence=`{capsule.get('confidence')}`, "
                    f"state=`{capsule.get('state')}`, "
                    f"outcome_success=`{capsule.get('outcome_success')}`"
                )
        else:
            lines.append("  - `-`")
        lines.append("")

    return "\n".join(lines).strip() + "\n"


def write_text(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


def main() -> int:
    args = parse_args()
    repo_root = Path(args.repo_root).resolve()

    core_file = (repo_root / args.core_file).resolve()
    store_dir = (repo_root / args.store_dir).resolve()
    json_out = (repo_root / args.json_out).resolve()
    md_out = (repo_root / args.md_out).resolve()

    core_text = read_text(core_file)
    builtin_genes = parse_builtin_genes(core_text)
    store_genes = [normalize_gene(raw) for raw in read_json_array(store_dir / "genes.json")]
    store_capsules = read_json_array(store_dir / "capsules.json")
    store_events = read_events_jsonl(store_dir / "events.jsonl")

    snapshot = build_snapshot(builtin_genes, store_genes, store_capsules, store_events)
    write_text(json_out, json.dumps(snapshot, indent=2, ensure_ascii=False) + "\n")
    write_text(md_out, render_markdown(snapshot))

    print(f"wrote JSON snapshot: {json_out}")
    print(f"wrote Markdown snapshot: {md_out}")
    print(f"asset_count={snapshot['asset_count']} finalized_count={snapshot['finalized_count']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
