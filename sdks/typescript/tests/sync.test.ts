import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { LocalStore } from "../src/store.js";
import { SyncManager } from "../src/sync.js";
import type { Gene } from "../src/gene.js";
import type { ExperienceClient } from "../src/experience.js";

function sampleGene(id = "gene-1"): Gene {
  const now = new Date();
  return {
    geneId: id,
    name: `test-${id}`,
    taskClass: "bugfix",
    confidence: 0.85,
    strategy: { approach: "retry" },
    signals: {},
    validation: {},
    qualityScore: 0.9,
    useCount: 5,
    successCount: 4,
    contributorId: "node-1",
    source: "local",
    createdAt: now,
    updatedAt: now,
  };
}

function mockExperience(overrides?: Partial<ExperienceClient>) {
  return {
    share: vi.fn().mockResolvedValue({ gene_id: "gene-1", status: "published" }),
    fetch_: vi.fn().mockResolvedValue({ assets: [], next_cursor: null, sync_audit: { total_available: 0, returned: 0 } }),
    registerPublicKey: vi.fn().mockResolvedValue(undefined),
    ...overrides,
  } as unknown as ExperienceClient;
}

describe("SyncManager", () => {
  let store: LocalStore;

  beforeEach(() => {
    store = new LocalStore(":memory:");
  });
  afterEach(() => {
    store.close();
  });

  it("pushToHub pushes unsynced genes", async () => {
    store.save(sampleGene());
    const exp = mockExperience();
    const mgr = new SyncManager({ store, experience: exp });

    const result = await mgr.pushToHub();
    expect(result.pushed).toBe(1);
    expect(result.failed).toBe(0);
    expect(exp.share).toHaveBeenCalledOnce();

    const got = store.get("gene-1")!;
    expect(got.syncedAt).toBeDefined();

    const logs = store.getSyncLog(10);
    expect(logs).toHaveLength(1);
    expect(logs[0].status).toBe("success");
  });

  it("pushToHub with specific gene ids", async () => {
    store.save(sampleGene("gene-1"));
    store.save(sampleGene("gene-2"));
    const exp = mockExperience();
    const mgr = new SyncManager({ store, experience: exp });

    const result = await mgr.pushToHub({ geneIds: ["gene-1"] });
    expect(result.pushed).toBe(1);
    expect(exp.share).toHaveBeenCalledOnce();
  });

  it("pushToHub handles failure", async () => {
    store.save(sampleGene());
    const exp = mockExperience({
      share: vi.fn().mockRejectedValue(new Error("network error")),
    });
    const mgr = new SyncManager({ store, experience: exp });

    const result = await mgr.pushToHub();
    expect(result.failed).toBe(1);
    expect(result.pushed).toBe(0);
    expect(result.errors[0].message).toContain("network error");
  });

  it("pullFromHub imports new genes", async () => {
    const exp = mockExperience({
      fetch_: vi.fn().mockResolvedValue({
        assets: [
          { type: "gene", id: "remote-1", confidence: 0.9, quality_score: 0.85, use_count: 10, success_count: 8 },
        ],
        next_cursor: null,
        sync_audit: { total_available: 1, returned: 1 },
      }),
    });
    const mgr = new SyncManager({ store, experience: exp });

    const imported = await mgr.pullFromHub({ limit: 10 });
    expect(imported).toBe(1);

    const got = store.get("remote-1")!;
    expect(got.source).toBe("hub");
    expect(got.confidence).toBe(0.9);
  });

  it("pullFromHub skips on conflict", async () => {
    const now = new Date();
    store.save({
      geneId: "remote-1", name: "existing", taskClass: "feature",
      confidence: 0.5, qualityScore: 0, useCount: 0, successCount: 0,
      contributorId: "", source: "local", createdAt: now, updatedAt: now,
    });

    const exp = mockExperience({
      fetch_: vi.fn().mockResolvedValue({
        assets: [{ type: "gene", id: "remote-1", confidence: 0.9 }],
        next_cursor: null,
        sync_audit: { total_available: 1, returned: 1 },
      }),
    });
    const mgr = new SyncManager({ store, experience: exp });

    const imported = await mgr.pullFromHub();
    expect(imported).toBe(0);

    const logs = store.getSyncLog(10);
    expect(logs.some((l) => l.status === "conflict")).toBe(true);
  });

  it("pullFromHub merges stats when same task class", async () => {
    const now = new Date();
    store.save({
      geneId: "gene-1", name: "existing", taskClass: "unknown",
      confidence: 0.5, qualityScore: 0.3, useCount: 3, successCount: 2,
      contributorId: "", source: "local", createdAt: now, updatedAt: now,
    });

    const exp = mockExperience({
      fetch_: vi.fn().mockResolvedValue({
        assets: [{ type: "gene", id: "gene-1", confidence: 0.9, quality_score: 0.8, use_count: 10, success_count: 8 }],
        next_cursor: null,
        sync_audit: { total_available: 1, returned: 1 },
      }),
    });
    const mgr = new SyncManager({ store, experience: exp });

    const imported = await mgr.pullFromHub();
    expect(imported).toBe(1);

    const got = store.get("gene-1")!;
    expect(got.confidence).toBe(0.9);
    expect(got.useCount).toBe(10);
    expect(got.successCount).toBe(8);
  });

  it("throws when no experience client", async () => {
    const mgr = new SyncManager({ store });
    await expect(mgr.pushToHub()).rejects.toThrow("experience client not configured");
    await expect(mgr.pullFromHub()).rejects.toThrow("experience client not configured");
  });
});
