import json

import httpx

from oris_sdk.execution import ExecutionClient, ExecutionConfig


def _envelope(data):
    return {"meta": {"status": "ok", "api_version": "v1"}, "request_id": "req-1", "data": data}


def test_run_job_sends_bearer():
    captured = {}

    def handler(request: httpx.Request):
        captured["auth"] = request.headers.get("authorization")
        captured["body"] = json.loads(request.content)
        return httpx.Response(
            200,
            json=_envelope({"thread_id": "t1", "status": "running", "interrupts": [], "idempotent_replay": False}),
        )

    client = httpx.Client(transport=httpx.MockTransport(handler))
    ec = ExecutionClient(ExecutionConfig(base_url="http://test", token="tok-123"), client=client)

    resp = ec.run_job(thread_id="t1", input={"task": "hello"})
    assert captured["auth"] == "Bearer tok-123"
    assert resp["thread_id"] == "t1"
    assert resp["status"] == "running"


def test_get_state_unwraps_envelope():
    def handler(request: httpx.Request):
        assert "/jobs/t1/state" in str(request.url)
        return httpx.Response(
            200,
            json=_envelope({"thread_id": "t1", "values": {"result": "done"}}),
        )

    client = httpx.Client(transport=httpx.MockTransport(handler))
    ec = ExecutionClient(ExecutionConfig(base_url="http://test", token="tok"), client=client)

    resp = ec.get_state("t1")
    assert resp["values"]["result"] == "done"


def test_cancel_sends_reason():
    captured = {}

    def handler(request: httpx.Request):
        captured["body"] = json.loads(request.content)
        return httpx.Response(
            200,
            json=_envelope({"thread_id": "t1", "status": "cancelled", "reason": "user abort"}),
        )

    client = httpx.Client(transport=httpx.MockTransport(handler))
    ec = ExecutionClient(ExecutionConfig(base_url="http://test", token="tok"), client=client)

    resp = ec.cancel("t1", reason="user abort")
    assert captured["body"]["reason"] == "user abort"
    assert resp["status"] == "cancelled"


def test_list_jobs_passes_params():
    captured = {}

    def handler(request: httpx.Request):
        captured["url"] = str(request.url)
        return httpx.Response(200, json=_envelope({"jobs": []}))

    client = httpx.Client(transport=httpx.MockTransport(handler))
    ec = ExecutionClient(ExecutionConfig(base_url="http://test", token="tok"), client=client)

    ec.list_jobs(status="running", limit=10)
    assert "status=running" in captured["url"]
    assert "limit=10" in captured["url"]
