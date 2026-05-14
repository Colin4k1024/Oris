import type {
  ExecutionConfig,
  ApiEnvelope,
  RunJobResponse,
  JobStateResponse,
  CancelJobResponse,
  ListJobsResponse,
  Fetcher,
} from "./types.js";

export class ExecutionClient {
  private cfg: ExecutionConfig;
  private fetch: Fetcher;

  constructor(config: ExecutionConfig, fetcher?: Fetcher) {
    this.cfg = config;
    this.fetch = fetcher ?? globalThis.fetch.bind(globalThis);
  }

  async runJob(
    threadId: string,
    input?: Record<string, unknown>,
    idempotencyKey?: string
  ): Promise<RunJobResponse> {
    const body: Record<string, unknown> = { thread_id: threadId };
    if (input) body.input = input;
    if (idempotencyKey) body.idempotency_key = idempotencyKey;
    return this.post("/jobs", body);
  }

  async getState(threadId: string): Promise<JobStateResponse> {
    return this.get(`/jobs/${threadId}/state`);
  }

  async getDetail(threadId: string): Promise<unknown> {
    return this.get(`/jobs/${threadId}`);
  }

  async getHistory(threadId: string): Promise<unknown> {
    return this.get(`/jobs/${threadId}/history`);
  }

  async getTimeline(threadId: string): Promise<unknown> {
    return this.get(`/jobs/${threadId}/timeline`);
  }

  async cancel(threadId: string, reason?: string): Promise<CancelJobResponse> {
    const body: Record<string, unknown> = {};
    if (reason) body.reason = reason;
    return this.post(`/jobs/${threadId}/cancel`, body);
  }

  async resume(
    threadId: string,
    value: Record<string, unknown>,
    checkpointId?: string
  ): Promise<RunJobResponse> {
    const body: Record<string, unknown> = { value };
    if (checkpointId) body.checkpoint_id = checkpointId;
    return this.post(`/jobs/${threadId}/resume`, body);
  }

  async replay(threadId: string, checkpointId?: string): Promise<RunJobResponse> {
    const body: Record<string, unknown> = {};
    if (checkpointId) body.checkpoint_id = checkpointId;
    return this.post(`/jobs/${threadId}/replay`, body);
  }

  async listJobs(opts?: {
    status?: string;
    limit?: number;
    offset?: number;
  }): Promise<ListJobsResponse> {
    const params = new URLSearchParams();
    if (opts?.status) params.set("status", opts.status);
    if (opts?.limit) params.set("limit", String(opts.limit));
    if (opts?.offset) params.set("offset", String(opts.offset));
    const qs = params.toString();
    return this.get(`/jobs${qs ? `?${qs}` : ""}`);
  }

  private async post<T>(path: string, body: unknown): Promise<T> {
    const resp = await this.fetch(`${this.cfg.baseUrl}${path}`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${this.cfg.token}`,
      },
      body: JSON.stringify(body),
    });
    if (!resp.ok) throw new Error(`execution: ${resp.status}`);
    const envelope: ApiEnvelope<T> = await resp.json();
    return envelope.data;
  }

  private async get<T>(path: string): Promise<T> {
    const resp = await this.fetch(`${this.cfg.baseUrl}${path}`, {
      method: "GET",
      headers: { Authorization: `Bearer ${this.cfg.token}` },
    });
    if (!resp.ok) throw new Error(`execution: ${resp.status}`);
    const envelope: ApiEnvelope<T> = await resp.json();
    return envelope.data;
  }
}
