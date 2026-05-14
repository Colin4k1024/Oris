from __future__ import annotations

import json
from dataclasses import dataclass
from typing import Any

import httpx

from oris_sdk.signing import public_key_base64, sign_body


@dataclass
class HubConfig:
    base_url: str
    api_key: str
    seed: bytes
    node_id: str


class HubClient:
    def __init__(self, config: HubConfig, *, client: httpx.Client | None = None):
        self._cfg = config
        self._http = client or httpx.Client()

    def register(
        self,
        endpoint: str,
        capabilities: list[str],
        version: str,
        region: str = "",
    ) -> dict[str, Any]:
        body_dict: dict[str, Any] = {
            "node_id": self._cfg.node_id,
            "endpoint": endpoint,
            "public_key": public_key_base64(self._cfg.seed),
            "capabilities": capabilities,
            "version": version,
        }
        if region:
            body_dict["region"] = region

        body = json.dumps(body_dict, separators=(",", ":")).encode()
        sig = sign_body(self._cfg.seed, body)

        resp = self._http.post(
            f"{self._cfg.base_url}/hub/nodes",
            content=body,
            headers={
                "Content-Type": "application/json",
                "X-OEN-Signature": sig,
            },
        )
        resp.raise_for_status()
        return resp.json()

    def heartbeat(self, status: str = "active") -> dict[str, Any]:
        body_dict = {"node_id": self._cfg.node_id, "status": status}
        body = json.dumps(body_dict, separators=(",", ":")).encode()
        sig = sign_body(self._cfg.seed, body)

        resp = self._http.put(
            f"{self._cfg.base_url}/hub/nodes/{self._cfg.node_id}/heartbeat",
            content=body,
            headers={
                "Content-Type": "application/json",
                "X-OEN-Signature": sig,
            },
        )
        resp.raise_for_status()
        return resp.json()

    def discover(
        self,
        capabilities: list[str] | None = None,
        region: str = "",
        limit: int = 0,
    ) -> dict[str, Any]:
        params: dict[str, str] = {}
        if capabilities:
            params["capabilities"] = ",".join(capabilities)
        if region:
            params["region"] = region
        if limit > 0:
            params["limit"] = str(limit)

        resp = self._http.get(
            f"{self._cfg.base_url}/hub/nodes",
            params=params or None,
            headers={"Authorization": f"Bearer {self._cfg.api_key}"},
        )
        resp.raise_for_status()
        return resp.json()

    def search(
        self,
        query: str,
        task_class: str = "",
        min_confidence: float = 0.0,
        limit: int = 0,
    ) -> dict[str, Any]:
        body_dict: dict[str, Any] = {"query": query}
        if task_class:
            body_dict["task_class"] = task_class
        if min_confidence > 0:
            body_dict["min_confidence"] = min_confidence
        if limit > 0:
            body_dict["limit"] = limit

        resp = self._http.post(
            f"{self._cfg.base_url}/hub/search",
            content=json.dumps(body_dict, separators=(",", ":")).encode(),
            headers={
                "Content-Type": "application/json",
                "Authorization": f"Bearer {self._cfg.api_key}",
            },
        )
        resp.raise_for_status()
        return resp.json()

    def subscribe(
        self,
        callback_url: str,
        filter_task_class: str = "",
        filter_min_confidence: float = 0.0,
    ) -> dict[str, Any]:
        body_dict: dict[str, Any] = {
            "subscriber_node_id": self._cfg.node_id,
            "callback_url": callback_url,
            "filter": {},
        }
        if filter_task_class:
            body_dict["filter"]["task_class"] = filter_task_class
        if filter_min_confidence > 0:
            body_dict["filter"]["min_confidence"] = filter_min_confidence

        resp = self._http.post(
            f"{self._cfg.base_url}/hub/subscriptions",
            content=json.dumps(body_dict, separators=(",", ":")).encode(),
            headers={
                "Content-Type": "application/json",
                "Authorization": f"Bearer {self._cfg.api_key}",
            },
        )
        resp.raise_for_status()
        return resp.json()

    def unsubscribe(self, subscription_id: str) -> None:
        resp = self._http.delete(
            f"{self._cfg.base_url}/hub/subscriptions/{subscription_id}",
            headers={"Authorization": f"Bearer {self._cfg.api_key}"},
        )
        resp.raise_for_status()

    def list_subscriptions(self) -> list[dict[str, Any]]:
        resp = self._http.get(
            f"{self._cfg.base_url}/hub/subscriptions",
            headers={"Authorization": f"Bearer {self._cfg.api_key}"},
        )
        resp.raise_for_status()
        return resp.json()
