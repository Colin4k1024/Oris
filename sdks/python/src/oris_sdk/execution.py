from __future__ import annotations

import json
from dataclasses import dataclass
from typing import Any

import httpx


@dataclass
class ExecutionConfig:
    base_url: str
    token: str


class ExecutionClient:
    def __init__(self, config: ExecutionConfig, *, client: httpx.Client | None = None):
        self._cfg = config
        self._http = client or httpx.Client()

    def run_job(
        self,
        thread_id: str,
        input: dict[str, Any] | None = None,
        idempotency_key: str = "",
    ) -> dict[str, Any]:
        body: dict[str, Any] = {"thread_id": thread_id}
        if input:
            body["input"] = input
        if idempotency_key:
            body["idempotency_key"] = idempotency_key
        return self._post("/jobs", body)

    def get_state(self, thread_id: str) -> dict[str, Any]:
        return self._get(f"/jobs/{thread_id}/state")

    def get_detail(self, thread_id: str) -> dict[str, Any]:
        return self._get(f"/jobs/{thread_id}")

    def get_history(self, thread_id: str) -> dict[str, Any]:
        return self._get(f"/jobs/{thread_id}/history")

    def get_timeline(self, thread_id: str) -> dict[str, Any]:
        return self._get(f"/jobs/{thread_id}/timeline")

    def cancel(self, thread_id: str, reason: str = "") -> dict[str, Any]:
        body: dict[str, Any] = {}
        if reason:
            body["reason"] = reason
        return self._post(f"/jobs/{thread_id}/cancel", body)

    def resume(
        self, thread_id: str, value: dict[str, Any], checkpoint_id: str = ""
    ) -> dict[str, Any]:
        body: dict[str, Any] = {"value": value}
        if checkpoint_id:
            body["checkpoint_id"] = checkpoint_id
        return self._post(f"/jobs/{thread_id}/resume", body)

    def replay(self, thread_id: str, checkpoint_id: str = "") -> dict[str, Any]:
        body: dict[str, Any] = {}
        if checkpoint_id:
            body["checkpoint_id"] = checkpoint_id
        return self._post(f"/jobs/{thread_id}/replay", body)

    def list_jobs(
        self, status: str = "", limit: int = 0, offset: int = 0
    ) -> dict[str, Any]:
        params: dict[str, str] = {}
        if status:
            params["status"] = status
        if limit > 0:
            params["limit"] = str(limit)
        if offset > 0:
            params["offset"] = str(offset)
        return self._get("/jobs", params=params)

    def _post(self, path: str, body: dict[str, Any]) -> dict[str, Any]:
        resp = self._http.post(
            f"{self._cfg.base_url}{path}",
            content=json.dumps(body, separators=(",", ":")).encode(),
            headers={
                "Content-Type": "application/json",
                "Authorization": f"Bearer {self._cfg.token}",
            },
        )
        resp.raise_for_status()
        envelope = resp.json()
        return envelope["data"]

    def _get(
        self, path: str, params: dict[str, str] | None = None
    ) -> dict[str, Any]:
        resp = self._http.get(
            f"{self._cfg.base_url}{path}",
            params=params or None,
            headers={"Authorization": f"Bearer {self._cfg.token}"},
        )
        resp.raise_for_status()
        envelope = resp.json()
        return envelope["data"]
