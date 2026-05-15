from __future__ import annotations

import json
import threading
from dataclasses import dataclass
from datetime import datetime, timezone
from typing import Any

from oris_sdk.gene import Gene, StoreQuery, SyncLogEntry


@dataclass
class MySQLConfig:
    host: str = "127.0.0.1"
    port: int = 3306
    user: str = ""
    password: str = ""
    database: str = "oris"
    charset: str = "utf8mb4"


class MySQLStore:
    def __init__(self, config: MySQLConfig):
        try:
            import pymysql
        except ImportError as e:
            raise ImportError(
                "pymysql is required for MySQLStore. "
                "Install with: pip install oris-rt-sdk[mysql]"
            ) from e

        self._conn = pymysql.connect(
            host=config.host,
            port=config.port,
            user=config.user,
            password=config.password,
            database=config.database,
            charset=config.charset,
            autocommit=True,
            cursorclass=pymysql.cursors.DictCursor,
        )
        self._lock = threading.Lock()
        self._migrate()

    @classmethod
    def from_url(cls, url: str) -> "MySQLStore":
        """Create from a DSN string like 'user:pass@host:port/db'."""
        try:
            import pymysql
        except ImportError as e:
            raise ImportError(
                "pymysql is required for MySQLStore. "
                "Install with: pip install oris-rt-sdk[mysql]"
            ) from e

        user_pass, rest = url.split("@", 1)
        user, password = user_pass.split(":", 1)
        host_port, database = rest.split("/", 1)
        if ":" in host_port:
            host, port_str = host_port.split(":", 1)
            port = int(port_str)
        else:
            host = host_port
            port = 3306

        config = MySQLConfig(
            host=host, port=port, user=user, password=password, database=database
        )
        return cls(config)

    def close(self) -> None:
        self._conn.close()

    def _migrate(self) -> None:
        stmts = [
            """CREATE TABLE IF NOT EXISTS genes (
                gene_id        VARCHAR(255) PRIMARY KEY,
                name           VARCHAR(512) NOT NULL,
                task_class     VARCHAR(255) NOT NULL,
                confidence     DOUBLE NOT NULL DEFAULT 0.0,
                strategy       JSON,
                signals        JSON,
                validation     JSON,
                quality_score  DOUBLE DEFAULT 0.0,
                use_count      INT DEFAULT 0,
                success_count  INT DEFAULT 0,
                contributor_id VARCHAR(255) DEFAULT '',
                source         VARCHAR(64) NOT NULL DEFAULT 'local',
                synced_at      DATETIME(3),
                created_at     DATETIME(3) NOT NULL,
                updated_at     DATETIME(3) NOT NULL,
                INDEX idx_genes_task_class (task_class),
                INDEX idx_genes_confidence (confidence DESC),
                INDEX idx_genes_source (source),
                INDEX idx_genes_synced_at (synced_at)
            ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4""",
            """CREATE TABLE IF NOT EXISTS sync_log (
                id             BIGINT AUTO_INCREMENT PRIMARY KEY,
                direction      VARCHAR(16) NOT NULL,
                gene_id        VARCHAR(255) NOT NULL,
                status         VARCHAR(32) NOT NULL,
                remote_url     TEXT,
                error_message  TEXT,
                timestamp      DATETIME(3) NOT NULL,
                INDEX idx_sync_log_timestamp (timestamp DESC),
                INDEX idx_sync_log_gene (gene_id)
            ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4""",
        ]
        with self._lock:
            with self._conn.cursor() as cur:
                for stmt in stmts:
                    try:
                        cur.execute(stmt)
                    except Exception as e:
                        if "Duplicate" not in str(e):
                            raise

    def save(self, gene: Gene) -> None:
        with self._lock:
            with self._conn.cursor() as cur:
                cur.execute(
                    """INSERT INTO genes
                    (gene_id, name, task_class, confidence, strategy, signals, validation,
                     quality_score, use_count, success_count, contributor_id, source,
                     synced_at, created_at, updated_at)
                    VALUES (%s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s)
                    ON DUPLICATE KEY UPDATE
                        name=VALUES(name), task_class=VALUES(task_class),
                        confidence=VALUES(confidence), strategy=VALUES(strategy),
                        signals=VALUES(signals), validation=VALUES(validation),
                        quality_score=VALUES(quality_score), use_count=VALUES(use_count),
                        success_count=VALUES(success_count), contributor_id=VALUES(contributor_id),
                        source=VALUES(source), synced_at=VALUES(synced_at),
                        updated_at=VALUES(updated_at)""",
                    (
                        gene.gene_id, gene.name, gene.task_class, gene.confidence,
                        json.dumps(gene.strategy) if gene.strategy else None,
                        json.dumps(gene.signals) if gene.signals else None,
                        json.dumps(gene.validation) if gene.validation else None,
                        gene.quality_score, gene.use_count, gene.success_count,
                        gene.contributor_id, gene.source,
                        gene.synced_at, gene.created_at, gene.updated_at,
                    ),
                )

    def get(self, gene_id: str) -> Gene | None:
        with self._conn.cursor() as cur:
            cur.execute(
                """SELECT gene_id, name, task_class, confidence,
                    strategy, signals, validation, quality_score, use_count, success_count,
                    contributor_id, source, synced_at, created_at, updated_at
                    FROM genes WHERE gene_id = %s""",
                (gene_id,),
            )
            row = cur.fetchone()
        if row is None:
            return None
        return self._row_to_gene(row)

    def query(self, q: StoreQuery) -> list[Gene]:
        clauses: list[str] = []
        args: list[Any] = []

        if q.task_class:
            clauses.append("task_class = %s")
            args.append(q.task_class)
        if q.min_confidence > 0:
            clauses.append("confidence >= %s")
            args.append(q.min_confidence)
        if q.source:
            clauses.append("source = %s")
            args.append(q.source)
        if q.q:
            clauses.append("(name LIKE %s OR gene_id LIKE %s)")
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
            FROM genes {where} ORDER BY {order_by} LIMIT %s OFFSET %s"""
        args.extend([limit, q.offset])

        with self._conn.cursor() as cur:
            cur.execute(sql, args)
            rows = cur.fetchall()
        return [self._row_to_gene(r) for r in rows]

    def delete(self, gene_id: str) -> None:
        with self._lock:
            with self._conn.cursor() as cur:
                cur.execute("DELETE FROM genes WHERE gene_id = %s", (gene_id,))

    def update_stats(self, gene_id: str, used: bool, success: bool) -> None:
        with self._lock:
            now = datetime.now(timezone.utc)
            with self._conn.cursor() as cur:
                if used and success:
                    cur.execute(
                        "UPDATE genes SET use_count = use_count + 1, success_count = success_count + 1, updated_at = %s WHERE gene_id = %s",
                        (now, gene_id),
                    )
                elif used:
                    cur.execute(
                        "UPDATE genes SET use_count = use_count + 1, updated_at = %s WHERE gene_id = %s",
                        (now, gene_id),
                    )

    def list_genes(self, limit: int = 50, offset: int = 0) -> list[Gene]:
        return self.query(StoreQuery(limit=limit, offset=offset))

    def get_unsynced(self) -> list[Gene]:
        return self.query(StoreQuery(source="local", limit=1000))

    def mark_synced(self, gene_id: str, synced_at: datetime) -> None:
        with self._lock:
            now = datetime.now(timezone.utc)
            with self._conn.cursor() as cur:
                cur.execute(
                    "UPDATE genes SET synced_at = %s, updated_at = %s WHERE gene_id = %s",
                    (synced_at, now, gene_id),
                )

    def log_sync(self, entry: SyncLogEntry) -> None:
        with self._lock:
            with self._conn.cursor() as cur:
                cur.execute(
                    "INSERT INTO sync_log (direction, gene_id, status, remote_url, error_message, timestamp) VALUES (%s, %s, %s, %s, %s, %s)",
                    (entry.direction, entry.gene_id, entry.status,
                     entry.remote_url, entry.error_message, entry.timestamp),
                )

    def get_sync_log(self, limit: int = 50) -> list[SyncLogEntry]:
        with self._conn.cursor() as cur:
            cur.execute(
                "SELECT id, direction, gene_id, status, remote_url, error_message, timestamp FROM sync_log ORDER BY timestamp DESC LIMIT %s",
                (limit,),
            )
            rows = cur.fetchall()
        entries = []
        for r in rows:
            entries.append(SyncLogEntry(
                id=r["id"], direction=r["direction"], gene_id=r["gene_id"],
                status=r["status"], remote_url=r["remote_url"] or "",
                error_message=r["error_message"] or "",
                timestamp=r["timestamp"] if isinstance(r["timestamp"], datetime) else datetime.fromisoformat(str(r["timestamp"])),
            ))
        return entries

    @staticmethod
    def _row_to_gene(row: dict[str, Any]) -> Gene:
        strategy = row["strategy"]
        if isinstance(strategy, str):
            strategy = json.loads(strategy)
        signals = row["signals"]
        if isinstance(signals, str):
            signals = json.loads(signals)
        validation = row["validation"]
        if isinstance(validation, str):
            validation = json.loads(validation)

        return Gene(
            gene_id=row["gene_id"],
            name=row["name"],
            task_class=row["task_class"],
            confidence=row["confidence"],
            strategy=strategy or {},
            signals=signals or {},
            validation=validation or {},
            quality_score=row["quality_score"],
            use_count=row["use_count"],
            success_count=row["success_count"],
            contributor_id=row["contributor_id"] or "",
            source=row["source"],
            synced_at=row["synced_at"],
            created_at=row["created_at"],
            updated_at=row["updated_at"],
        )
