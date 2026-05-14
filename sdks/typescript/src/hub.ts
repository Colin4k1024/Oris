import { signBody, publicKeyBase64 } from "./signing.js";
import type {
  HubConfig,
  RegisterResponse,
  HeartbeatResponse,
  DiscoveryResult,
  FederatedQuery,
  FederatedResult,
  Fetcher,
} from "./types.js";

export class HubClient {
  private cfg: HubConfig;
  private fetch: Fetcher;

  constructor(config: HubConfig, fetcher?: Fetcher) {
    this.cfg = config;
    this.fetch = fetcher ?? globalThis.fetch.bind(globalThis);
  }

  async register(
    endpoint: string,
    capabilities: string[],
    version: string,
    region?: string
  ): Promise<RegisterResponse> {
    const bodyObj: Record<string, unknown> = {
      node_id: this.cfg.nodeId,
      endpoint,
      public_key: await publicKeyBase64(this.cfg.seed),
      capabilities,
      version,
    };
    if (region) bodyObj.region = region;

    const body = JSON.stringify(bodyObj);
    const sig = await signBody(this.cfg.seed, new TextEncoder().encode(body));

    const resp = await this.fetch(`${this.cfg.baseUrl}/hub/nodes`, {
      method: "POST",
      headers: { "Content-Type": "application/json", "X-OEN-Signature": sig },
      body,
    });
    if (!resp.ok) throw new Error(`hub register: ${resp.status}`);
    return resp.json();
  }

  async heartbeat(status: string = "active"): Promise<HeartbeatResponse> {
    const bodyObj = { node_id: this.cfg.nodeId, status };
    const body = JSON.stringify(bodyObj);
    const sig = await signBody(this.cfg.seed, new TextEncoder().encode(body));

    const resp = await this.fetch(
      `${this.cfg.baseUrl}/hub/nodes/${this.cfg.nodeId}/heartbeat`,
      {
        method: "PUT",
        headers: { "Content-Type": "application/json", "X-OEN-Signature": sig },
        body,
      }
    );
    if (!resp.ok) throw new Error(`hub heartbeat: ${resp.status}`);
    return resp.json();
  }

  async discover(opts?: {
    capabilities?: string[];
    region?: string;
    limit?: number;
  }): Promise<DiscoveryResult> {
    const resp = await this.fetch(`${this.cfg.baseUrl}/hub/nodes`, {
      method: "GET",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${this.cfg.apiKey}`,
      },
    });
    if (!resp.ok) throw new Error(`hub discover: ${resp.status}`);
    return resp.json();
  }

  async search(query: FederatedQuery): Promise<FederatedResult> {
    const resp = await this.fetch(`${this.cfg.baseUrl}/hub/search`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${this.cfg.apiKey}`,
      },
      body: JSON.stringify(query),
    });
    if (!resp.ok) throw new Error(`hub search: ${resp.status}`);
    return resp.json();
  }

  async subscribe(
    callbackUrl: string,
    filter?: { task_class?: string; min_confidence?: number }
  ): Promise<unknown> {
    const resp = await this.fetch(`${this.cfg.baseUrl}/hub/subscriptions`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${this.cfg.apiKey}`,
      },
      body: JSON.stringify({
        subscriber_node_id: this.cfg.nodeId,
        callback_url: callbackUrl,
        filter: filter ?? {},
      }),
    });
    if (!resp.ok) throw new Error(`hub subscribe: ${resp.status}`);
    return resp.json();
  }

  async unsubscribe(subscriptionId: string): Promise<void> {
    const resp = await this.fetch(
      `${this.cfg.baseUrl}/hub/subscriptions/${subscriptionId}`,
      {
        method: "DELETE",
        headers: { Authorization: `Bearer ${this.cfg.apiKey}` },
      }
    );
    if (!resp.ok) throw new Error(`hub unsubscribe: ${resp.status}`);
  }

  async listSubscriptions(): Promise<unknown[]> {
    const resp = await this.fetch(`${this.cfg.baseUrl}/hub/subscriptions`, {
      method: "GET",
      headers: { Authorization: `Bearer ${this.cfg.apiKey}` },
    });
    if (!resp.ok) throw new Error(`hub listSubscriptions: ${resp.status}`);
    return resp.json();
  }
}
