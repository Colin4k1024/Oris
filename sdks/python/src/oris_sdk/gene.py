from __future__ import annotations

from dataclasses import dataclass, field
from datetime import datetime
from typing import Any


@dataclass
class Gene:
    gene_id: str
    name: str
    task_class: str
    confidence: float = 0.0
    strategy: dict[str, Any] = field(default_factory=dict)
    signals: dict[str, Any] = field(default_factory=dict)
    validation: dict[str, Any] = field(default_factory=dict)
    quality_score: float = 0.0
    use_count: int = 0
    success_count: int = 0
    contributor_id: str = ""
    source: str = "local"
    synced_at: datetime | None = None
    created_at: datetime = field(default_factory=datetime.utcnow)
    updated_at: datetime = field(default_factory=datetime.utcnow)


@dataclass
class StoreQuery:
    q: str = ""
    task_class: str = ""
    min_confidence: float = 0.0
    source: str = ""
    order_by: str = ""
    limit: int = 50
    offset: int = 0


@dataclass
class PushOpts:
    gene_ids: list[str] = field(default_factory=list)


@dataclass
class PullOpts:
    q: str = ""
    min_confidence: float = 0.0
    limit: int = 0
    task_class: str = ""


@dataclass
class PushResult:
    pushed: int = 0
    failed: int = 0
    errors: list[PushError] = field(default_factory=list)


@dataclass
class PushError:
    gene_id: str = ""
    message: str = ""


@dataclass
class SyncLogEntry:
    id: int = 0
    direction: str = ""
    gene_id: str = ""
    status: str = ""
    remote_url: str = ""
    error_message: str = ""
    timestamp: datetime = field(default_factory=datetime.utcnow)
