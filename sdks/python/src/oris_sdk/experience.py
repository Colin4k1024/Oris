from __future__ import annotations

import json
from dataclasses import dataclass
from datetime import datetime, timezone
from typing import Any

import httpx

from oris_sdk.signing import public_key_hex, sign_payload
from oris_sdk.experience_v1 import ExperienceBundleV1, UsageReceiptV1, CapsuleV1


@dataclass
class ExperienceConfig:
    base_url: str
    api_key: str
    seed: bytes
    sender_id: str


class ExperienceClient:
    def __init__(self, config: ExperienceConfig, *, client: httpx.Client | None = None):
        self._cfg = config
        self._http = client or httpx.Client()

    @property
    def agent_id(self) -> str:
        return self._cfg.sender_id

    def share(self, payload: Any) -> dict[str, Any]:
        sig = sign_payload(self._cfg.seed, payload)
        envelope = {
            "sender_id": self._cfg.sender_id,
            "message_type": "publish",
            "payload": payload,
            "signature": sig,
            "timestamp": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        }
        body = json.dumps({"envelope": envelope}, separators=(",", ":")).encode()

        resp = self._http.post(
            f"{self._cfg.base_url}/experience",
            content=body,
            headers={
                "Content-Type": "application/json",
                "X-Api-Key": self._cfg.api_key,
            },
        )
        resp.raise_for_status()
        return resp.json()

    def fetch(
        self,
        q: str = "",
        min_confidence: float = 0.0,
        limit: int = 0,
        cursor: str = "",
    ) -> dict[str, Any]:
        params: dict[str, str] = {}
        if q:
            params["q"] = q
        if min_confidence > 0:
            params["min_confidence"] = str(min_confidence)
        if limit > 0:
            params["limit"] = str(limit)
        if cursor:
            params["cursor"] = cursor

        resp = self._http.get(
            f"{self._cfg.base_url}/experience",
            params=params or None,
            headers={"X-Api-Key": self._cfg.api_key},
        )
        resp.raise_for_status()
        return resp.json()

    def search_v1(self, **query: Any) -> dict[str, Any]:
        params = {key: value for key, value in query.items() if value is not None}
        if isinstance(params.get("available_tools"), (list, tuple)):
            params["available_tools"] = ",".join(params["available_tools"])
        if isinstance(params.get("environment"), dict):
            params["environment"] = json.dumps(params["environment"], separators=(",", ":"))
        resp = self._http.get(
            f"{self._cfg.base_url}/v1/experience-assets",
            params=params,
            headers={"X-Api-Key": self._cfg.api_key},
        )
        resp.raise_for_status()
        return resp.json()

    def propose_v1(self, bundle: ExperienceBundleV1) -> dict[str, Any]:
        bundle.validate()
        resp = self._http.post(
            f"{self._cfg.base_url}/v1/experience-assets",
            json=bundle.to_dict(),
            headers={"X-Api-Key": self._cfg.api_key},
        )
        resp.raise_for_status()
        return resp.json()

    def begin_use(self, gene_id: str, gene_version: int, run_id: str, task_context_hash: str) -> dict[str, Any]:
        resp = self._http.post(
            f"{self._cfg.base_url}/v1/experience-assets/{gene_id}/use",
            json={"gene_version": gene_version, "run_id": run_id, "task_context_hash": task_context_hash},
            headers={"X-Api-Key": self._cfg.api_key},
        )
        resp.raise_for_status()
        return resp.json()

    def record_outcome(self, receipt: UsageReceiptV1, capsule: CapsuleV1 | None = None) -> dict[str, Any]:
        body: dict[str, Any] = {"receipt": vars(receipt)}
        if capsule is not None:
            body["capsule"] = vars(capsule)
        resp = self._http.post(
            f"{self._cfg.base_url}/v1/experience-assets/{receipt.gene_id}/outcomes",
            json=body,
            headers={"X-Api-Key": self._cfg.api_key},
        )
        resp.raise_for_status()
        return resp.json()

    def register_public_key(self) -> None:
        body = json.dumps(
            {
                "sender_id": self._cfg.sender_id,
                "public_key_hex": public_key_hex(self._cfg.seed),
            },
            separators=(",", ":"),
        ).encode()

        resp = self._http.post(
            f"{self._cfg.base_url}/public-keys",
            content=body,
            headers={
                "Content-Type": "application/json",
                "X-Api-Key": self._cfg.api_key,
            },
        )
        resp.raise_for_status()
