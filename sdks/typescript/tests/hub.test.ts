import { describe, it, expect } from "vitest";
import { HubClient } from "../src/hub.js";
import type { HubConfig, Fetcher } from "../src/types.js";

const SEED = new Uint8Array(32);
const seedStr = "test-seed-32-bytes-for-testing!!";
for (let i = 0; i < seedStr.length; i++) SEED[i] = seedStr.charCodeAt(i);

function makeFetcher(handler: (url: string, init: RequestInit) => unknown): Fetcher {
  return async (url: string, init: RequestInit) => {
    const result = handler(url, init);
    return {
      ok: true,
      status: 200,
      json: async () => result,
    } as Response;
  };
}

const cfg: HubConfig = { baseUrl: "http://test", apiKey: "my-key", seed: SEED, nodeId: "n1" };

describe("HubClient", () => {
  it("register sends X-OEN-Signature", async () => {
    let capturedHeaders: Record<string, string> = {};
    let capturedBody: unknown;

    const fetcher = makeFetcher((_url, init) => {
      capturedHeaders = Object.fromEntries(
        Object.entries(init.headers as Record<string, string>)
      );
      capturedBody = JSON.parse(init.body as string);
      return { node_id: "n1", heartbeat_interval_seconds: 30, ttl_seconds: 90 };
    });

    const hub = new HubClient(cfg, fetcher);
    const resp = await hub.register("http://localhost:8080", ["evolve"], "0.1.0");

    expect(capturedHeaders["X-OEN-Signature"]).toBeTruthy();
    expect((capturedBody as any).node_id).toBe("n1");
    expect((capturedBody as any).public_key).toBeTruthy();
    expect(resp.node_id).toBe("n1");
  });

  it("heartbeat sends X-OEN-Signature", async () => {
    let capturedHeaders: Record<string, string> = {};
    let capturedUrl = "";

    const fetcher = makeFetcher((url, init) => {
      capturedUrl = url;
      capturedHeaders = Object.fromEntries(
        Object.entries(init.headers as Record<string, string>)
      );
      return { acknowledged: true, next_heartbeat_seconds: 30 };
    });

    const hub = new HubClient(cfg, fetcher);
    const resp = await hub.heartbeat("active");

    expect(capturedHeaders["X-OEN-Signature"]).toBeTruthy();
    expect(capturedUrl).toContain("n1/heartbeat");
    expect(resp.acknowledged).toBe(true);
  });

  it("discover sends Bearer token", async () => {
    let capturedHeaders: Record<string, string> = {};

    const fetcher = makeFetcher((_url, init) => {
      capturedHeaders = Object.fromEntries(
        Object.entries(init.headers as Record<string, string>)
      );
      return { nodes: [], total: 0 };
    });

    const hub = new HubClient(cfg, fetcher);
    const resp = await hub.discover({ capabilities: ["evolve"] });

    expect(capturedHeaders["Authorization"]).toBe("Bearer my-key");
    expect(resp.total).toBe(0);
  });

  it("search sends Bearer and body", async () => {
    let capturedHeaders: Record<string, string> = {};
    let capturedBody: unknown;

    const fetcher = makeFetcher((_url, init) => {
      capturedHeaders = Object.fromEntries(
        Object.entries(init.headers as Record<string, string>)
      );
      capturedBody = JSON.parse(init.body as string);
      return { results: [], meta: { nodes_queried: 0, nodes_responded: 0, coverage: 0, freshness: "", timeout_nodes: [] } };
    });

    const hub = new HubClient(cfg, fetcher);
    await hub.search({ query: "bugfix", task_class: "fix" });

    expect(capturedHeaders["Authorization"]).toBe("Bearer my-key");
    expect((capturedBody as any).query).toBe("bugfix");
  });
});
