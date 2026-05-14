import json

import httpx

from oris_sdk.experience import ExperienceClient, ExperienceConfig


SEED = b"test-seed-32-bytes-for-testing!!"


def test_share_sends_api_key_and_envelope():
    captured = {}

    def handler(request: httpx.Request):
        captured["api_key"] = request.headers.get("x-api-key")
        captured["body"] = json.loads(request.content)
        return httpx.Response(200, json={"gene_id": "g1", "status": "published"})

    client = httpx.Client(transport=httpx.MockTransport(handler))
    ec = ExperienceClient(
        ExperienceConfig(base_url="http://test", api_key="ak-123", seed=SEED, sender_id="agent-1"),
        client=client,
    )

    resp = ec.share({"gene_id": "g1", "confidence": 0.9})
    assert captured["api_key"] == "ak-123"
    env = captured["body"]["envelope"]
    assert env["sender_id"] == "agent-1"
    assert env["message_type"] == "publish"
    assert env["signature"] != ""
    assert resp["gene_id"] == "g1"


def test_fetch_requires_no_auth():
    captured = {}

    def handler(request: httpx.Request):
        captured["auth"] = request.headers.get("authorization")
        captured["api_key"] = request.headers.get("x-api-key")
        return httpx.Response(200, json={"assets": [], "next_cursor": None, "sync_audit": {"total_available": 0, "returned": 0}})

    client = httpx.Client(transport=httpx.MockTransport(handler))
    ec = ExperienceClient(
        ExperienceConfig(base_url="http://test", api_key="ak-123", seed=SEED, sender_id="a1"),
        client=client,
    )

    resp = ec.fetch(q="bugfix", limit=5)
    assert captured["auth"] is None
    assert captured["api_key"] is None
    assert resp["assets"] == []


def test_register_public_key_sends_hex():
    captured = {}

    def handler(request: httpx.Request):
        captured["body"] = json.loads(request.content)
        return httpx.Response(200)

    client = httpx.Client(transport=httpx.MockTransport(handler))
    ec = ExperienceClient(
        ExperienceConfig(base_url="http://test", api_key="ak-123", seed=SEED, sender_id="agent-1"),
        client=client,
    )

    ec.register_public_key()
    assert captured["body"]["sender_id"] == "agent-1"
    assert len(captured["body"]["public_key_hex"]) == 64
