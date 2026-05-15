from __future__ import annotations

from datetime import datetime, timezone
from typing import Any

from oris_sdk.experience import ExperienceClient
from oris_sdk.gene import Gene, PullOpts, PushError, PushOpts, PushResult, SyncLogEntry
from oris_sdk.hub import HubClient
from oris_sdk.store import LocalStore


class SyncManager:
    def __init__(
        self,
        store: LocalStore,
        experience: ExperienceClient | None = None,
        hub: HubClient | None = None,
    ):
        self._store = store
        self._experience = experience
        self._hub = hub

    def push_to_hub(self, opts: PushOpts | None = None) -> PushResult:
        if self._experience is None:
            raise RuntimeError("experience client not configured")

        opts = opts or PushOpts()
        if opts.gene_ids:
            genes = [g for gid in opts.gene_ids if (g := self._store.get(gid)) is not None]
        else:
            genes = self._store.get_unsynced()

        result = PushResult()
        for gene in genes:
            payload = _gene_to_payload(gene)
            entry = SyncLogEntry(direction="push", gene_id=gene.gene_id, timestamp=_now())

            try:
                self._experience.share(payload)
                entry.status = "success"
                result.pushed += 1
                self._store.mark_synced(gene.gene_id, _now())
            except Exception as e:
                entry.status = "failed"
                entry.error_message = str(e)
                result.failed += 1
                result.errors.append(PushError(gene_id=gene.gene_id, message=str(e)))

            self._store.log_sync(entry)
        return result

    def pull_from_hub(self, opts: PullOpts | None = None) -> int:
        if self._experience is None:
            raise RuntimeError("experience client not configured")

        opts = opts or PullOpts()
        resp = self._experience.fetch(
            q=opts.q,
            min_confidence=opts.min_confidence,
            limit=opts.limit,
        )

        imported = 0
        for asset in resp.get("assets", []):
            gene = _asset_to_gene(asset)
            existing = self._store.get(gene.gene_id)

            if existing is not None:
                if _core_fields_match(existing, gene):
                    gene = _merge_stats(existing, gene)
                else:
                    self._store.log_sync(SyncLogEntry(
                        direction="pull", gene_id=gene.gene_id,
                        status="conflict", error_message="core fields differ, skipped",
                        timestamp=_now(),
                    ))
                    continue

            self._store.save(gene)
            self._store.log_sync(SyncLogEntry(
                direction="pull", gene_id=gene.gene_id,
                status="success", timestamp=_now(),
            ))
            imported += 1
        return imported

    def register_node(self, **kwargs: Any) -> dict[str, Any]:
        if self._hub is None:
            raise RuntimeError("hub client not configured")
        return self._hub.register(**kwargs)

    def get_sync_log(self, limit: int = 50) -> list[SyncLogEntry]:
        return self._store.get_sync_log(limit)


def _now() -> datetime:
    return datetime.now(timezone.utc)


def _gene_to_payload(g: Gene) -> dict[str, Any]:
    p: dict[str, Any] = {
        "type": "gene",
        "id": g.gene_id,
        "confidence": g.confidence,
        "quality_score": g.quality_score,
        "use_count": g.use_count,
        "success_count": g.success_count,
        "created_at": g.created_at.isoformat(),
    }
    if g.strategy:
        p["strategy"] = g.strategy
    if g.signals:
        p["signals"] = g.signals
    if g.validation:
        p["validation"] = g.validation
    if g.contributor_id:
        p["contributor_id"] = g.contributor_id
    return p


def _asset_to_gene(asset: dict[str, Any]) -> Gene:
    now = _now()
    return Gene(
        gene_id=asset.get("id", ""),
        name=asset.get("id", ""),
        task_class="unknown",
        confidence=asset.get("confidence", 0.0),
        quality_score=asset.get("quality_score", 0.0),
        use_count=asset.get("use_count", 0),
        success_count=asset.get("success_count", 0),
        contributor_id=asset.get("contributor_id", ""),
        strategy=asset.get("strategy") if isinstance(asset.get("strategy"), dict) else {},
        signals=asset.get("signals") if isinstance(asset.get("signals"), dict) else {},
        validation=asset.get("validation") if isinstance(asset.get("validation"), dict) else {},
        source="hub",
        synced_at=now,
        created_at=now,
        updated_at=now,
    )


def _core_fields_match(existing: Gene, incoming: Gene) -> bool:
    return existing.task_class == incoming.task_class


def _merge_stats(existing: Gene, incoming: Gene) -> Gene:
    existing.use_count = max(existing.use_count, incoming.use_count)
    existing.success_count = max(existing.success_count, incoming.success_count)
    existing.confidence = max(existing.confidence, incoming.confidence)
    existing.quality_score = max(existing.quality_score, incoming.quality_score)
    existing.updated_at = _now()
    existing.synced_at = _now()
    return existing
