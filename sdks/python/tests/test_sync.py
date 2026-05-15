from datetime import datetime, timezone
from unittest.mock import MagicMock

from oris_sdk.experience import ExperienceClient
from oris_sdk.gene import Gene, PullOpts, PushOpts
from oris_sdk.store import LocalStore
from oris_sdk.sync_manager import SyncManager


def _store() -> LocalStore:
    return LocalStore(":memory:")


def _sample_gene(gene_id: str = "gene-1") -> Gene:
    now = datetime.now(timezone.utc)
    return Gene(
        gene_id=gene_id, name=f"test-{gene_id}", task_class="bugfix",
        confidence=0.85, source="local", created_at=now, updated_at=now,
    )


def test_push_to_hub():
    s = _store()
    s.save(_sample_gene())

    exp = MagicMock(spec=ExperienceClient)
    exp.share.return_value = {"gene_id": "gene-1", "status": "published"}

    mgr = SyncManager(store=s, experience=exp)
    result = mgr.push_to_hub()
    assert result.pushed == 1
    assert result.failed == 0

    exp.share.assert_called_once()
    payload = exp.share.call_args[0][0]
    assert payload["id"] == "gene-1"
    assert payload["type"] == "gene"

    got = s.get("gene-1")
    assert got.synced_at is not None

    logs = s.get_sync_log(10)
    assert len(logs) == 1
    assert logs[0].status == "success"
    s.close()


def test_push_specific_genes():
    s = _store()
    s.save(_sample_gene("gene-1"))
    s.save(_sample_gene("gene-2"))

    exp = MagicMock(spec=ExperienceClient)
    exp.share.return_value = {"status": "published"}

    mgr = SyncManager(store=s, experience=exp)
    result = mgr.push_to_hub(PushOpts(gene_ids=["gene-1"]))
    assert result.pushed == 1
    assert exp.share.call_count == 1
    s.close()


def test_push_failure():
    s = _store()
    s.save(_sample_gene())

    exp = MagicMock(spec=ExperienceClient)
    exp.share.side_effect = Exception("connection refused")

    mgr = SyncManager(store=s, experience=exp)
    result = mgr.push_to_hub()
    assert result.failed == 1
    assert result.pushed == 0
    assert len(result.errors) == 1
    assert "connection refused" in result.errors[0].message

    logs = s.get_sync_log(10)
    assert logs[0].status == "failed"
    s.close()


def test_pull_from_hub():
    s = _store()

    exp = MagicMock(spec=ExperienceClient)
    exp.fetch.return_value = {
        "assets": [
            {"type": "gene", "id": "remote-1", "confidence": 0.9,
             "quality_score": 0.85, "use_count": 10, "success_count": 8}
        ],
        "sync_audit": {"total_available": 1, "returned": 1},
    }

    mgr = SyncManager(store=s, experience=exp)
    imported = mgr.pull_from_hub(PullOpts(limit=10))
    assert imported == 1

    exp.fetch.assert_called_once_with(q="", min_confidence=0.0, limit=10)

    got = s.get("remote-1")
    assert got is not None
    assert got.source == "hub"
    assert got.confidence == 0.9
    s.close()


def test_pull_conflict_skips():
    s = _store()
    now = datetime.now(timezone.utc)
    s.save(Gene(
        gene_id="remote-1", name="existing", task_class="feature",
        confidence=0.5, source="local", created_at=now, updated_at=now,
    ))

    exp = MagicMock(spec=ExperienceClient)
    exp.fetch.return_value = {
        "assets": [{"type": "gene", "id": "remote-1", "confidence": 0.9}],
    }

    mgr = SyncManager(store=s, experience=exp)
    imported = mgr.pull_from_hub()
    assert imported == 0

    logs = s.get_sync_log(10)
    assert any(l.status == "conflict" for l in logs)
    s.close()


def test_pull_merge_stats_when_same_task_class():
    s = _store()
    now = datetime.now(timezone.utc)
    s.save(Gene(
        gene_id="gene-1", name="existing", task_class="unknown",
        confidence=0.5, use_count=3, success_count=2,
        source="local", created_at=now, updated_at=now,
    ))

    exp = MagicMock(spec=ExperienceClient)
    exp.fetch.return_value = {
        "assets": [{"type": "gene", "id": "gene-1", "confidence": 0.9,
                    "use_count": 10, "success_count": 8}],
    }

    mgr = SyncManager(store=s, experience=exp)
    imported = mgr.pull_from_hub()
    assert imported == 1

    got = s.get("gene-1")
    assert got.confidence == 0.9
    assert got.use_count == 10
    assert got.success_count == 8
    s.close()


def test_no_experience_client():
    s = _store()
    mgr = SyncManager(store=s)

    try:
        mgr.push_to_hub()
        assert False, "should raise"
    except RuntimeError:
        pass

    try:
        mgr.pull_from_hub()
        assert False, "should raise"
    except RuntimeError:
        pass
    s.close()
