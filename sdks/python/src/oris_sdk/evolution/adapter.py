from __future__ import annotations

from datetime import datetime, timezone
from typing import Any

from oris_sdk.evolution.types import (
    REPLAY_SKIP,
    REPLAY_SUGGEST,
    EvolutionCandidate,
    EvolutionSignal,
    ReplayDecision,
    ReplayPolicy,
    SolidifyInput,
    Status,
    detect_signal,
    fingerprint,
)
from oris_sdk.gene import Gene, StoreQuery
from oris_sdk.store_protocol import StoreProtocol


class OrisEvolutionAdapter:
    def __init__(self, store: StoreProtocol, policy: ReplayPolicy | None = None):
        self.store = store
        self.policy = policy or ReplayPolicy()
        if not self.policy.mode:
            self.policy.mode = REPLAY_SUGGEST
        if self.policy.limit <= 0:
            self.policy.limit = 5

    def detect(
        self,
        error: Exception | str,
        task_class: str = "general",
        context: dict[str, Any] | None = None,
    ) -> EvolutionSignal:
        return detect_signal(error, task_class, context)

    def select(self, signal: EvolutionSignal) -> list[EvolutionCandidate]:
        genes = self.store.query(
            StoreQuery(
                task_class=signal.task_class,
                min_confidence=self.policy.min_confidence,
                order_by="confidence DESC",
                limit=self.policy.limit * 3,
            )
        )
        candidates = [
            self._candidate_from_gene(gene, signal)
            for gene in genes
            if self._matches_signal(gene, signal)
        ]
        candidates.sort(key=lambda candidate: candidate.confidence, reverse=True)
        return candidates[: self.policy.limit]

    def replay(self, candidate: EvolutionCandidate) -> ReplayDecision:
        if not candidate.gene_id or candidate.confidence < self.policy.min_confidence:
            return ReplayDecision(mode=REPLAY_SKIP)
        return ReplayDecision(
            mode=self.policy.mode or REPLAY_SUGGEST,
            candidate=candidate,
            instructions=candidate.steps,
            metadata={"rationale": candidate.rationale},
        )

    def solidify(self, input: SolidifyInput) -> Gene:
        if not input.validation.passed:
            raise ValueError("validation did not pass")
        now = datetime.now(timezone.utc)
        gene_id = input.gene_id or f"evo-{fingerprint(input.signal.fingerprint, input.solution, now.isoformat())}"
        gene = Gene(
            gene_id=gene_id,
            name=input.name or f"Evolution pattern for {input.signal.task_class}",
            task_class=input.signal.task_class,
            confidence=float(input.validation.metrics.get("confidence", 0.8)),
            strategy={
                "solution": input.solution,
                "steps": input.steps,
                "tags": input.tags,
            },
            signals={
                "fingerprint": input.signal.fingerprint,
                "error_type": input.signal.error_type,
                "message": input.signal.message,
            },
            validation={
                "passed": input.validation.passed,
                "evidence": input.validation.evidence,
                "metrics": input.validation.metrics,
            },
            quality_score=0.8,
            success_count=1,
            contributor_id="oris-adapter",
            source="local",
            created_at=now,
            updated_at=now,
        )
        self.store.save(gene)
        return gene

    def status(self) -> Status:
        genes = self.store.list_genes(limit=1000)
        status = Status(genes_count=len(genes))
        for gene in genes:
            status.avg_confidence += gene.confidence
            status.total_uses += gene.use_count
            status.successful_uses += gene.success_count
        if status.genes_count:
            status.avg_confidence /= status.genes_count
        if status.total_uses:
            status.reuse_rate = status.successful_uses / status.total_uses
        return status

    @staticmethod
    def _matches_signal(gene: Gene, signal: EvolutionSignal) -> bool:
        if not gene.signals:
            return True
        gene_fingerprint = gene.signals.get("fingerprint")
        if gene_fingerprint:
            return gene_fingerprint == signal.fingerprint
        error_type = gene.signals.get("error_type")
        if error_type:
            return error_type == signal.error_type
        return True

    @staticmethod
    def _candidate_from_gene(gene: Gene, signal: EvolutionSignal) -> EvolutionCandidate:
        raw_steps = gene.strategy.get("steps", [])
        steps = [step for step in raw_steps if isinstance(step, str)]
        if not steps and isinstance(gene.strategy.get("solution"), str):
            steps = [gene.strategy["solution"]]
        return EvolutionCandidate(
            gene_id=gene.gene_id,
            capsule_id=str(gene.strategy.get("capsule_id", "")),
            confidence=gene.confidence,
            rationale=str(gene.strategy.get("rationale", "")),
            steps=steps,
            metadata={
                "task_class": gene.task_class,
                "fingerprint": signal.fingerprint,
                "source": gene.source,
            },
        )

