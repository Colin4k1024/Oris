from __future__ import annotations

from datetime import datetime, timezone
import hashlib
import uuid
import warnings
from typing import Any, Callable

from oris_sdk.evolution.adapter import OrisEvolutionAdapter
from oris_sdk.experience import ExperienceClient
from oris_sdk.experience_v1 import UsageReceiptV1


def replay_message(instructions: list[str]) -> str:
    if not instructions:
        return "No reusable Oris experience matched this tool failure."
    lines = ["Oris found a reusable experience. Apply these steps:"]
    lines.extend(f"{idx}. {step}" for idx, step in enumerate(instructions, start=1))
    return "\n".join(lines)


def _usage_metadata(request: Any) -> dict[str, Any] | None:
    metadata = getattr(request, "metadata", None)
    if isinstance(metadata, dict) and isinstance(metadata.get("oris_experience"), dict):
        return metadata["oris_experience"]
    return None


def create_oris_middleware(
    adapter: OrisEvolutionAdapter,
    *,
    task_class: str = "agent-tool",
    experience_client: ExperienceClient | None = None,
    evidence_provider: Callable[[Any, Any], list[str]] | None = None,
):
    try:
        from langchain.agents.middleware import AgentMiddleware
        from langchain.messages import ToolMessage
    except ImportError as exc:
        raise ImportError(
            "LangChain support requires the optional dependency: pip install 'oris-rt-sdk[langchain]'"
        ) from exc

    class OrisEvolutionMiddleware(AgentMiddleware):
        def wrap_tool_call(self, request: Any, handler: Callable[[Any], Any]) -> Any:
            usage = _usage_metadata(request)
            run_id = str(usage.get("run_id") if usage else "") or str(uuid.uuid4())
            context_hash = str(usage.get("task_context_hash") if usage else "")
            if usage and not context_hash:
                tool_call = getattr(request, "tool_call", {}) or {}
                context_hash = "sha256:" + hashlib.sha256(repr(tool_call).encode()).hexdigest()
            if usage and experience_client:
                try:
                    experience_client.begin_use(
                        str(usage["gene_id"]), int(usage.get("gene_version", 1)), run_id, context_hash
                    )
                except Exception as recorder_error:
                    warnings.warn(f"Oris begin_use could not be recorded: {recorder_error}", RuntimeWarning)
            try:
                result = handler(request)
            except Exception as err:
                if usage and experience_client:
                    try:
                        experience_client.record_outcome(UsageReceiptV1(
                            id=str(uuid.uuid4()), gene_id=str(usage["gene_id"]),
                            gene_version=int(usage.get("gene_version", 1)), agent_id=experience_client.agent_id,
                            run_id=run_id, task_context_hash=context_hash, adoption="adopted",
                            applied_step_ids=list(usage.get("applied_step_ids", [])), outcome="failed",
                            failure_reason=str(err), test_evidence_refs=[], created_at=datetime.now(timezone.utc).isoformat(),
                        ))
                    except Exception as recorder_error:
                        warnings.warn(f"Oris failure outcome could not be recorded: {recorder_error}", RuntimeWarning)
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

            if usage and experience_client:
                evidence = evidence_provider(request, result) if evidence_provider else []
                outcome = "succeeded" if evidence else "inconclusive"
                try:
                    experience_client.record_outcome(UsageReceiptV1(
                        id=str(uuid.uuid4()), gene_id=str(usage["gene_id"]),
                        gene_version=int(usage.get("gene_version", 1)), agent_id=experience_client.agent_id,
                        run_id=run_id, task_context_hash=context_hash, adoption="adopted",
                        applied_step_ids=list(usage.get("applied_step_ids", [])), outcome=outcome,
                        test_evidence_refs=evidence, created_at=datetime.now(timezone.utc).isoformat(),
                    ))
                except Exception as recorder_error:
                    warnings.warn(f"Oris success outcome could not be recorded: {recorder_error}", RuntimeWarning)
            return result

    return OrisEvolutionMiddleware()
