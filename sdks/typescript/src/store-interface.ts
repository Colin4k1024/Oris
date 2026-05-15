import type { Gene, StoreQuery, SyncLogEntry } from "./gene.js";

export interface GeneStore {
  close(): void;
  save(gene: Gene): void | Promise<void>;
  get(geneId: string): Gene | null | Promise<Gene | null>;
  delete(geneId: string): void | Promise<void>;
  query(q: StoreQuery): Gene[] | Promise<Gene[]>;
  updateStats(geneId: string, used: boolean, success: boolean): void | Promise<void>;
  list(limit?: number, offset?: number): Gene[] | Promise<Gene[]>;
  getUnsynced(): Gene[] | Promise<Gene[]>;
  markSynced(geneId: string, syncedAt: Date): void | Promise<void>;
  logSync(entry: Omit<SyncLogEntry, "id">): void | Promise<void>;
  getSyncLog(limit?: number): SyncLogEntry[] | Promise<SyncLogEntry[]>;
}
