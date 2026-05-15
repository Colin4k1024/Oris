export interface Gene {
  geneId: string;
  name: string;
  taskClass: string;
  confidence: number;
  strategy?: Record<string, unknown>;
  signals?: Record<string, unknown>;
  validation?: Record<string, unknown>;
  qualityScore: number;
  useCount: number;
  successCount: number;
  contributorId: string;
  source: string;
  syncedAt?: Date;
  createdAt: Date;
  updatedAt: Date;
}

export interface StoreQuery {
  q?: string;
  taskClass?: string;
  minConfidence?: number;
  source?: string;
  orderBy?: string;
  limit?: number;
  offset?: number;
}

export interface PushOpts {
  geneIds?: string[];
}

export interface PullOpts {
  q?: string;
  minConfidence?: number;
  limit?: number;
  taskClass?: string;
}

export interface PushResult {
  pushed: number;
  failed: number;
  errors: PushError[];
}

export interface PushError {
  geneId: string;
  message: string;
}

export interface SyncLogEntry {
  id: number;
  direction: string;
  geneId: string;
  status: string;
  remoteUrl: string;
  errorMessage: string;
  timestamp: Date;
}
