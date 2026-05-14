from __future__ import annotations

import json
from dataclasses import dataclass
from datetime import datetime, timezone
from typing import Any

import httpx

from oris_sdk.signing import public_key_hex, sign_payload


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
