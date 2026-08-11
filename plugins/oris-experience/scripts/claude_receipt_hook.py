#!/usr/bin/env python3
"""Track Claude Oris sessions; never infer success from a transcript."""
from __future__ import annotations

import json
import os
import pathlib
import sys


def main() -> None:
    event = json.load(sys.stdin)
    cwd = pathlib.Path(event.get("cwd") or os.getcwd())
    state_dir = cwd / ".oris" / "agent-usage"
    state_dir.mkdir(parents=True, exist_ok=True)
    session_id = str(event.get("session_id", "unknown")).replace("/", "_")
    state_path = state_dir / f"claude-{session_id}.json"
    event_name = event.get("hook_event_name", "")
    tool_name = event.get("tool_name", "")

    if event_name == "PostToolUse" and tool_name.endswith(
        ("oris_experience_begin_use", "oris.experience.begin_use")
    ):
        state_path.write_text(json.dumps({"status": "active", "arguments": event.get("tool_input", {})}, indent=2))
    elif event_name == "PostToolUse" and tool_name.endswith(
        ("oris_experience_record_outcome", "oris.experience.record_outcome")
    ):
        state_path.write_text(json.dumps({"status": "recorded", "arguments": event.get("tool_input", {})}, indent=2))
    elif event_name == "Stop" and state_path.exists():
        state = json.loads(state_path.read_text())
        if state.get("status") == "active":
            state["status"] = "pending_inconclusive_receipt"
            state["reason"] = "Claude session ended without an evidence-backed record_outcome call"
            state_path.write_text(json.dumps(state, indent=2))
    print(json.dumps({"continue": True}))


if __name__ == "__main__":
    main()
