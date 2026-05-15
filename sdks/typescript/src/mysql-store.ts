import type { Gene, StoreQuery, SyncLogEntry } from "./gene.js";
import type { GeneStore } from "./store-interface.js";

export interface MySQLConfig {
  host?: string;
  port?: number;
  user: string;
  password: string;
  database?: string;
}

interface PoolConnection {
  execute(sql: string, values?: unknown[]): Promise<[unknown[], unknown]>;
  query(sql: string, values?: unknown[]): Promise<[unknown[], unknown]>;
  end(): Promise<void>;
}

export class MySQLStore implements GeneStore {
  private pool: PoolConnection;

  private constructor(pool: PoolConnection) {
    this.pool = pool;
  }

  static async create(config: MySQLConfig): Promise<MySQLStore> {
    let mysql2: { createPool: (opts: Record<string, unknown>) => PoolConnection };
    try {
      // eslint-disable-next-line @typescript-eslint/no-require-imports
      mysql2 = await (Function('return import("mysql2/promise")')() as Promise<typeof mysql2>);
    } catch {
      throw new Error(
        "mysql2 is required for MySQLStore. Install with: npm install mysql2"
      );
    }

    const pool = mysql2.createPool({
      host: config.host || "127.0.0.1",
      port: config.port || 3306,
      user: config.user,
      password: config.password,
      database: config.database || "oris",
      charset: "utf8mb4",
      waitForConnections: true,
      connectionLimit: 25,
      idleTimeout: 300000,
    });

    const store = new MySQLStore(pool);
    await store.migrate();
    return store;
  }

  private async migrate(): Promise<void> {
    await this.pool.execute(`
      CREATE TABLE IF NOT EXISTS genes (
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
      ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
    `);

    await this.pool.execute(`
      CREATE TABLE IF NOT EXISTS sync_log (
        id             BIGINT AUTO_INCREMENT PRIMARY KEY,
        direction      VARCHAR(16) NOT NULL,
        gene_id        VARCHAR(255) NOT NULL,
        status         VARCHAR(32) NOT NULL,
        remote_url     TEXT,
        error_message  TEXT,
        timestamp      DATETIME(3) NOT NULL,
        INDEX idx_sync_log_timestamp (timestamp DESC),
        INDEX idx_sync_log_gene (gene_id)
      ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
    `);
  }

  close(): void {
    this.pool.end();
  }

  async save(gene: Gene): Promise<void> {
    await this.pool.execute(
      `INSERT INTO genes
      (gene_id, name, task_class, confidence, strategy, signals, validation,
       quality_score, use_count, success_count, contributor_id, source,
       synced_at, created_at, updated_at)
      VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
      ON DUPLICATE KEY UPDATE
        name=VALUES(name), task_class=VALUES(task_class),
        confidence=VALUES(confidence), strategy=VALUES(strategy),
        signals=VALUES(signals), validation=VALUES(validation),
        quality_score=VALUES(quality_score), use_count=VALUES(use_count),
        success_count=VALUES(success_count), contributor_id=VALUES(contributor_id),
        source=VALUES(source), synced_at=VALUES(synced_at),
        updated_at=VALUES(updated_at)`,
      [
        gene.geneId, gene.name, gene.taskClass, gene.confidence,
        gene.strategy ? JSON.stringify(gene.strategy) : null,
        gene.signals ? JSON.stringify(gene.signals) : null,
        gene.validation ? JSON.stringify(gene.validation) : null,
        gene.qualityScore, gene.useCount, gene.successCount,
        gene.contributorId, gene.source,
        gene.syncedAt ?? null,
        gene.createdAt, gene.updatedAt,
      ],
    );
  }

  async get(geneId: string): Promise<Gene | null> {
    const [rows] = await this.pool.execute(
      `SELECT gene_id, name, task_class, confidence,
        strategy, signals, validation, quality_score, use_count, success_count,
        contributor_id, source, synced_at, created_at, updated_at
        FROM genes WHERE gene_id = ?`,
      [geneId],
    );
    const arr = rows as Record<string, unknown>[];
    if (arr.length === 0) return null;
    return rowToGene(arr[0]);
  }

  async query(q: StoreQuery): Promise<Gene[]> {
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

    args.push(limit, offset);

    const [rows] = await this.pool.execute(
      `SELECT gene_id, name, task_class, confidence,
        strategy, signals, validation, quality_score, use_count, success_count,
        contributor_id, source, synced_at, created_at, updated_at
        FROM genes ${where} ORDER BY ${orderBy} LIMIT ? OFFSET ?`,
      args,
    );
    return (rows as Record<string, unknown>[]).map(rowToGene);
  }

  async delete(geneId: string): Promise<void> {
    await this.pool.execute("DELETE FROM genes WHERE gene_id = ?", [geneId]);
  }

  async updateStats(geneId: string, used: boolean, success: boolean): Promise<void> {
    const now = new Date();
    if (used && success) {
      await this.pool.execute(
        "UPDATE genes SET use_count = use_count + 1, success_count = success_count + 1, updated_at = ? WHERE gene_id = ?",
        [now, geneId],
      );
    } else if (used) {
      await this.pool.execute(
        "UPDATE genes SET use_count = use_count + 1, updated_at = ? WHERE gene_id = ?",
        [now, geneId],
      );
    }
  }

  async list(limit = 50, offset = 0): Promise<Gene[]> {
    return this.query({ limit, offset });
  }

  async getUnsynced(): Promise<Gene[]> {
    return this.query({ source: "local", limit: 1000 });
  }

  async markSynced(geneId: string, syncedAt: Date): Promise<void> {
    await this.pool.execute(
      "UPDATE genes SET synced_at = ?, updated_at = ? WHERE gene_id = ?",
      [syncedAt, new Date(), geneId],
    );
  }

  async logSync(entry: Omit<SyncLogEntry, "id">): Promise<void> {
    await this.pool.execute(
      "INSERT INTO sync_log (direction, gene_id, status, remote_url, error_message, timestamp) VALUES (?, ?, ?, ?, ?, ?)",
      [entry.direction, entry.geneId, entry.status, entry.remoteUrl, entry.errorMessage, entry.timestamp],
    );
  }

  async getSyncLog(limit = 50): Promise<SyncLogEntry[]> {
    const [rows] = await this.pool.execute(
      "SELECT id, direction, gene_id, status, remote_url, error_message, timestamp FROM sync_log ORDER BY timestamp DESC LIMIT ?",
      [limit],
    );
    return (rows as Record<string, unknown>[]).map((r) => ({
      id: r.id as number,
      direction: r.direction as string,
      geneId: r.gene_id as string,
      status: r.status as string,
      remoteUrl: (r.remote_url as string) || "",
      errorMessage: (r.error_message as string) || "",
      timestamp: new Date(r.timestamp as string),
    }));
  }
}

function rowToGene(row: Record<string, unknown>): Gene {
  const parseJson = (v: unknown): Record<string, unknown> | undefined => {
    if (!v) return undefined;
    if (typeof v === "string") return JSON.parse(v);
    if (typeof v === "object") return v as Record<string, unknown>;
    return undefined;
  };

  return {
    geneId: row.gene_id as string,
    name: row.name as string,
    taskClass: row.task_class as string,
    confidence: row.confidence as number,
    strategy: parseJson(row.strategy),
    signals: parseJson(row.signals),
    validation: parseJson(row.validation),
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
