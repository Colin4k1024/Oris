"""Generated-shape models for ``oris.experience.bundle/v1``.

The JSON Schema in ``spec/experience`` is authoritative. These dataclasses
preserve every contract field without adding an SDK dependency.
"""
from __future__ import annotations

from dataclasses import asdict, dataclass, field
from typing import Any


@dataclass
class Applicability:
    required_signals: list[str] = field(default_factory=list)
    excluded_signals: list[str] = field(default_factory=list)
    environments: list[dict[str, Any]] = field(default_factory=list)
    project_ids: list[str] = field(default_factory=list)
    tenant_ids: list[str] = field(default_factory=list)
    do_not_use_when: list[str] = field(default_factory=list)


@dataclass
class GeneV1:
    id: str
    version: int
    name: str
    description: str
    scope: str
    task_category: str
    applicability: Applicability
    steps: list[dict[str, Any]]
    tool_requirements: list[dict[str, Any]]
    safety: dict[str, Any]
    validation: dict[str, Any]
    provenance: dict[str, Any]
    lifecycle: str
    created_at: str
    updated_at: str
    metadata: dict[str, Any] = field(default_factory=dict)


@dataclass
class CapsuleV1:
    id: str
    gene_id: str
    gene_version: int
    environment_fingerprint: str
    task_context_hash: str
    execution_evidence_hash: str
    validation: dict[str, Any]
    artifact_refs: list[str]
    redaction: str
    created_at: str


@dataclass
class UsageReceiptV1:
    id: str
    gene_id: str
    gene_version: int
    agent_id: str
    run_id: str
    task_context_hash: str
    adoption: str
    applied_step_ids: list[str]
    outcome: str
    test_evidence_refs: list[str]
    created_at: str
    failure_reason: str | None = None
    cost: dict[str, Any] | None = None


@dataclass
class ExperienceBundleV1:
    schema_version: str
    gene: GeneV1
    capsules: list[CapsuleV1] = field(default_factory=list)
    usage_receipts: list[UsageReceiptV1] = field(default_factory=list)

    @classmethod
    def from_dict(cls, value: dict[str, Any]) -> "ExperienceBundleV1":
        gene = value["gene"]
        return cls(
            schema_version=value["schema_version"],
            gene=GeneV1(**{**gene, "applicability": Applicability(**gene["applicability"])}),
            capsules=[CapsuleV1(**item) for item in value.get("capsules", [])],
            usage_receipts=[UsageReceiptV1(**item) for item in value.get("usage_receipts", [])],
        )

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)

    def validate(self) -> None:
        if self.schema_version != "oris.experience.bundle/v1":
            raise ValueError(f"unsupported schema_version: {self.schema_version}")
        if not self.gene.steps or not self.gene.validation.get("checks"):
            raise ValueError("a Gene requires steps and validation checks")
        if self.gene.lifecycle == "stable":
            p = self.gene.provenance
            if p.get("verified_successes", 0) < 3 or p.get("distinct_task_contexts", 0) < 2:
                raise ValueError("stable Gene lacks promotion evidence")
        for receipt in self.usage_receipts:
            if receipt.outcome == "succeeded" and receipt.adoption == "adopted" and not receipt.test_evidence_refs:
                raise ValueError("successful adoption requires test evidence")
