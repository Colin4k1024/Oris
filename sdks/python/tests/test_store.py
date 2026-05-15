from datetime import datetime, timezone

from oris_sdk.gene import Gene, StoreQuery
from oris_sdk.store import LocalStore


def _store() -> LocalStore:
    return LocalStore(":memory:")


def _sample_gene(gene_id: str = "gene-1") -> Gene:
    now = datetime.now(timezone.utc)
    return Gene(
        gene_id=gene_id,
        name=f"test-{gene_id}",
        task_class="bugfix",
        confidence=0.85,
        strategy={"approach": "retry"},
        signals={"error_rate": 0.1},
        validation={"tests_passed": True},
        quality_score=0.9,
        use_count=5,
        success_count=4,
        contributor_id="node-1",
        source="local",
        created_at=now,
        updated_at=now,
    )


def test_save_and_get():
    s = _store()
    g = _sample_gene()
    s.save(g)
    got = s.get("gene-1")
    assert got is not None
    assert got.gene_id == "gene-1"
    assert got.confidence == 0.85
    assert got.strategy == {"approach": "retry"}
    s.close()


def test_get_not_found():
    s = _store()
    assert s.get("nonexistent") is None
    s.close()


def test_save_upsert():
    s = _store()
    g = _sample_gene()
    s.save(g)
    g.confidence = 0.95
    g.name = "updated"
    s.save(g)
    got = s.get("gene-1")
    assert got.confidence == 0.95
    assert got.name == "updated"
    s.close()


def test_delete():
    s = _store()
    s.save(_sample_gene())
    s.delete("gene-1")
    assert s.get("gene-1") is None
    s.close()


def test_query_by_task_class():
    s = _store()
    g1 = _sample_gene("gene-1")
    g1.task_class = "bugfix"
    g2 = _sample_gene("gene-2")
    g2.task_class = "feature"
    s.save(g1)
    s.save(g2)
    results = s.query(StoreQuery(task_class="bugfix"))
    assert len(results) == 1
    assert results[0].gene_id == "gene-1"
    s.close()


def test_query_by_min_confidence():
    s = _store()
    g1 = _sample_gene("gene-low")
    g1.confidence = 0.3
    g2 = _sample_gene("gene-high")
    g2.confidence = 0.9
    s.save(g1)
    s.save(g2)
    results = s.query(StoreQuery(min_confidence=0.8))
    assert len(results) == 1
    assert results[0].gene_id == "gene-high"
    s.close()


def test_query_by_search_term():
    s = _store()
    g1 = _sample_gene("gene-1")
    g1.name = "retry-handler"
    g2 = _sample_gene("gene-2")
    g2.name = "cache-warmer"
    s.save(g1)
    s.save(g2)
    results = s.query(StoreQuery(q="retry"))
    assert len(results) == 1
    assert results[0].name == "retry-handler"
    s.close()


def test_update_stats():
    s = _store()
    g = _sample_gene()
    g.use_count = 0
    g.success_count = 0
    s.save(g)
    s.update_stats("gene-1", used=True, success=True)
    got = s.get("gene-1")
    assert got.use_count == 1
    assert got.success_count == 1
    s.update_stats("gene-1", used=True, success=False)
    got = s.get("gene-1")
    assert got.use_count == 2
    assert got.success_count == 1
    s.close()


def test_get_unsynced_and_mark_synced():
    s = _store()
    g = _sample_gene()
    g.source = "local"
    s.save(g)
    unsynced = s.get_unsynced()
    assert len(unsynced) == 1
    now = datetime.now(timezone.utc)
    s.mark_synced("gene-1", now)
    got = s.get("gene-1")
    assert got.synced_at is not None
    s.close()


def test_sync_log():
    from oris_sdk.gene import SyncLogEntry
    s = _store()
    entry = SyncLogEntry(
        direction="push", gene_id="gene-1", status="success",
        remote_url="http://localhost:3000",
        timestamp=datetime.now(timezone.utc),
    )
    s.log_sync(entry)
    logs = s.get_sync_log(10)
    assert len(logs) == 1
    assert logs[0].direction == "push"
    assert logs[0].gene_id == "gene-1"
    s.close()
