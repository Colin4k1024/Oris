import pytest

from datetime import datetime, timezone
from types import SimpleNamespace

from oris_sdk.evolution import OrisEvolutionAdapter
from oris_sdk.gene import Gene
from oris_sdk.store import LocalStore
from oris_sdk.langchain.middleware import create_oris_middleware, replay_message


def test_replay_message_formats_steps() -> None:
    message = replay_message(["inspect the traceback", "rerun the failing test"])
    assert "Oris found a reusable experience" in message
    assert "1. inspect the traceback" in message
    assert "2. rerun the failing test" in message


def test_create_oris_middleware_reports_missing_optional_dependency() -> None:
    try:
        middleware = create_oris_middleware(object())  # type: ignore[arg-type]
    except ImportError as exc:
        assert "oris-rt-sdk[langchain]" in str(exc)
    else:
        assert hasattr(middleware, "wrap_tool_call")


def test_langchain_middleware_returns_tool_message_for_known_failure(tmp_path) -> None:
    tool_message_cls = pytest.importorskip("langchain.messages").ToolMessage
    store = LocalStore(str(tmp_path / "genes.db"))
    try:
        adapter = OrisEvolutionAdapter(store)
        signal = adapter.detect(RuntimeError("known tool failure"), task_class="agent-tool")
        now = datetime.now(timezone.utc)
        store.save(
            Gene(
                gene_id="gene-tool-failure",
                name="Recover known tool failure",
                task_class="agent-tool",
                confidence=0.9,
                strategy={"steps": ["inspect tool args", "retry with fixed input"]},
                signals={"fingerprint": signal.fingerprint, "error_type": signal.error_type},
                source="local",
                created_at=now,
                updated_at=now,
            )
        )

        middleware = create_oris_middleware(adapter, task_class="agent-tool")
        request = SimpleNamespace(
            tool_call={"id": "call-1", "name": "demo_tool", "args": {"input": "bad"}}
        )

        def failing_handler(_request):
            raise RuntimeError("known tool failure")

        result = middleware.wrap_tool_call(request, failing_handler)
        assert isinstance(result, tool_message_cls)
        assert "inspect tool args" in result.content
        assert result.tool_call_id == "call-1"
    finally:
        store.close()
