import type { ExperienceConfig, HubConfig } from "./types.js";
import { ExperienceClient } from "./experience.js";
import { HubClient } from "./hub.js";
import { LocalStore } from "./store.js";
import { SyncManager } from "./sync.js";

export interface OrisConfig {
  storePath?: string;
  hub?: HubConfig;
  experience?: ExperienceConfig;
}

export class OrisClient {
  public readonly store: LocalStore;
  public readonly sync: SyncManager;

  constructor(config?: OrisConfig) {
    const storePath = config?.storePath || "oris_genes.db";
    this.store = new LocalStore(storePath);

    const experience = config?.experience
      ? new ExperienceClient(config.experience)
      : undefined;
    const hub = config?.hub ? new HubClient(config.hub) : undefined;

    this.sync = new SyncManager({ store: this.store, experience, hub });
  }

  close(): void {
    this.store.close();
  }
}
