from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any


@dataclass
class OenEnvelope:
    sender_id: str
    message_type: str
    payload: Any
    signature: str
    timestamp: str


@dataclass
class RegisterRequest:
    node_id: str
    endpoint: str
    public_key: str
    capabilities: list[str]
    version: str
    region: str = ""


@dataclass
class RegisterResponse:
    node_id: str
    heartbeat_interval_seconds: int
    ttl_seconds: int


@dataclass
class HeartbeatResponse:
    acknowledged: bool
    next_heartbeat_seconds: int


@dataclass
class NodeInfo:
    node_id: str
    endpoint: str
    public_key: str
    capabilities: list[str]
    version: str
    status: str
    registered_at: str
    last_heartbeat: str
    ttl_seconds: int
    region: str = ""


@dataclass
class DiscoveryResult:
    nodes: list[NodeInfo]
    total: int


@dataclass
class FederatedQuery:
    query: str
    task_class: str = ""
    min_confidence: float = 0.0
    timeout_ms: int = 0
    target_nodes: list[str] = field(default_factory=list)
    limit: int = 0


@dataclass
class GeneResult:
    gene_id: str
    name: str
    task_class: str
    confidence: float
    source_node: str
    created_at: str


@dataclass
class FederationMeta:
    nodes_queried: int
    nodes_responded: int
    coverage: float
    freshness: str
    timeout_nodes: list[str] = field(default_factory=list)


@dataclass
class FederatedResult:
    results: list[GeneResult]
    meta: FederationMeta


@dataclass
class RunJobRequest:
    thread_id: str
    input: dict[str, Any] | None = None
    idempotency_key: str = ""
    priority: int | None = None
    tenant_id: str = ""


@dataclass
class RunJobResponse:
    thread_id: str
    status: str
    interrupts: list[Any] = field(default_factory=list)
    idempotency_key: str = ""
    idempotent_replay: bool = False


@dataclass
class JobStateResponse:
    thread_id: str
    values: dict[str, Any] = field(default_factory=dict)
    checkpoint_id: str = ""
    created_at: str | None = None


@dataclass
class CancelJobResponse:
    thread_id: str
    status: str
    reason: str = ""


@dataclass
class JobListItem:
    thread_id: str
    status: str
    updated_at: str


@dataclass
class ListJobsResponse:
    jobs: list[JobListItem] = field(default_factory=list)


@dataclass
class ShareResponse:
    gene_id: str
    status: str
    published_at: str = ""


@dataclass
class NetworkAsset:
    type: str
    id: str
    confidence: float = 0.0
    quality_score: float = 0.0
    use_count: int = 0
    success_count: int = 0
    contributor_id: str = ""
    created_at: str = ""
    signals: Any = None
    strategy: Any = None
    validation: Any = None


@dataclass
class SyncAudit:
    total_available: int = 0
    returned: int = 0


@dataclass
class FetchResponse:
    assets: list[NetworkAsset] = field(default_factory=list)
    next_cursor: str | None = None
    sync_audit: SyncAudit = field(default_factory=SyncAudit)


@dataclass
class ApiEnvelope:
    meta: dict[str, str]
    request_id: str
    data: Any
