import type { ExperienceConfig, HubConfig } from "./types.js";
import type { GeneStore } from "./store-interface.js";
import type { MySQLConfig } from "./mysql-store.js";
import { ExperienceClient } from "./experience.js";
import { HubClient } from "./hub.js";
import { LocalStore } from "./store.js";
import { SyncManager } from "./sync.js";

export interface OrisConfig {
  storePath?: string;
  mysql?: MySQLConfig;
  hub?: HubConfig;
  experience?: ExperienceConfig;
}

export class OrisClient {
  public readonly store: GeneStore;
  public readonly sync: SyncManager;

  private constructor(store: GeneStore, config?: OrisConfig) {
    this.store = store;

    const experience = config?.experience
      ? new ExperienceClient(config.experience)
      : undefined;
    const hub = config?.hub ? new HubClient(config.hub) : undefined;

    this.sync = new SyncManager({ store: this.store, experience, hub });
  }

  static createSync(config?: OrisConfig): OrisClient {
    const storePath = config?.storePath || "oris_genes.db";
    const store = new LocalStore(storePath);
    return new OrisClient(store, config);
  }

  static async create(config?: OrisConfig): Promise<OrisClient> {
    if (config?.mysql) {
      const { MySQLStore } = await import("./mysql-store.js");
      const store = await MySQLStore.create(config.mysql);
      return new OrisClient(store, config);
    }
    return OrisClient.createSync(config);
  }

  close(): void {
    this.store.close();
  }
}
