import { describe, it, expect } from "vitest";
import { ExecutionClient } from "../src/execution.js";
import type { Fetcher } from "../src/types.js";

function envelope(data: unknown) {
  return { meta: { status: "ok", api_version: "v1" }, request_id: "req-1", data };
}

function makeFetcher(handler: (url: string, init: RequestInit) => unknown): Fetcher {
  return async (url: string, init: RequestInit) => {
    const result = handler(url, init);
    return { ok: true, status: 200, json: async () => result } as Response;
  };
}

describe("ExecutionClient", () => {
  it("runJob sends Bearer", async () => {
    let capturedAuth = "";
    let capturedBody: any;

    const fetcher = makeFetcher((_url, init) => {
      capturedAuth = (init.headers as Record<string, string>)["Authorization"];
      capturedBody = JSON.parse(init.body as string);
      return envelope({ thread_id: "t1", status: "running", interrupts: [], idempotent_replay: false });
    });

    const ec = new ExecutionClient({ baseUrl: "http://test", token: "tok-123" }, fetcher);
    const resp = await ec.runJob("t1", { task: "hello" });

    expect(capturedAuth).toBe("Bearer tok-123");
    expect(capturedBody.thread_id).toBe("t1");
    expect(resp.thread_id).toBe("t1");
    expect(resp.status).toBe("running");
  });

  it("getState unwraps envelope", async () => {
    let capturedUrl = "";

    const fetcher = makeFetcher((url) => {
      capturedUrl = url;
      return envelope({ thread_id: "t1", values: { result: "done" } });
    });

    const ec = new ExecutionClient({ baseUrl: "http://test", token: "tok" }, fetcher);
    const resp = await ec.getState("t1");

    expect(capturedUrl).toContain("/jobs/t1/state");
    expect(resp.values.result).toBe("done");
  });

  it("cancel sends reason", async () => {
    let capturedBody: any;

    const fetcher = makeFetcher((_url, init) => {
      capturedBody = JSON.parse(init.body as string);
      return envelope({ thread_id: "t1", status: "cancelled", reason: "user abort" });
    });

    const ec = new ExecutionClient({ baseUrl: "http://test", token: "tok" }, fetcher);
    const resp = await ec.cancel("t1", "user abort");

    expect(capturedBody.reason).toBe("user abort");
    expect(resp.status).toBe("cancelled");
  });

  it("listJobs passes query params", async () => {
    let capturedUrl = "";

    const fetcher = makeFetcher((url) => {
      capturedUrl = url;
      return envelope({ jobs: [] });
    });

    const ec = new ExecutionClient({ baseUrl: "http://test", token: "tok" }, fetcher);
    await ec.listJobs({ status: "running", limit: 10 });

    expect(capturedUrl).toContain("status=running");
    expect(capturedUrl).toContain("limit=10");
  });
});
