import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { LocalStore } from "../src/store.js";
import type { Gene } from "../src/gene.js";

function sampleGene(id = "gene-1"): Gene {
  const now = new Date();
  return {
    geneId: id,
    name: `test-${id}`,
    taskClass: "bugfix",
    confidence: 0.85,
    strategy: { approach: "retry" },
    signals: { error_rate: 0.1 },
    validation: { tests_passed: true },
    qualityScore: 0.9,
    useCount: 5,
    successCount: 4,
    contributorId: "node-1",
    source: "local",
    createdAt: now,
    updatedAt: now,
  };
}

describe("LocalStore", () => {
  let store: LocalStore;

  beforeEach(() => {
    store = new LocalStore(":memory:");
  });
  afterEach(() => {
    store.close();
  });

  it("save and get", () => {
    store.save(sampleGene());
    const got = store.get("gene-1");
    expect(got).not.toBeNull();
    expect(got!.geneId).toBe("gene-1");
    expect(got!.confidence).toBe(0.85);
    expect(got!.strategy).toEqual({ approach: "retry" });
  });

  it("get returns null for missing", () => {
    expect(store.get("nonexistent")).toBeNull();
  });

  it("upsert on save", () => {
    const g = sampleGene();
    store.save(g);
    g.confidence = 0.95;
    g.name = "updated";
    store.save(g);
    const got = store.get("gene-1")!;
    expect(got.confidence).toBe(0.95);
    expect(got.name).toBe("updated");
  });

  it("delete", () => {
    store.save(sampleGene());
    store.delete("gene-1");
    expect(store.get("gene-1")).toBeNull();
  });

  it("query by task class", () => {
    const g1 = sampleGene("gene-1");
    g1.taskClass = "bugfix";
    const g2 = sampleGene("gene-2");
    g2.taskClass = "feature";
    store.save(g1);
    store.save(g2);
    const results = store.query({ taskClass: "bugfix" });
    expect(results).toHaveLength(1);
    expect(results[0].geneId).toBe("gene-1");
  });

  it("query by min confidence", () => {
    const g1 = sampleGene("gene-low");
    g1.confidence = 0.3;
    const g2 = sampleGene("gene-high");
    g2.confidence = 0.9;
    store.save(g1);
    store.save(g2);
    const results = store.query({ minConfidence: 0.8 });
    expect(results).toHaveLength(1);
    expect(results[0].geneId).toBe("gene-high");
  });

  it("query by search term", () => {
    const g1 = sampleGene("gene-1");
    g1.name = "retry-handler";
    const g2 = sampleGene("gene-2");
    g2.name = "cache-warmer";
    store.save(g1);
    store.save(g2);
    const results = store.query({ q: "retry" });
    expect(results).toHaveLength(1);
    expect(results[0].name).toBe("retry-handler");
  });

  it("update stats", () => {
    const g = sampleGene();
    g.useCount = 0;
    g.successCount = 0;
    store.save(g);
    store.updateStats("gene-1", true, true);
    let got = store.get("gene-1")!;
    expect(got.useCount).toBe(1);
    expect(got.successCount).toBe(1);
    store.updateStats("gene-1", true, false);
    got = store.get("gene-1")!;
    expect(got.useCount).toBe(2);
    expect(got.successCount).toBe(1);
  });

  it("get unsynced and mark synced", () => {
    const g = sampleGene();
    g.source = "local";
    store.save(g);
    const unsynced = store.getUnsynced();
    expect(unsynced).toHaveLength(1);
    store.markSynced("gene-1", new Date());
    const got = store.get("gene-1")!;
    expect(got.syncedAt).toBeDefined();
  });

  it("sync log", () => {
    store.logSync({
      direction: "push",
      geneId: "gene-1",
      status: "success",
      remoteUrl: "http://localhost:3000",
      errorMessage: "",
      timestamp: new Date(),
    });
    const logs = store.getSyncLog(10);
    expect(logs).toHaveLength(1);
    expect(logs[0].direction).toBe("push");
    expect(logs[0].geneId).toBe("gene-1");
  });
});
