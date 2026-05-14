import { describe, it, expect } from "vitest";
import { ExperienceClient } from "../src/experience.js";
import type { Fetcher } from "../src/types.js";

const SEED = new Uint8Array(32);
const seedStr = "test-seed-32-bytes-for-testing!!";
for (let i = 0; i < seedStr.length; i++) SEED[i] = seedStr.charCodeAt(i);

function makeFetcher(handler: (url: string, init: RequestInit) => unknown): Fetcher {
  return async (url: string, init: RequestInit) => {
    const result = handler(url, init);
    return { ok: true, status: 200, json: async () => result } as Response;
  };
}

describe("ExperienceClient", () => {
  it("share sends X-Api-Key and envelope with signature", async () => {
    let capturedHeaders: Record<string, string> = {};
    let capturedBody: any;

    const fetcher = makeFetcher((_url, init) => {
      capturedHeaders = Object.fromEntries(
        Object.entries(init.headers as Record<string, string>)
      );
      capturedBody = JSON.parse(init.body as string);
      return { gene_id: "g1", status: "published" };
    });

    const ec = new ExperienceClient(
      { baseUrl: "http://test", apiKey: "ak-123", seed: SEED, senderId: "agent-1" },
      fetcher
    );
    const resp = await ec.share({ gene_id: "g1", confidence: 0.9 });

    expect(capturedHeaders["X-Api-Key"]).toBe("ak-123");
    expect(capturedBody.envelope.sender_id).toBe("agent-1");
    expect(capturedBody.envelope.message_type).toBe("publish");
    expect(capturedBody.envelope.signature).toBeTruthy();
    expect(resp.gene_id).toBe("g1");
  });

  it("fetch_ sends no auth headers", async () => {
    let capturedHeaders: Record<string, string> = {};

    const fetcher = makeFetcher((_url, init) => {
      capturedHeaders = Object.fromEntries(
        Object.entries(init.headers as Record<string, string>)
      );
      return { assets: [], next_cursor: null, sync_audit: { total_available: 0, returned: 0 } };
    });

    const ec = new ExperienceClient(
      { baseUrl: "http://test", apiKey: "ak-123", seed: SEED, senderId: "a1" },
      fetcher
    );
    const resp = await ec.fetch_({ q: "bugfix", limit: 5 });

    expect(capturedHeaders["Authorization"]).toBeUndefined();
    expect(capturedHeaders["X-Api-Key"]).toBeUndefined();
    expect(resp.assets).toEqual([]);
  });

  it("registerPublicKey sends hex key", async () => {
    let capturedBody: any;

    const fetcher = makeFetcher((_url, init) => {
      capturedBody = JSON.parse(init.body as string);
      return {};
    });

    const ec = new ExperienceClient(
      { baseUrl: "http://test", apiKey: "ak-123", seed: SEED, senderId: "agent-1" },
      fetcher
    );
    await ec.registerPublicKey();

    expect(capturedBody.sender_id).toBe("agent-1");
    expect(capturedBody.public_key_hex.length).toBe(64);
  });
});
