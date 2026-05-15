from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any

from oris_sdk.experience import ExperienceClient, ExperienceConfig
from oris_sdk.hub import HubClient, HubConfig
from oris_sdk.store import LocalStore
from oris_sdk.sync_manager import SyncManager


@dataclass
class OrisConfig:
    store_path: str = "oris_genes.db"
    hub: HubConfig | None = None
    experience: ExperienceConfig | None = None


class OrisClient:
    def __init__(self, config: OrisConfig | None = None):
        config = config or OrisConfig()
        self.store = LocalStore(config.store_path)

        exp_client = None
        hub_client = None

        if config.experience:
            exp_client = ExperienceClient(config.experience)
        if config.hub:
            hub_client = HubClient(config.hub)

        self.sync = SyncManager(
            store=self.store,
            experience=exp_client,
            hub=hub_client,
        )

    def close(self) -> None:
        self.store.close()

    def __enter__(self) -> "OrisClient":
        return self

    def __exit__(self, *_: Any) -> None:
        self.close()
