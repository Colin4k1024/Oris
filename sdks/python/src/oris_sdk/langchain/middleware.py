from __future__ import annotations

from typing import Any, Callable

from oris_sdk.evolution.adapter import OrisEvolutionAdapter


def replay_message(instructions: list[str]) -> str:
    if not instructions:
        return "No reusable Oris experience matched this tool failure."
    lines = ["Oris found a reusable experience. Apply these steps:"]
    lines.extend(f"{idx}. {step}" for idx, step in enumerate(instructions, start=1))
    return "\n".join(lines)


def create_oris_middleware(adapter: OrisEvolutionAdapter, *, task_class: str = "agent-tool"):
    try:
        from langchain.agents.middleware import AgentMiddleware
        from langchain.messages import ToolMessage
    except ImportError as exc:
        raise ImportError(
            "LangChain support requires the optional dependency: pip install 'oris-rt-sdk[langchain]'"
        ) from exc

    class OrisEvolutionMiddleware(AgentMiddleware):
        def wrap_tool_call(self, request: Any, handler: Callable[[Any], Any]) -> Any:
            try:
                return handler(request)
            except Exception as err:
                tool_call = getattr(request, "tool_call", {}) or {}
                context = {
                    "tool_name": tool_call.get("name", ""),
                    "tool_args": tool_call.get("args", {}),
                }
                signal = adapter.detect(err, task_class=task_class, context=context)
                candidates = adapter.select(signal)
                if not candidates:
                    raise
                decision = adapter.replay(candidates[0])
                if decision.mode == "skip":
                    raise
                return ToolMessage(
                    content=replay_message(decision.instructions),
                    tool_call_id=tool_call.get("id", "oris-replay"),
                )

    return OrisEvolutionMiddleware()

