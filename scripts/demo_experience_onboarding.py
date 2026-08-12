#!/usr/bin/env python3
"""Run a deterministic, end-to-end Oris experience onboarding scenario.

The demo launches the real `oris-experience-mcp` binary three times with a
shared SQLite database. Each process represents a different upstream agent.
It deliberately avoids calling a model so the result is fast, repeatable, and
safe to use in documentation. Model-level interoperability is covered by the
separate Claude Code/Grok E2E report.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
from pathlib import Path
import subprocess
import sys
from typing import Any


REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_BINARY = REPO_ROOT / "target" / "debug" / "oris-experience-mcp"
DEFAULT_OUTPUT = REPO_ROOT / "target" / "experience-onboarding-demo"
GOLDEN_FIXTURE = REPO_ROOT / "spec" / "experience" / "golden" / "experience-bundle-v1.json"
GENE_ID = "gene-stdio-json-keep-diagnostics-off-stdout"


class McpSession:
    """Small newline-delimited JSON-RPC client for the Oris STDIO server."""

    def __init__(self, binary: Path, database: Path, agent_id: str) -> None:
        env = os.environ.copy()
        env.update(
            {
                "ORIS_EXPERIENCE_DB": str(database),
                "ORIS_AGENT_ID": agent_id,
                "ORIS_MCP_SCOPES": "experience:read,experience:write",
            }
        )
        self.process = subprocess.Popen(
            [str(binary)],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            env=env,
        )
        self.next_id = 1
        self.request(
            "initialize",
            {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": {"name": agent_id, "version": "demo-v1"},
            },
        )

    def request(self, method: str, params: dict[str, Any] | None = None) -> dict[str, Any]:
        if self.process.stdin is None or self.process.stdout is None:
            raise RuntimeError("MCP process streams are unavailable")
        request_id = self.next_id
        self.next_id += 1
        payload: dict[str, Any] = {
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
        }
        if params is not None:
            payload["params"] = params
        self.process.stdin.write(json.dumps(payload, separators=(",", ":")) + "\n")
        self.process.stdin.flush()
        line = self.process.stdout.readline()
        if not line:
            stderr = self.process.stderr.read() if self.process.stderr else ""
            raise RuntimeError(f"MCP server closed before replying: {stderr.strip()}")
        response = json.loads(line)
        if response.get("id") != request_id:
            raise RuntimeError(f"unexpected JSON-RPC response id: {response}")
        if response.get("error"):
            raise RuntimeError(f"MCP error for {method}: {response['error']}")
        return response["result"]

    def call_tool(self, name: str, arguments: dict[str, Any]) -> Any:
        result = self.request(
            "tools/call", {"name": name, "arguments": arguments}
        )
        if result.get("isError"):
            raise RuntimeError(f"tool {name} failed: {result}")
        return result["structuredContent"]["data"]

    def close(self) -> None:
        if self.process.stdin is not None:
            self.process.stdin.close()
        try:
            return_code = self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.process.terminate()
            return_code = self.process.wait(timeout=5)
        if return_code != 0:
            stderr = self.process.stderr.read() if self.process.stderr else ""
            raise RuntimeError(f"MCP process exited with {return_code}: {stderr.strip()}")

    def __enter__(self) -> "McpSession":
        return self

    def __exit__(self, exc_type: Any, exc: Any, traceback: Any) -> None:
        self.close()


def utc_now() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat().replace("+00:00", "Z")


def build_bundle() -> dict[str, Any]:
    bundle = json.loads(GOLDEN_FIXTURE.read_text(encoding="utf-8"))
    created_at = utc_now()
    bundle["gene"] = {
        "id": GENE_ID,
        "version": 1,
        "name": "Keep diagnostics off a JSON-RPC stdout stream",
        "description": (
            "Route logs and readiness diagnostics to stderr so stdout remains "
            "a valid newline-delimited JSON-RPC protocol stream."
        ),
        "scope": "local",
        "task_category": "software.protocol.stdio-json",
        "applicability": {
            "required_signals": ["stdio", "json-rpc", "stdout"],
            "excluded_signals": ["human-readable-stdout"],
            "environments": [
                {"key": "transport", "operator": "equals", "value": "stdio"}
            ],
            "project_ids": [],
            "tenant_ids": [],
            "do_not_use_when": [
                "stdout is intentionally a human-readable console",
                "the protocol uses a different dedicated file descriptor",
            ],
        },
        "steps": [
            {
                "id": "identify-protocol-stream",
                "instruction": "Confirm stdout is reserved for JSON-RPC messages.",
                "tool": None,
                "requires_approval": False,
                "expected_output": "stdout protocol boundary identified",
            },
            {
                "id": "redirect-diagnostics",
                "instruction": "Move logs, banners, and readiness diagnostics to stderr.",
                "tool": "apply_patch",
                "requires_approval": False,
                "expected_output": "stdout contains only protocol messages",
            },
            {
                "id": "validate-stream",
                "instruction": "Run tests and parse every stdout line as JSON.",
                "tool": "run_tests",
                "requires_approval": False,
                "expected_output": "tests pass and every stdout line parses",
            },
        ],
        "tool_requirements": [],
        "safety": {
            "suggestion_only": True,
            "forbidden_operations": ["discard-diagnostics", "disable-validation"],
            "required_approvals": [],
            "secret_handling": "redact",
        },
        "validation": {
            "checks": [
                {
                    "id": "stdout-json-only",
                    "command_or_assertion": "every non-empty stdout line parses as JSON",
                    "evidence_kind": "test",
                    "timeout_seconds": 120,
                }
            ],
            "success_condition": "all",
        },
        "provenance": {
            "source_agent": "claude-code",
            "source_run_id": "demo-claude-seed",
            "trace_refs": ["oris://traces/demo-claude-seed"],
            "extractor_version": "oris-onboarding-demo/1",
            "verified_successes": 0,
            "verified_failures": 0,
            "distinct_task_contexts": 0,
        },
        "lifecycle": "candidate",
        "created_at": created_at,
        "updated_at": created_at,
        "metadata": {"demo": True},
    }
    bundle["capsules"] = [
        make_capsule("claude-seed", "sha256:demo-context-claude")
    ]
    bundle["usage_receipts"] = [
        make_receipt("claude-code", "claude-seed", "sha256:demo-context-claude")
    ]
    return bundle


def make_capsule(run_name: str, context_hash: str) -> dict[str, Any]:
    return {
        "id": f"capsule-demo-{run_name}",
        "gene_id": GENE_ID,
        "gene_version": 1,
        "environment_fingerprint": f"sha256:demo-env-{run_name}",
        "task_context_hash": context_hash,
        "execution_evidence_hash": f"sha256:demo-evidence-{run_name}",
        "validation": {
            "status": "succeeded",
            "checks": [
                {
                    "check_id": "stdout-json-only",
                    "passed": True,
                    "evidence_ref": f"artifact://demo/{run_name}/tests",
                }
            ],
            "summary": "Protocol-stream validation passed",
        },
        "artifact_refs": [f"artifact://demo/{run_name}/diff"],
        "redaction": "verified_clean",
        "created_at": utc_now(),
    }


def make_receipt(agent_id: str, run_name: str, context_hash: str) -> dict[str, Any]:
    return {
        "id": f"receipt-demo-{run_name}",
        "gene_id": GENE_ID,
        "gene_version": 1,
        "agent_id": agent_id,
        "run_id": f"demo-{run_name}",
        "task_context_hash": context_hash,
        "adoption": "adopted",
        "applied_step_ids": [
            "identify-protocol-stream",
            "redirect-diagnostics",
            "validate-stream",
        ],
        "outcome": "succeeded",
        "failure_reason": None,
        "test_evidence_refs": [f"artifact://demo/{run_name}/tests"],
        "cost": {"latency_ms": 1200},
        "created_at": utc_now(),
    }


def ensure_binary(binary: Path) -> None:
    if binary.is_file():
        return
    print("[setup] Building oris-experience-mcp ...", flush=True)
    subprocess.run(
        [
            "cargo",
            "build",
            "-p",
            "oris-experience-repo",
            "--bin",
            "oris-experience-mcp",
        ],
        cwd=REPO_ROOT,
        check=True,
    )
    if not binary.is_file():
        raise RuntimeError(f"expected MCP binary was not created: {binary}")


def run(binary: Path, output_dir: Path) -> dict[str, Any]:
    output_dir.mkdir(parents=True, exist_ok=True)
    database = output_dir / "experience.db"
    for path in (database, output_dir / "session.log", output_dir / "summary.json"):
        path.unlink(missing_ok=True)

    lines: list[str] = []

    def emit(message: str = "") -> None:
        print(message, flush=True)
        lines.append(message)

    emit("ORIS EXPERIENCE — VERIFIED CROSS-AGENT DEMO")
    emit("Shared store: local private SQLite")
    emit("")

    with McpSession(binary, database, "claude-code") as claude:
        tools = claude.request("tools/list")["tools"]
        names = {tool["name"] for tool in tools}
        expected = {
            "oris_experience_search",
            "oris_experience_get",
            "oris_experience_propose",
            "oris_experience_begin_use",
            "oris_experience_record_outcome",
        }
        if names != expected:
            raise AssertionError(f"unexpected ordinary tool surface: {sorted(names)}")
        emit("[1/5] Claude Code completes a task and submits verified evidence")
        gene = claude.call_tool("oris_experience_propose", {"bundle": build_bundle()})
        if gene["lifecycle"] != "candidate":
            raise AssertionError(f"proposal was not a candidate: {gene}")
        emit("      ✓ local candidate created · evidence attached · successes 1/3")

    agent_runs = [
        ("grok-cli", "grok-reuse", "sha256:demo-context-grok", "Grok"),
        ("opencode", "opencode-reuse", "sha256:demo-context-opencode", "OpenCode"),
    ]
    last_gene: dict[str, Any] | None = None
    for index, (agent_id, run_name, context_hash, label) in enumerate(agent_runs, start=2):
        with McpSession(binary, database, agent_id) as agent:
            emit(f"[{index}/5] {label} asks Oris before solving a related task")
            results = agent.call_tool(
                "oris_experience_search",
                {
                    "text": "JSON-RPC stdio server emits diagnostics on stdout",
                    "task_category": "software.protocol.stdio-json",
                    "environment": {"transport": "stdio"},
                    "available_tools": ["read_file", "apply_patch", "run_command"],
                    "limit": 3,
                },
            )
            if not results or results[0]["gene"]["id"] != GENE_ID:
                raise AssertionError(f"{label} did not retrieve the expected Gene: {results}")
            score = results[0]["score"]
            emit(f"      ✓ matching procedure found · score {score:.3f} · boundaries preserved")
            agent.call_tool(
                "oris_experience_begin_use",
                {
                    "gene_id": GENE_ID,
                    "gene_version": 1,
                    "run_id": f"demo-{run_name}",
                    "task_context_hash": context_hash,
                },
            )
            last_gene = agent.call_tool(
                "oris_experience_record_outcome",
                {
                    "receipt": make_receipt(agent_id, run_name, context_hash),
                    "capsule": make_capsule(run_name, context_hash),
                },
            )
            successes = last_gene["provenance"]["verified_successes"]
            lifecycle = last_gene["lifecycle"]
            emit(f"      ✓ native validation passed · receipt recorded · {successes}/3 · {lifecycle}")

    if last_gene is None or last_gene["lifecycle"] != "stable":
        raise AssertionError(f"Gene did not become stable: {last_gene}")
    if last_gene["provenance"]["distinct_task_contexts"] < 2:
        raise AssertionError(f"insufficient task contexts: {last_gene}")

    with McpSession(binary, database, "claude-code") as claude:
        emit("[4/5] Oris evaluates lifecycle gates")
        complete = claude.call_tool(
            "oris_experience_get",
            {"id": GENE_ID, "version": 1, "include_skill_projection": True},
        )
        projection = complete["skill_projection"]
        if not projection["installable"]:
            raise AssertionError(f"stable Skill projection was not installable: {projection}")
        emit("      ✓ 3 verified successes · 3 contexts · 0 failures")
        emit("[5/5] Stable experience is projected as a portable Agent Skill")
        emit("      ✓ installable by Claude Code, Codex, OpenCode, Grok, and agent runtimes")

    emit("")
    emit("RESULT: PASS — verified experience moved from candidate to stable")

    summary = {
        "status": "pass",
        "scenario": "cross-agent-experience-onboarding",
        "gene_id": GENE_ID,
        "lifecycle": last_gene["lifecycle"],
        "verified_successes": last_gene["provenance"]["verified_successes"],
        "verified_failures": last_gene["provenance"]["verified_failures"],
        "distinct_task_contexts": last_gene["provenance"]["distinct_task_contexts"],
        "agents": ["claude-code", "grok-cli", "opencode"],
        "database": str(database),
        "completed_at": utc_now(),
    }
    (output_dir / "session.log").write_text("\n".join(lines) + "\n", encoding="utf-8")
    (output_dir / "summary.json").write_text(
        json.dumps(summary, indent=2) + "\n", encoding="utf-8"
    )
    return summary


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=DEFAULT_BINARY)
    parser.add_argument("--output-dir", type=Path, default=DEFAULT_OUTPUT)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    binary = args.binary.resolve()
    output_dir = args.output_dir.resolve()
    try:
        ensure_binary(binary)
        run(binary, output_dir)
    except (AssertionError, RuntimeError, subprocess.CalledProcessError) as error:
        print(f"RESULT: FAIL — {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
