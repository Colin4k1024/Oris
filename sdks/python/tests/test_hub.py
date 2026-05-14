import json

import httpx

from oris_sdk.hub import HubClient, HubConfig


SEED = b"test-seed-32-bytes-for-testing!!"


def test_register_sends_signature_header():
    captured = {}

    def handler(request: httpx.Request):
        captured["signature"] = request.headers.get("x-oen-signature")
        captured["body"] = json.loads(request.content)
        return httpx.Response(
            200,
            json={"node_id": "n1", "heartbeat_interval_seconds": 30, "ttl_seconds": 90},
        )

    client = httpx.Client(transport=httpx.MockTransport(handler))
    hub = HubClient(
        HubConfig(base_url="http://test", api_key="ak", seed=SEED, node_id="n1"),
        client=client,
    )

    resp = hub.register(endpoint="http://localhost:8080", capabilities=["evolve"], version="0.1.0")

    assert captured["signature"] != ""
    assert captured["body"]["node_id"] == "n1"
    assert captured["body"]["public_key"] != ""
    assert resp["node_id"] == "n1"


def test_heartbeat_sends_signature():
    captured = {}

    def handler(request: httpx.Request):
        captured["signature"] = request.headers.get("x-oen-signature")
        captured["url"] = str(request.url)
        return httpx.Response(200, json={"acknowledged": True, "next_heartbeat_seconds": 30})

    client = httpx.Client(transport=httpx.MockTransport(handler))
    hub = HubClient(
        HubConfig(base_url="http://test", api_key="ak", seed=SEED, node_id="n1"),
        client=client,
    )

    resp = hub.heartbeat(status="active")
    assert captured["signature"] != ""
    assert "n1/heartbeat" in captured["url"]
    assert resp["acknowledged"] is True


def test_discover_sends_bearer():
    captured = {}

    def handler(request: httpx.Request):
        captured["auth"] = request.headers.get("authorization")
        return httpx.Response(200, json={"nodes": [], "total": 0})

    client = httpx.Client(transport=httpx.MockTransport(handler))
    hub = HubClient(
        HubConfig(base_url="http://test", api_key="my-key", seed=SEED, node_id="n1"),
        client=client,
    )

    resp = hub.discover(capabilities=["evolve"])
    assert captured["auth"] == "Bearer my-key"
    assert resp["total"] == 0


def test_search_sends_bearer():
    captured = {}

    def handler(request: httpx.Request):
        captured["auth"] = request.headers.get("authorization")
        captured["body"] = json.loads(request.content)
        return httpx.Response(
            200,
            json={"results": [], "meta": {"nodes_queried": 0, "nodes_responded": 0, "coverage": 0.0, "freshness": "", "timeout_nodes": []}},
        )

    client = httpx.Client(transport=httpx.MockTransport(handler))
    hub = HubClient(
        HubConfig(base_url="http://test", api_key="my-key", seed=SEED, node_id="n1"),
        client=client,
    )

    resp = hub.search(query="bugfix", task_class="fix")
    assert captured["auth"] == "Bearer my-key"
    assert captured["body"]["query"] == "bugfix"
