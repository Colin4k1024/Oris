import { signPayload, publicKeyHex } from "./signing.js";
import type { ExperienceConfig, ShareResponse, FetchResponse, Fetcher } from "./types.js";

export class ExperienceClient {
  private cfg: ExperienceConfig;
  private fetch: Fetcher;

  constructor(config: ExperienceConfig, fetcher?: Fetcher) {
    this.cfg = config;
    this.fetch = fetcher ?? globalThis.fetch.bind(globalThis);
  }

  async share(payload: unknown): Promise<ShareResponse> {
    const sig = await signPayload(this.cfg.seed, payload);
    const envelope = {
      sender_id: this.cfg.senderId,
      message_type: "publish",
      payload,
      signature: sig,
      timestamp: new Date().toISOString().replace(/\.\d{3}Z$/, "Z"),
    };

    const resp = await this.fetch(`${this.cfg.baseUrl}/experience`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "X-Api-Key": this.cfg.apiKey,
      },
      body: JSON.stringify({ envelope }),
    });
    if (!resp.ok) throw new Error(`experience share: ${resp.status}`);
    return resp.json();
  }

  async fetch_(opts?: {
    q?: string;
    minConfidence?: number;
    limit?: number;
    cursor?: string;
  }): Promise<FetchResponse> {
    const params = new URLSearchParams();
    if (opts?.q) params.set("q", opts.q);
    if (opts?.minConfidence) params.set("min_confidence", String(opts.minConfidence));
    if (opts?.limit) params.set("limit", String(opts.limit));
    if (opts?.cursor) params.set("cursor", opts.cursor);
    const qs = params.toString();

    const resp = await this.fetch(`${this.cfg.baseUrl}/experience${qs ? `?${qs}` : ""}`, {
      method: "GET",
      headers: {},
    });
    if (!resp.ok) throw new Error(`experience fetch: ${resp.status}`);
    return resp.json();
  }

  async registerPublicKey(): Promise<void> {
    const hex = await publicKeyHex(this.cfg.seed);
    const resp = await this.fetch(`${this.cfg.baseUrl}/public-keys`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "X-Api-Key": this.cfg.apiKey,
      },
      body: JSON.stringify({
        sender_id: this.cfg.senderId,
        public_key_hex: hex,
      }),
    });
    if (!resp.ok) throw new Error(`experience registerPublicKey: ${resp.status}`);
  }
}
