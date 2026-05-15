import type { ExperienceClient } from "./experience.js";
import type { HubClient } from "./hub.js";
import type { Gene, PullOpts, PushError, PushOpts, PushResult, SyncLogEntry } from "./gene.js";
import type { LocalStore } from "./store.js";
import type { NetworkAsset } from "./types.js";

export interface SyncConfig {
  store: LocalStore;
  experience?: ExperienceClient;
  hub?: HubClient;
}

export class SyncManager {
  private store: LocalStore;
  private experience?: ExperienceClient;
  private hub?: HubClient;

  constructor(config: SyncConfig) {
    this.store = config.store;
    this.experience = config.experience;
    this.hub = config.hub;
  }

  async pushToHub(opts?: PushOpts): Promise<PushResult> {
    if (!this.experience) throw new Error("experience client not configured");

    let genes: Gene[];
    if (opts?.geneIds && opts.geneIds.length > 0) {
      genes = opts.geneIds
        .map((id) => this.store.get(id))
        .filter((g): g is Gene => g !== null);
    } else {
      genes = this.store.getUnsynced();
    }

    const result: PushResult = { pushed: 0, failed: 0, errors: [] };

    for (const gene of genes) {
      const payload = geneToPayload(gene);
      const entry: Omit<SyncLogEntry, "id"> = {
        direction: "push",
        geneId: gene.geneId,
        status: "",
        remoteUrl: "",
        errorMessage: "",
        timestamp: new Date(),
      };

      try {
        await this.experience.share(payload);
        entry.status = "success";
        result.pushed++;
        this.store.markSynced(gene.geneId, new Date());
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        entry.status = "failed";
        entry.errorMessage = msg;
        result.failed++;
        result.errors.push({ geneId: gene.geneId, message: msg });
      }

      this.store.logSync(entry);
    }
    return result;
  }

  async pullFromHub(opts?: PullOpts): Promise<number> {
    if (!this.experience) throw new Error("experience client not configured");

    const resp = await this.experience.fetch_({
      q: opts?.q,
      minConfidence: opts?.minConfidence,
      limit: opts?.limit,
    });

    let imported = 0;
    for (const asset of resp.assets) {
      let gene = assetToGene(asset);
      const existing = this.store.get(gene.geneId);

      if (existing) {
        if (coreFieldsMatch(existing, gene)) {
          gene = mergeStats(existing, gene);
        } else {
          this.store.logSync({
            direction: "pull",
            geneId: gene.geneId,
            status: "conflict",
            remoteUrl: "",
            errorMessage: "core fields differ, skipped",
            timestamp: new Date(),
          });
          continue;
        }
      }

      this.store.save(gene);
      this.store.logSync({
        direction: "pull",
        geneId: gene.geneId,
        status: "success",
        remoteUrl: "",
        errorMessage: "",
        timestamp: new Date(),
      });
      imported++;
    }
    return imported;
  }

  getSyncLog(limit = 50): SyncLogEntry[] {
    return this.store.getSyncLog(limit);
  }
}

function geneToPayload(g: Gene): Record<string, unknown> {
  const p: Record<string, unknown> = {
    type: "gene",
    id: g.geneId,
    confidence: g.confidence,
    quality_score: g.qualityScore,
    use_count: g.useCount,
    success_count: g.successCount,
    created_at: g.createdAt.toISOString(),
  };
  if (g.strategy) p.strategy = g.strategy;
  if (g.signals) p.signals = g.signals;
  if (g.validation) p.validation = g.validation;
  if (g.contributorId) p.contributor_id = g.contributorId;
  return p;
}

function assetToGene(a: NetworkAsset): Gene {
  const now = new Date();
  return {
    geneId: a.id,
    name: a.id,
    taskClass: "unknown",
    confidence: a.confidence ?? 0,
    qualityScore: a.quality_score ?? 0,
    useCount: a.use_count ?? 0,
    successCount: a.success_count ?? 0,
    contributorId: a.contributor_id ?? "",
    source: "hub",
    syncedAt: now,
    createdAt: a.created_at ? new Date(a.created_at) : now,
    updatedAt: now,
  };
}

function coreFieldsMatch(existing: Gene, incoming: Gene): boolean {
  return existing.taskClass === incoming.taskClass;
}

function mergeStats(existing: Gene, incoming: Gene): Gene {
  return {
    ...existing,
    useCount: Math.max(existing.useCount, incoming.useCount),
    successCount: Math.max(existing.successCount, incoming.successCount),
    confidence: Math.max(existing.confidence, incoming.confidence),
    qualityScore: Math.max(existing.qualityScore, incoming.qualityScore),
    updatedAt: new Date(),
    syncedAt: new Date(),
  };
}
