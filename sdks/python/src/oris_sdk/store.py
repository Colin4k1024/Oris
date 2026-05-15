from __future__ import annotations

import json
import sqlite3
import threading
from datetime import datetime, timezone
from typing import Any

from oris_sdk.gene import Gene, StoreQuery, SyncLogEntry


class LocalStore:
    def __init__(self, path: str = "oris_genes.db"):
        self._db = sqlite3.connect(path, check_same_thread=False)
        self._db.execute("PRAGMA journal_mode=WAL")
        self._db.execute("PRAGMA busy_timeout=5000")
        self._lock = threading.Lock()
        self._migrate()

    def close(self) -> None:
        self._db.close()

    def _migrate(self) -> None:
        self._db.executescript("""
            CREATE TABLE IF NOT EXISTS genes (
                gene_id        TEXT PRIMARY KEY,
                name           TEXT NOT NULL,
                task_class     TEXT NOT NULL,
                confidence     REAL NOT NULL DEFAULT 0.0,
                strategy       TEXT,
                signals        TEXT,
                validation     TEXT,
                quality_score  REAL DEFAULT 0.0,
                use_count      INTEGER DEFAULT 0,
                success_count  INTEGER DEFAULT 0,
                contributor_id TEXT DEFAULT '',
                source         TEXT NOT NULL DEFAULT 'local',
                synced_at      TEXT,
                created_at     TEXT NOT NULL,
                updated_at     TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_genes_task_class ON genes(task_class);
            CREATE INDEX IF NOT EXISTS idx_genes_confidence ON genes(confidence DESC);
            CREATE INDEX IF NOT EXISTS idx_genes_source ON genes(source);
            CREATE INDEX IF NOT EXISTS idx_genes_synced_at ON genes(synced_at);

            CREATE TABLE IF NOT EXISTS sync_log (
                id             INTEGER PRIMARY KEY AUTOINCREMENT,
                direction      TEXT NOT NULL,
                gene_id        TEXT NOT NULL,
                status         TEXT NOT NULL,
                remote_url     TEXT,
                error_message  TEXT,
                timestamp      TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_sync_log_timestamp ON sync_log(timestamp DESC);
            CREATE INDEX IF NOT EXISTS idx_sync_log_gene ON sync_log(gene_id);
        """)

    def save(self, gene: Gene) -> None:
        with self._lock:
            synced = gene.synced_at.isoformat() if gene.synced_at else None
            self._db.execute(
                """INSERT OR REPLACE INTO genes
                (gene_id, name, task_class, confidence, strategy, signals, validation,
                 quality_score, use_count, success_count, contributor_id, source,
                 synced_at, created_at, updated_at)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
                (
                    gene.gene_id, gene.name, gene.task_class, gene.confidence,
                    json.dumps(gene.strategy) if gene.strategy else None,
                    json.dumps(gene.signals) if gene.signals else None,
                    json.dumps(gene.validation) if gene.validation else None,
                    gene.quality_score, gene.use_count, gene.success_count,
                    gene.contributor_id, gene.source, synced,
                    gene.created_at.isoformat(), gene.updated_at.isoformat(),
                ),
            )
            self._db.commit()

    def get(self, gene_id: str) -> Gene | None:
        row = self._db.execute(
            """SELECT gene_id, name, task_class, confidence,
                strategy, signals, validation, quality_score, use_count, success_count,
                contributor_id, source, synced_at, created_at, updated_at
                FROM genes WHERE gene_id = ?""",
            (gene_id,),
        ).fetchone()
        if row is None:
            return None
        return self._row_to_gene(row)

    def query(self, q: StoreQuery) -> list[Gene]:
        clauses: list[str] = []
        args: list[Any] = []

        if q.task_class:
            clauses.append("task_class = ?")
            args.append(q.task_class)
        if q.min_confidence > 0:
            clauses.append("confidence >= ?")
            args.append(q.min_confidence)
        if q.source:
            clauses.append("source = ?")
            args.append(q.source)
        if q.q:
            clauses.append("(name LIKE ? OR gene_id LIKE ?)")
            pattern = f"%{q.q}%"
            args.extend([pattern, pattern])

        where = ""
        if clauses:
            where = "WHERE " + " AND ".join(clauses)

        order_by = q.order_by or "created_at DESC"
        limit = q.limit if q.limit > 0 else 50

        sql = f"""SELECT gene_id, name, task_class, confidence,
            strategy, signals, validation, quality_score, use_count, success_count,
            contributor_id, source, synced_at, created_at, updated_at
            FROM genes {where} ORDER BY {order_by} LIMIT ? OFFSET ?"""
        args.extend([limit, q.offset])

        rows = self._db.execute(sql, args).fetchall()
        return [self._row_to_gene(r) for r in rows]

    def delete(self, gene_id: str) -> None:
        with self._lock:
            self._db.execute("DELETE FROM genes WHERE gene_id = ?", (gene_id,))
            self._db.commit()

    def update_stats(self, gene_id: str, used: bool, success: bool) -> None:
        with self._lock:
            now = datetime.now(timezone.utc).isoformat()
            if used and success:
                self._db.execute(
                    "UPDATE genes SET use_count = use_count + 1, success_count = success_count + 1, updated_at = ? WHERE gene_id = ?",
                    (now, gene_id),
                )
            elif used:
                self._db.execute(
                    "UPDATE genes SET use_count = use_count + 1, updated_at = ? WHERE gene_id = ?",
                    (now, gene_id),
                )
            self._db.commit()

    def list_genes(self, limit: int = 50, offset: int = 0) -> list[Gene]:
        return self.query(StoreQuery(limit=limit, offset=offset))

    def get_unsynced(self) -> list[Gene]:
        return self.query(StoreQuery(source="local", limit=1000))

    def mark_synced(self, gene_id: str, synced_at: datetime) -> None:
        with self._lock:
            now = datetime.now(timezone.utc).isoformat()
            self._db.execute(
                "UPDATE genes SET synced_at = ?, updated_at = ? WHERE gene_id = ?",
                (synced_at.isoformat(), now, gene_id),
            )
            self._db.commit()

    def log_sync(self, entry: SyncLogEntry) -> None:
        with self._lock:
            self._db.execute(
                "INSERT INTO sync_log (direction, gene_id, status, remote_url, error_message, timestamp) VALUES (?, ?, ?, ?, ?, ?)",
                (entry.direction, entry.gene_id, entry.status, entry.remote_url,
                 entry.error_message, entry.timestamp.isoformat()),
            )
            self._db.commit()

    def get_sync_log(self, limit: int = 50) -> list[SyncLogEntry]:
        rows = self._db.execute(
            "SELECT id, direction, gene_id, status, remote_url, error_message, timestamp FROM sync_log ORDER BY timestamp DESC LIMIT ?",
            (limit,),
        ).fetchall()
        entries = []
        for r in rows:
            entries.append(SyncLogEntry(
                id=r[0], direction=r[1], gene_id=r[2], status=r[3],
                remote_url=r[4] or "", error_message=r[5] or "",
                timestamp=datetime.fromisoformat(r[6]),
            ))
        return entries

    @staticmethod
    def _row_to_gene(row: tuple) -> Gene:
        synced = datetime.fromisoformat(row[12]) if row[12] else None
        return Gene(
            gene_id=row[0],
            name=row[1],
            task_class=row[2],
            confidence=row[3],
            strategy=json.loads(row[4]) if row[4] else {},
            signals=json.loads(row[5]) if row[5] else {},
            validation=json.loads(row[6]) if row[6] else {},
            quality_score=row[7],
            use_count=row[8],
            success_count=row[9],
            contributor_id=row[10],
            source=row[11],
            synced_at=synced,
            created_at=datetime.fromisoformat(row[13]),
            updated_at=datetime.fromisoformat(row[14]),
        )
