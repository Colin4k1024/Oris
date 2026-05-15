import Database from "better-sqlite3";
import type { Gene, StoreQuery, SyncLogEntry } from "./gene.js";

export class LocalStore {
  private db: Database.Database;

  constructor(path: string = "oris_genes.db") {
    this.db = new Database(path);
    this.db.pragma("journal_mode = WAL");
    this.db.pragma("busy_timeout = 5000");
    this.migrate();
  }

  close(): void {
    this.db.close();
  }

  private migrate(): void {
    this.db.exec(`
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
    `);
  }

  save(gene: Gene): void {
    const stmt = this.db.prepare(`
      INSERT OR REPLACE INTO genes
      (gene_id, name, task_class, confidence, strategy, signals, validation,
       quality_score, use_count, success_count, contributor_id, source,
       synced_at, created_at, updated_at)
      VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    `);
    stmt.run(
      gene.geneId, gene.name, gene.taskClass, gene.confidence,
      gene.strategy ? JSON.stringify(gene.strategy) : null,
      gene.signals ? JSON.stringify(gene.signals) : null,
      gene.validation ? JSON.stringify(gene.validation) : null,
      gene.qualityScore, gene.useCount, gene.successCount,
      gene.contributorId, gene.source,
      gene.syncedAt?.toISOString() ?? null,
      gene.createdAt.toISOString(), gene.updatedAt.toISOString(),
    );
  }

  get(geneId: string): Gene | null {
    const row = this.db.prepare(`
      SELECT gene_id, name, task_class, confidence,
        strategy, signals, validation, quality_score, use_count, success_count,
        contributor_id, source, synced_at, created_at, updated_at
      FROM genes WHERE gene_id = ?
    `).get(geneId) as Record<string, unknown> | undefined;
    if (!row) return null;
    return this.rowToGene(row);
  }

  query(q: StoreQuery): Gene[] {
    const clauses: string[] = [];
    const args: unknown[] = [];

    if (q.taskClass) {
      clauses.push("task_class = ?");
      args.push(q.taskClass);
    }
    if (q.minConfidence && q.minConfidence > 0) {
      clauses.push("confidence >= ?");
      args.push(q.minConfidence);
    }
    if (q.source) {
      clauses.push("source = ?");
      args.push(q.source);
    }
    if (q.q) {
      clauses.push("(name LIKE ? OR gene_id LIKE ?)");
      const pattern = `%${q.q}%`;
      args.push(pattern, pattern);
    }

    const where = clauses.length > 0 ? `WHERE ${clauses.join(" AND ")}` : "";
    const orderBy = q.orderBy || "created_at DESC";
    const limit = q.limit && q.limit > 0 ? q.limit : 50;
    const offset = q.offset || 0;

    const sql = `
      SELECT gene_id, name, task_class, confidence,
        strategy, signals, validation, quality_score, use_count, success_count,
        contributor_id, source, synced_at, created_at, updated_at
      FROM genes ${where} ORDER BY ${orderBy} LIMIT ? OFFSET ?
    `;
    args.push(limit, offset);

    const rows = this.db.prepare(sql).all(...args) as Record<string, unknown>[];
    return rows.map((r) => this.rowToGene(r));
  }

  delete(geneId: string): void {
    this.db.prepare("DELETE FROM genes WHERE gene_id = ?").run(geneId);
  }

  updateStats(geneId: string, used: boolean, success: boolean): void {
    const now = new Date().toISOString();
    if (used && success) {
      this.db.prepare(
        "UPDATE genes SET use_count = use_count + 1, success_count = success_count + 1, updated_at = ? WHERE gene_id = ?"
      ).run(now, geneId);
    } else if (used) {
      this.db.prepare(
        "UPDATE genes SET use_count = use_count + 1, updated_at = ? WHERE gene_id = ?"
      ).run(now, geneId);
    }
  }

  list(limit = 50, offset = 0): Gene[] {
    return this.query({ limit, offset });
  }

  getUnsynced(): Gene[] {
    return this.query({ source: "local", limit: 1000 });
  }

  markSynced(geneId: string, syncedAt: Date): void {
    this.db.prepare(
      "UPDATE genes SET synced_at = ?, updated_at = ? WHERE gene_id = ?"
    ).run(syncedAt.toISOString(), new Date().toISOString(), geneId);
  }

  logSync(entry: Omit<SyncLogEntry, "id">): void {
    this.db.prepare(
      "INSERT INTO sync_log (direction, gene_id, status, remote_url, error_message, timestamp) VALUES (?, ?, ?, ?, ?, ?)"
    ).run(entry.direction, entry.geneId, entry.status, entry.remoteUrl, entry.errorMessage, entry.timestamp.toISOString());
  }

  getSyncLog(limit = 50): SyncLogEntry[] {
    const rows = this.db.prepare(
      "SELECT id, direction, gene_id, status, remote_url, error_message, timestamp FROM sync_log ORDER BY timestamp DESC LIMIT ?"
    ).all(limit) as Record<string, unknown>[];

    return rows.map((r) => ({
      id: r.id as number,
      direction: r.direction as string,
      geneId: r.gene_id as string,
      status: r.status as string,
      remoteUrl: (r.remote_url as string) || "",
      errorMessage: (r.error_message as string) || "",
      timestamp: new Date(r.timestamp as string),
    }));
  }

  private rowToGene(row: Record<string, unknown>): Gene {
    return {
      geneId: row.gene_id as string,
      name: row.name as string,
      taskClass: row.task_class as string,
      confidence: row.confidence as number,
      strategy: row.strategy ? JSON.parse(row.strategy as string) : undefined,
      signals: row.signals ? JSON.parse(row.signals as string) : undefined,
      validation: row.validation ? JSON.parse(row.validation as string) : undefined,
      qualityScore: row.quality_score as number,
      useCount: row.use_count as number,
      successCount: row.success_count as number,
      contributorId: (row.contributor_id as string) || "",
      source: row.source as string,
      syncedAt: row.synced_at ? new Date(row.synced_at as string) : undefined,
      createdAt: new Date(row.created_at as string),
      updatedAt: new Date(row.updated_at as string),
    };
  }
}
