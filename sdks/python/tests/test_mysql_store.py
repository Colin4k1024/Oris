import os
from datetime import datetime, timezone

import pytest

from oris_sdk.gene import Gene, StoreQuery, SyncLogEntry


def get_mysql_config():
    dsn = os.environ.get("MYSQL_DSN")
    if not dsn:
        pytest.skip("MYSQL_DSN not set, skipping MySQL tests")

    from oris_sdk.mysql_store import MySQLConfig, MySQLStore
    return MySQLStore.from_url(dsn)


@pytest.fixture
def store():
    s = get_mysql_config()
    yield s
    s.close()


def _make_gene(gene_id: str = "test-py-mysql-1") -> Gene:
    now = datetime.now(timezone.utc)
    return Gene(
        gene_id=gene_id,
        name="Test Gene",
        task_class="compile-fix",
        confidence=0.85,
        strategy={"approach": "retry"},
        signals={"error_type": "syntax"},
        validation={"tests_passed": True},
        quality_score=0.9,
        use_count=5,
        success_count=4,
        contributor_id="node-1",
        source="local",
        created_at=now,
        updated_at=now,
    )


class TestMySQLStore:
    def test_save_and_get(self, store):
        gene = _make_gene()
        store.save(gene)

        got = store.get("test-py-mysql-1")
        assert got is not None
        assert got.name == "Test Gene"
        assert got.confidence == 0.85
        assert got.strategy == {"approach": "retry"}

        store.delete("test-py-mysql-1")

    def test_get_not_found(self, store):
        got = store.get("nonexistent-gene-id")
        assert got is None

    def test_update_stats(self, store):
        gene = _make_gene("test-py-mysql-stats")
        store.save(gene)

        store.update_stats("test-py-mysql-stats", used=True, success=True)

        got = store.get("test-py-mysql-stats")
        assert got.use_count == 6
        assert got.success_count == 5

        store.delete("test-py-mysql-stats")

    def test_query_by_task_class(self, store):
        for i in range(3):
            g = _make_gene(f"test-py-mysql-q-{i}")
            g.task_class = "query-target"
            store.save(g)

        results = store.query(StoreQuery(task_class="query-target", limit=10))
        assert len(results) >= 3

        for i in range(3):
            store.delete(f"test-py-mysql-q-{i}")

    def test_list_and_get_unsynced(self, store):
        gene = _make_gene("test-py-mysql-list")
        store.save(gene)

        genes = store.list_genes(limit=100)
        assert any(g.gene_id == "test-py-mysql-list" for g in genes)

        unsynced = store.get_unsynced()
        assert any(g.gene_id == "test-py-mysql-list" for g in unsynced)

        store.delete("test-py-mysql-list")

    def test_mark_synced(self, store):
        gene = _make_gene("test-py-mysql-sync")
        store.save(gene)

        now = datetime.now(timezone.utc)
        store.mark_synced("test-py-mysql-sync", now)

        got = store.get("test-py-mysql-sync")
        assert got.synced_at is not None

        store.delete("test-py-mysql-sync")

    def test_sync_log(self, store):
        entry = SyncLogEntry(
            direction="push",
            gene_id="test-log-gene",
            status="success",
            remote_url="https://hub.example.com",
            timestamp=datetime.now(timezone.utc),
        )
        store.log_sync(entry)

        logs = store.get_sync_log(limit=10)
        assert len(logs) > 0
        assert any(e.gene_id == "test-log-gene" for e in logs)
