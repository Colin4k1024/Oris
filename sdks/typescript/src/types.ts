export interface OenEnvelope {
  sender_id: string;
  message_type: string;
  payload: unknown;
  signature: string;
  timestamp: string;
}

export interface HubConfig {
  baseUrl: string;
  apiKey: string;
  seed: Uint8Array;
  nodeId: string;
}

export interface ExecutionConfig {
  baseUrl: string;
  token: string;
}

export interface ExperienceConfig {
  baseUrl: string;
  apiKey: string;
  seed: Uint8Array;
  senderId: string;
}

export interface RegisterRequest {
  node_id: string;
  endpoint: string;
  public_key: string;
  capabilities: string[];
  version: string;
  region?: string;
}

export interface RegisterResponse {
  node_id: string;
  heartbeat_interval_seconds: number;
  ttl_seconds: number;
}

export interface HeartbeatResponse {
  acknowledged: boolean;
  next_heartbeat_seconds: number;
}

export interface DiscoveryResult {
  nodes: NodeInfo[];
  total: number;
}

export interface NodeInfo {
  node_id: string;
  endpoint: string;
  public_key: string;
  capabilities: string[];
  version: string;
  status: string;
  registered_at: string;
  last_heartbeat: string;
  ttl_seconds: number;
  region?: string;
}

export interface FederatedQuery {
  query: string;
  task_class?: string;
  min_confidence?: number;
  timeout_ms?: number;
  target_nodes?: string[];
  limit?: number;
}

export interface FederatedResult {
  results: GeneResult[];
  meta: FederationMeta;
}

export interface GeneResult {
  gene_id: string;
  name: string;
  task_class: string;
  confidence: number;
  source_node: string;
  created_at: string;
}

export interface FederationMeta {
  nodes_queried: number;
  nodes_responded: number;
  coverage: number;
  freshness: string;
  timeout_nodes: string[];
}

export interface ApiEnvelope<T> {
  meta: { status: string; api_version: string };
  request_id: string;
  data: T;
}

export interface RunJobRequest {
  thread_id: string;
  input?: Record<string, unknown>;
  idempotency_key?: string;
}

export interface RunJobResponse {
  thread_id: string;
  status: string;
  interrupts: unknown[];
  idempotent_replay: boolean;
}

export interface JobStateResponse {
  thread_id: string;
  values: Record<string, unknown>;
  checkpoint_id?: string;
}

export interface CancelJobResponse {
  thread_id: string;
  status: string;
  reason?: string;
}

export interface ListJobsResponse {
  jobs: JobListItem[];
}

export interface JobListItem {
  thread_id: string;
  status: string;
  updated_at: string;
}

export interface ShareResponse {
  gene_id: string;
  status: string;
  published_at?: string;
}

export interface FetchResponse {
  assets: NetworkAsset[];
  next_cursor: string | null;
  sync_audit: { total_available: number; returned: number };
}

export interface NetworkAsset {
  type: string;
  id: string;
  confidence?: number;
  quality_score?: number;
  use_count?: number;
  success_count?: number;
  contributor_id?: string;
  created_at?: string;
}

export type Fetcher = (url: string, init: RequestInit) => Promise<Response>;
