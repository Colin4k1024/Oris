import { describe, it, expect, beforeAll, afterAll } from "vitest";

const MYSQL_HOST = process.env.MYSQL_HOST;
const MYSQL_USER = process.env.MYSQL_USER;
const MYSQL_PASSWORD = process.env.MYSQL_PASSWORD;
const MYSQL_DATABASE = process.env.MYSQL_DATABASE || "oris";

const skipMySQL = !MYSQL_HOST || !MYSQL_USER || !MYSQL_PASSWORD;

describe.skipIf(skipMySQL)("MySQLStore", () => {
  let store: Awaited<ReturnType<typeof import("../src/mysql-store.js")>["MySQLStore"]["create"]>;

  beforeAll(async () => {
    const { MySQLStore } = await import("../src/mysql-store.js");
    store = await MySQLStore.create({
      host: MYSQL_HOST!,
      user: MYSQL_USER!,
      password: MYSQL_PASSWORD!,
      database: MYSQL_DATABASE,
    });
  });

  afterAll(() => {
    store?.close();
  });

  it("should save and get a gene", async () => {
    const gene = {
      geneId: "test-ts-mysql-1",
      name: "Test Gene",
      taskClass: "compile-fix",
      confidence: 0.85,
      strategy: { approach: "retry" },
      signals: { error_type: "syntax" },
      validation: { tests_passed: true },
      qualityScore: 0.9,
      useCount: 5,
      successCount: 4,
      contributorId: "node-1",
      source: "local",
      createdAt: new Date(),
      updatedAt: new Date(),
    };

    await store.save(gene);
    const got = await store.get("test-ts-mysql-1");

    expect(got).not.toBeNull();
    expect(got!.name).toBe("Test Gene");
    expect(got!.confidence).toBe(0.85);

    await store.delete("test-ts-mysql-1");
  });

  it("should return null for non-existent gene", async () => {
    const got = await store.get("nonexistent-gene");
    expect(got).toBeNull();
  });

  it("should update stats", async () => {
    const gene = {
      geneId: "test-ts-mysql-stats",
      name: "Stats Gene",
      taskClass: "test",
      confidence: 0,
      qualityScore: 0,
      useCount: 0,
      successCount: 0,
      contributorId: "",
      source: "local",
      createdAt: new Date(),
      updatedAt: new Date(),
    };
    await store.save(gene);

    await store.updateStats("test-ts-mysql-stats", true, true);

    const got = await store.get("test-ts-mysql-stats");
    expect(got!.useCount).toBe(1);
    expect(got!.successCount).toBe(1);

    await store.delete("test-ts-mysql-stats");
  });

  it("should query by task class", async () => {
    for (let i = 0; i < 3; i++) {
      await store.save({
        geneId: `test-ts-mysql-q-${i}`,
        name: "Query Gene",
        taskClass: "query-target",
        confidence: 0,
        qualityScore: 0,
        useCount: 0,
        successCount: 0,
        contributorId: "",
        source: "local",
        createdAt: new Date(),
        updatedAt: new Date(),
      });
    }

    const results = await store.query({ taskClass: "query-target", limit: 10 });
    expect(results.length).toBeGreaterThanOrEqual(3);

    for (let i = 0; i < 3; i++) {
      await store.delete(`test-ts-mysql-q-${i}`);
    }
  });

  it("should log and retrieve sync log", async () => {
    await store.logSync({
      direction: "push",
      geneId: "test-ts-log-gene",
      status: "success",
      remoteUrl: "https://hub.example.com",
      errorMessage: "",
      timestamp: new Date(),
    });

    const logs = await store.getSyncLog(10);
    expect(logs.length).toBeGreaterThan(0);
    expect(logs.some((e) => e.geneId === "test-ts-log-gene")).toBe(true);
  });

  it("should mark synced", async () => {
    await store.save({
      geneId: "test-ts-mysql-synced",
      name: "Sync Gene",
      taskClass: "test",
      confidence: 0,
      qualityScore: 0,
      useCount: 0,
      successCount: 0,
      contributorId: "",
      source: "local",
      createdAt: new Date(),
      updatedAt: new Date(),
    });

    await store.markSynced("test-ts-mysql-synced", new Date());

    const got = await store.get("test-ts-mysql-synced");
    expect(got!.syncedAt).toBeDefined();

    await store.delete("test-ts-mysql-synced");
  });
});
