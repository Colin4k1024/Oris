# Oris SDK — Local-First Integration Architecture

> Version: 0.3.0 | Status: Stable | Updated: 2026-05-15

## Overview

Oris SDK 采用 **Local-First** 架构设计：集成方拥有本地经验存储（LocalStore 或 MySQLStore），所有经验/基因数据默认落盘到本地或共享数据库。Hub 和 Experience Repo 作为可选的网络层，集成方可以选择性地将本地经验分享到网络，或从网络获取经验到本地。

**v0.3.0 新增 MySQL 后端**：除默认 SQLite 外，现在可以使用 MySQL 作为共享存储后端，适用于多节点部署和团队协作场景。两种后端共享同一套 Store 接口，切换只需更改初始化配置。

```
┌─────────────────────────────────────────────────────────────┐
│  Integrator Application                                     │
│                                                             │
│  ┌───────────────────────────────────────────────────────┐  │
│  │  Oris SDK                                             │  │
│  │                                                       │  │
│  │  ┌─────────────┐   ┌──────────────┐                  │  │
│  │  │   Store     │◄──│ SyncManager  │──► Hub / Repo    │  │
│  │  │(SQLite/MySQL)│   │ (optional)   │                  │  │
│  │  └─────────────┘   └──────────────┘                  │  │
│  │        ▲                                              │  │
│  │        │                                              │  │
│  │  ┌─────────────┐  ┌──────────────┐  ┌────────────┐  │  │
│  │  │ Experience  │  │  Execution   │  │    Hub     │  │  │
│  │  │   Client    │  │   Client     │  │   Client   │  │  │
│  │  └─────────────┘  └──────────────┘  └────────────┘  │  │
│  │                                                       │  │
│  │  Internal: Signing · CanonicalJSON · Errors           │  │
│  └───────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
         │ (optional)              │ (optional)
         ▼                         ▼
┌────────────────┐      ┌────────────────────────┐
│  Oris Hub      │      │  Oris Experience Repo  │
│  :8100         │      │  :3000                 │
└────────────────┘      └────────────────────────┘
```

---

## 核心设计原则

| 原则 | 说明 |
|------|------|
| **本地优先** | 所有经验数据首先写入本地存储（SQLite 或 MySQL），无需网络即可使用 |
| **存储可选** | 默认 SQLite（零配置），可选 MySQL（多节点共享） |
| **Hub 可选** | Hub 连接是"增强"不是"前提"；离线模式下 SDK 完整可用 |
| **显式同步** | 数据从不自动上传；share/pull 必须由集成方代码显式触发 |
| **单一数据源** | 本地存储是集成方的 ground truth；Hub 是网络层缓存 |
| **冲突回避** | 基因是不可变的（immutable by ID）；confidence/use_count 按最新值合并 |

---

## 数据模型

### Gene（经验基因）

```
┌──────────────────────────────────────────────────────────────┐
│ Gene                                                         │
├──────────────────────────────────────────────────────────────┤
│ gene_id        TEXT PRIMARY KEY   -- 全局唯一 ID              │
│ name           TEXT NOT NULL      -- 人类可读名称              │
│ task_class     TEXT NOT NULL      -- 所属任务类别              │
│ confidence     REAL NOT NULL      -- 信心分 [0.0, 1.0]       │
│ strategy       TEXT (JSON)        -- 策略详情                 │
│ signals        TEXT (JSON)        -- 触发信号                 │
│ validation     TEXT (JSON)        -- 验证结果                 │
│ quality_score  REAL               -- 质量分                   │
│ use_count      INTEGER DEFAULT 0  -- 本地使用次数              │
│ success_count  INTEGER DEFAULT 0  -- 本地成功次数              │
│ contributor_id TEXT               -- 贡献者 ID                │
│ source         TEXT NOT NULL      -- 来源: 'local' | 'hub'   │
│ synced_at      TEXT               -- 最近同步时间（ISO 8601）  │
│ created_at     TEXT NOT NULL      -- 创建时间                 │
│ updated_at     TEXT NOT NULL      -- 更新时间                 │
└──────────────────────────────────────────────────────────────┘
```

### SyncLog（同步日志）

```
┌──────────────────────────────────────────────────────────────┐
│ SyncLog                                                      │
├──────────────────────────────────────────────────────────────┤
│ id             INTEGER PRIMARY KEY AUTOINCREMENT              │
│ direction      TEXT NOT NULL       -- 'push' | 'pull'        │
│ gene_id        TEXT NOT NULL       -- 关联基因                │
│ status         TEXT NOT NULL       -- 'ok' | 'conflict' | 'error' │
│ remote_url     TEXT                -- 目标 Hub/Repo URL       │
│ error_message  TEXT                -- 失败原因                │
│ timestamp      TEXT NOT NULL       -- 发生时间                │
└──────────────────────────────────────────────────────────────┘
```

---

## SDK 配置模型

### 模式一：纯本地（Local Only）

无需网络配置，SDK 仅操作本地 SQLite。

```python
# Python
from oris_sdk import OrisClient

client = OrisClient(
    store_path="./oris_data.db"   # 本地 SQLite 路径
)
```

```go
// Go
client := oris.NewClient(oris.Config{
    StorePath: "./oris_data.db",
})
```

```typescript
// TypeScript
const client = new OrisClient({
  storePath: "./oris_data.db",
});
```

### 模式二：本地 + Hub 同步（Local + Hub Sync）

本地存储 + 可选的网络同步能力。

```python
# Python
from oris_sdk import OrisClient, HubSync

client = OrisClient(
    store_path="./oris_data.db",
    hub=HubSync(
        base_url="https://hub.oris.network:8100",
        api_key="oris_key_xxxxx",
        seed=bytes.fromhex("9d61b19d..."),  # Ed25519 seed
        sender_id="node-my-app-001",
    ),
    experience=ExperienceSync(
        base_url="https://repo.oris.network:3000",
        api_key="oris_key_xxxxx",
        seed=bytes.fromhex("9d61b19d..."),
        sender_id="node-my-app-001",
    ),
)
```

```go
// Go
client := oris.NewClient(oris.Config{
    StorePath: "./oris_data.db",
    Hub: &oris.HubSync{
        BaseURL:  "https://hub.oris.network:8100",
        APIKey:   "oris_key_xxxxx",
        Seed:     seed,
        SenderID: "node-my-app-001",
    },
    Experience: &oris.ExperienceSync{
        BaseURL:  "https://repo.oris.network:3000",
        APIKey:   "oris_key_xxxxx",
        Seed:     seed,
        SenderID: "node-my-app-001",
    },
})
```

```typescript
// TypeScript
const client = new OrisClient({
  storePath: "./oris_data.db",
  hub: {
    baseUrl: "https://hub.oris.network:8100",
    apiKey: "oris_key_xxxxx",
    seed: new Uint8Array([...]),
    senderId: "node-my-app-001",
  },
  experience: {
    baseUrl: "https://repo.oris.network:3000",
    apiKey: "oris_key_xxxxx",
    seed: new Uint8Array([...]),
    senderId: "node-my-app-001",
  },
});
```

---

## 核心 API 接口

### LocalStore（本地存储）

| 方法 | 说明 |
|------|------|
| `store.save(gene)` | 保存基因到本地 |
| `store.get(gene_id)` | 按 ID 获取基因 |
| `store.query(filter)` | 按条件查询本地基因 |
| `store.delete(gene_id)` | 删除本地基因 |
| `store.updateStats(gene_id, used, success)` | 更新使用统计 |
| `store.list(opts)` | 列出本地基因（分页） |

### SyncManager（同步管理）

| 方法 | 说明 |
|------|------|
| `sync.pushToHub(gene_ids?)` | 将本地基因分享到 Experience Repo |
| `sync.pullFromHub(query?)` | 从 Experience Repo 拉取基因到本地 |
| `sync.registerNode()` | 向 Hub 注册当前节点 |
| `sync.getSyncLog()` | 查看同步历史 |

---

## 操作流程

### 1. 本地创建经验

集成方在本地产生新的经验基因，直接写入 LocalStore。

```python
gene = Gene(
    gene_id="gene-" + uuid4().hex[:12],
    name="fix-null-pointer",
    task_class="bugfix",
    confidence=0.85,
    strategy={"approach": "pattern-match", "language": "rust"},
    signals={"error_type": "NullPointerException"},
    source="local",
)

client.store.save(gene)
```

```go
gene := oris.Gene{
    GeneID:     "gene-" + randomHex(12),
    Name:       "fix-null-pointer",
    TaskClass:  "bugfix",
    Confidence: 0.85,
    Strategy:   map[string]any{"approach": "pattern-match", "language": "rust"},
    Signals:    map[string]any{"error_type": "NullPointerException"},
    Source:     "local",
}

client.Store.Save(ctx, gene)
```

```typescript
const gene: Gene = {
  geneId: `gene-${crypto.randomUUID().slice(0, 12)}`,
  name: "fix-null-pointer",
  taskClass: "bugfix",
  confidence: 0.85,
  strategy: { approach: "pattern-match", language: "rust" },
  signals: { errorType: "NullPointerException" },
  source: "local",
};

await client.store.save(gene);
```

### 2. 查询本地经验

```python
# 按 task_class 查询
results = client.store.query(
    task_class="bugfix",
    min_confidence=0.7,
    limit=10,
)

# 按关键词搜索
results = client.store.query(q="null pointer", limit=5)
```

```go
results, err := client.Store.Query(ctx, oris.StoreQuery{
    TaskClass:     "bugfix",
    MinConfidence: 0.7,
    Limit:         10,
})
```

```typescript
const results = await client.store.query({
  taskClass: "bugfix",
  minConfidence: 0.7,
  limit: 10,
});
```

### 3. 分享经验到 Hub（可选）

当集成方决定将本地经验贡献给网络时，显式调用 push。

```python
# 分享指定基因
result = client.sync.push_to_hub(gene_ids=["gene-abc123"])

# 分享所有未同步的本地基因
result = client.sync.push_to_hub()  # 默认推送 source='local' 且 synced_at IS NULL
```

```go
// 分享指定基因
result, err := client.Sync.PushToHub(ctx, oris.PushOpts{
    GeneIDs: []string{"gene-abc123"},
})

// 分享所有未同步的本地基因
result, err := client.Sync.PushToHub(ctx, oris.PushOpts{})
```

```typescript
// 分享指定基因
const result = await client.sync.pushToHub({ geneIds: ["gene-abc123"] });

// 分享所有未同步的本地基因
const result = await client.sync.pushToHub();
```

**Push 内部流程：**

```
1. 从 LocalStore 读取待推送基因
2. 对每个基因:
   a. 生成 canonical JSON（keys 字母排序）
   b. Ed25519 签名 → hex 编码
   c. 构造 OEN Envelope
   d. POST /experience（带 X-Api-Key header）
3. 成功后更新 LocalStore 的 synced_at 字段
4. 写入 SyncLog
```

### 4. 从 Hub 拉取经验到本地（可选）

当集成方想获取网络上其他节点的经验时，显式调用 pull。

```python
# 按条件从 Hub 拉取
new_genes = client.sync.pull_from_hub(
    q="memory leak",
    min_confidence=0.8,
    limit=20,
)
# new_genes 已写入本地 LocalStore，source='hub'

print(f"Pulled {len(new_genes)} genes from hub")
```

```go
newGenes, err := client.Sync.PullFromHub(ctx, oris.PullOpts{
    Q:             "memory leak",
    MinConfidence: 0.8,
    Limit:         20,
})
// newGenes 已写入本地 LocalStore，Source="hub"
```

```typescript
const newGenes = await client.sync.pullFromHub({
  q: "memory leak",
  minConfidence: 0.8,
  limit: 20,
});
// newGenes 已写入本地 LocalStore，source='hub'
```

**Pull 内部流程：**

```
1. GET /experience?q=...&min_confidence=...&limit=...
2. 对返回的每个 NetworkAsset:
   a. 检查本地是否已有相同 gene_id
   b. 若无 → 插入 LocalStore（source='hub', synced_at=now）
   c. 若有 → 合并: 取较高的 confidence，累加 use_count
3. 写入 SyncLog
4. 返回新增/更新的基因列表
```

### 5. 使用经验（本地匹配）

集成方匹配问题到已有经验，全部在本地完成。

```python
# 根据运行时信号查找匹配的经验
matches = client.store.query(
    task_class="bugfix",
    signals={"error_type": "NullPointerException"},
    min_confidence=0.7,
    order_by="confidence DESC",
    limit=3,
)

if matches:
    best = matches[0]
    # 应用经验策略...
    client.store.update_stats(best.gene_id, used=True, success=True)
```

```go
matches, err := client.Store.Query(ctx, oris.StoreQuery{
    TaskClass:     "bugfix",
    Signals:       map[string]any{"error_type": "NullPointerException"},
    MinConfidence: 0.7,
    OrderBy:       "confidence DESC",
    Limit:         3,
})

if len(matches) > 0 {
    best := matches[0]
    // 应用经验策略...
    client.Store.UpdateStats(ctx, best.GeneID, true, true)
}
```

```typescript
const matches = await client.store.query({
  taskClass: "bugfix",
  signals: { errorType: "NullPointerException" },
  minConfidence: 0.7,
  orderBy: "confidence DESC",
  limit: 3,
});

if (matches.length > 0) {
  const best = matches[0];
  // 应用经验策略...
  await client.store.updateStats(best.geneId, { used: true, success: true });
}
```

---

## 接口定义

### Go

```go
package oris

type Config struct {
    StorePath  string          // 必填: SQLite 文件路径
    Hub        *HubSync        // 可选: Hub 同步配置
    Experience *ExperienceSync // 可选: Experience Repo 同步配置
}

type HubSync struct {
    BaseURL  string
    APIKey   string
    Seed     [32]byte
    SenderID string
    NodeID   string
    Endpoint string
}

type ExperienceSync struct {
    BaseURL  string
    APIKey   string
    Seed     [32]byte
    SenderID string
}

type Client struct {
    Store *LocalStore
    Sync  *SyncManager
}

// LocalStore — 本地 SQLite 存储
type LocalStore interface {
    Save(ctx context.Context, gene Gene) error
    Get(ctx context.Context, geneID string) (*Gene, error)
    Query(ctx context.Context, q StoreQuery) ([]Gene, error)
    Delete(ctx context.Context, geneID string) error
    UpdateStats(ctx context.Context, geneID string, used bool, success bool) error
    List(ctx context.Context, opts ListOpts) ([]Gene, error)
}

// SyncManager — 可选网络同步
type SyncManager interface {
    PushToHub(ctx context.Context, opts PushOpts) (*PushResult, error)
    PullFromHub(ctx context.Context, opts PullOpts) ([]Gene, error)
    RegisterNode(ctx context.Context) error
    GetSyncLog(ctx context.Context, limit int) ([]SyncLogEntry, error)
}

type Gene struct {
    GeneID       string         `json:"gene_id"`
    Name         string         `json:"name"`
    TaskClass    string         `json:"task_class"`
    Confidence   float64        `json:"confidence"`
    Strategy     map[string]any `json:"strategy,omitempty"`
    Signals      map[string]any `json:"signals,omitempty"`
    Validation   map[string]any `json:"validation,omitempty"`
    QualityScore float64        `json:"quality_score,omitempty"`
    UseCount     int            `json:"use_count"`
    SuccessCount int            `json:"success_count"`
    ContributorID string        `json:"contributor_id,omitempty"`
    Source       string         `json:"source"` // "local" | "hub"
    SyncedAt     *time.Time     `json:"synced_at,omitempty"`
    CreatedAt    time.Time      `json:"created_at"`
    UpdatedAt    time.Time      `json:"updated_at"`
}

type StoreQuery struct {
    Q             string
    TaskClass     string
    MinConfidence float64
    Signals       map[string]any
    Source        string // "" = all, "local", "hub"
    OrderBy       string
    Limit         int
    Offset        int
}

type PushOpts struct {
    GeneIDs []string // 为空则推送所有未同步的本地基因
}

type PullOpts struct {
    Q             string
    MinConfidence float64
    Limit         int
    TaskClass     string
}

type PushResult struct {
    Pushed   int
    Failed   int
    Errors   []PushError
}

type PullResult struct {
    Inserted int
    Updated  int
    Skipped  int
}
```

### Python

```python
from dataclasses import dataclass, field
from typing import Optional, Any

@dataclass
class Config:
    store_path: str                           # 必填: SQLite 路径
    hub: Optional["HubSync"] = None           # 可选
    experience: Optional["ExperienceSync"] = None  # 可选

@dataclass
class HubSync:
    base_url: str
    api_key: str
    seed: bytes          # 32-byte Ed25519 seed
    sender_id: str
    node_id: str = ""
    endpoint: str = ""

@dataclass
class ExperienceSync:
    base_url: str
    api_key: str
    seed: bytes
    sender_id: str

@dataclass
class Gene:
    gene_id: str
    name: str
    task_class: str
    confidence: float
    strategy: dict[str, Any] = field(default_factory=dict)
    signals: dict[str, Any] = field(default_factory=dict)
    validation: dict[str, Any] = field(default_factory=dict)
    quality_score: float = 0.0
    use_count: int = 0
    success_count: int = 0
    contributor_id: str = ""
    source: str = "local"         # "local" | "hub"
    synced_at: Optional[str] = None
    created_at: str = ""
    updated_at: str = ""

class LocalStore:
    def save(self, gene: Gene) -> None: ...
    def get(self, gene_id: str) -> Optional[Gene]: ...
    def query(self, **kwargs) -> list[Gene]: ...
    def delete(self, gene_id: str) -> None: ...
    def update_stats(self, gene_id: str, used: bool, success: bool) -> None: ...
    def list(self, limit: int = 50, offset: int = 0) -> list[Gene]: ...

class SyncManager:
    def push_to_hub(self, gene_ids: list[str] | None = None) -> PushResult: ...
    def pull_from_hub(self, **kwargs) -> list[Gene]: ...
    def register_node(self) -> None: ...
    def get_sync_log(self, limit: int = 50) -> list[SyncLogEntry]: ...
```

### TypeScript

```typescript
interface Config {
  storePath: string;                    // 必填: SQLite 路径
  hub?: HubSyncConfig;                  // 可选
  experience?: ExperienceSyncConfig;    // 可选
}

interface HubSyncConfig {
  baseUrl: string;
  apiKey: string;
  seed: Uint8Array;
  senderId: string;
  nodeId?: string;
  endpoint?: string;
}

interface ExperienceSyncConfig {
  baseUrl: string;
  apiKey: string;
  seed: Uint8Array;
  senderId: string;
}

interface Gene {
  geneId: string;
  name: string;
  taskClass: string;
  confidence: number;
  strategy?: Record<string, unknown>;
  signals?: Record<string, unknown>;
  validation?: Record<string, unknown>;
  qualityScore?: number;
  useCount: number;
  successCount: number;
  contributorId?: string;
  source: "local" | "hub";
  syncedAt?: string;
  createdAt: string;
  updatedAt: string;
}

interface LocalStore {
  save(gene: Gene): Promise<void>;
  get(geneId: string): Promise<Gene | null>;
  query(opts: StoreQuery): Promise<Gene[]>;
  delete(geneId: string): Promise<void>;
  updateStats(geneId: string, opts: { used: boolean; success: boolean }): Promise<void>;
  list(opts?: { limit?: number; offset?: number }): Promise<Gene[]>;
}

interface SyncManager {
  pushToHub(opts?: { geneIds?: string[] }): Promise<PushResult>;
  pullFromHub(opts?: PullOpts): Promise<Gene[]>;
  registerNode(): Promise<void>;
  getSyncLog(limit?: number): Promise<SyncLogEntry[]>;
}
```

---

## 网络同步细节

### Push 协议（本地 → Experience Repo）

```
POST {experience_base_url}/experience
Headers:
  Content-Type: application/json
  X-Api-Key: {api_key}

Body:
{
  "envelope": {
    "sender_id": "node-my-app-001",
    "message_type": "publish",
    "payload": {
      "gene_id": "gene-abc123",
      "name": "fix-null-pointer",
      "task_class": "bugfix",
      "confidence": 0.85,
      "strategy": {"approach": "pattern-match", "language": "rust"},
      "public_key_hex": "d75a980182b10ab7..."
    },
    "signature": "<hex(Ed25519(seed, canonical_json(payload)))>",
    "timestamp": "2026-05-15T10:30:00Z"
  }
}

Response 200:
{
  "gene_id": "gene-abc123",
  "status": "published",
  "published_at": "2026-05-15T10:30:01Z"
}
```

### Pull 协议（Experience Repo → 本地）

```
GET {experience_base_url}/experience?q=memory+leak&min_confidence=0.8&limit=20

Response 200:
{
  "assets": [
    {
      "type": "gene",
      "id": "gene-xyz789",
      "confidence": 0.92,
      "quality_score": 0.88,
      "use_count": 47,
      "success_count": 41,
      "contributor_id": "node-other-001",
      "created_at": "2026-05-10T08:00:00Z"
    }
  ],
  "next_cursor": "cursor_abc",
  "sync_audit": { "total_available": 150, "returned": 20 }
}
```

### Hub 注册（可选）

集成方如果想加入节点发现网络：

```
POST {hub_base_url}/nodes/register
Headers:
  Content-Type: application/json
  X-OEN-Signature: <base64(Ed25519(seed, body))>

Body:
{
  "node_id": "node-my-app-001",
  "endpoint": "https://my-app.example.com",
  "public_key": "<base64(public_key)>",
  "capabilities": ["evolution", "intake"],
  "version": "0.61.0",
  "region": "us-west"
}
```

---

## 认证矩阵

| 操作 | 目标服务 | 认证方式 | 签名编码 | 公钥编码 |
|------|----------|----------|----------|----------|
| Push 经验 | Experience Repo | `X-Api-Key` + envelope 内签名 | hex | hex |
| Pull 经验 | Experience Repo | 无（公开读） | — | — |
| 注册节点 | Hub | `X-OEN-Signature` header | base64 | base64 |
| 心跳 | Hub | `X-OEN-Signature` header | base64 | base64 |
| 发现节点 | Hub | `Authorization: Bearer` | — | — |
| 提交任务 | Execution Runtime | `Authorization: Bearer` | — | — |

---

## 冲突解决策略

基因是**不可变的**（同一 gene_id 的核心字段不变），但统计字段可以合并：

| 字段 | 冲突处理 |
|------|----------|
| `gene_id`, `name`, `task_class`, `strategy`, `signals` | 不可变，以先写入的为准 |
| `confidence` | 取远端和本地的最大值 |
| `quality_score` | 取远端和本地的最大值 |
| `use_count` | 取远端和本地的最大值 |
| `success_count` | 取远端和本地的最大值 |

Pull 时如果本地已存在相同 gene_id：
1. 核心字段相同 → 合并统计字段
2. 核心字段不同 → 跳过并记录 conflict 到 SyncLog

---

## SQLite Schema

```sql
CREATE TABLE IF NOT EXISTS genes (
    gene_id        TEXT PRIMARY KEY,
    name           TEXT NOT NULL,
    task_class     TEXT NOT NULL,
    confidence     REAL NOT NULL DEFAULT 0.0,
    strategy       TEXT,  -- JSON
    signals        TEXT,  -- JSON
    validation     TEXT,  -- JSON
    quality_score  REAL DEFAULT 0.0,
    use_count      INTEGER DEFAULT 0,
    success_count  INTEGER DEFAULT 0,
    contributor_id TEXT DEFAULT '',
    source         TEXT NOT NULL DEFAULT 'local',  -- 'local' | 'hub'
    synced_at      TEXT,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_genes_task_class ON genes(task_class);
CREATE INDEX IF NOT EXISTS idx_genes_confidence ON genes(confidence DESC);
CREATE INDEX IF NOT EXISTS idx_genes_source ON genes(source);
CREATE INDEX IF NOT EXISTS idx_genes_synced_at ON genes(synced_at);

CREATE TABLE IF NOT EXISTS sync_log (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    direction      TEXT NOT NULL,   -- 'push' | 'pull'
    gene_id        TEXT NOT NULL,
    status         TEXT NOT NULL,   -- 'ok' | 'conflict' | 'error'
    remote_url     TEXT,
    error_message  TEXT,
    timestamp      TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_sync_log_timestamp ON sync_log(timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_sync_log_gene ON sync_log(gene_id);
```

---

## 完整集成示例

### Python: 自动修复引擎 + 经验积累

```python
from oris_sdk import OrisClient, Gene, HubSync, ExperienceSync
import uuid
from datetime import datetime, timezone

# 初始化（本地 + 可选 Hub）
client = OrisClient(
    store_path="./my_agent_data.db",
    experience=ExperienceSync(
        base_url="https://repo.oris.network:3000",
        api_key="oris_key_xxxxx",
        seed=bytes.fromhex("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60"),
        sender_id="my-agent-001",
    ),
)

# 1. 遇到问题时，先查本地经验
def handle_error(error_signal: dict) -> bool:
    matches = client.store.query(
        task_class=error_signal.get("task_class", "bugfix"),
        signals=error_signal,
        min_confidence=0.7,
        order_by="confidence DESC",
        limit=3,
    )

    if matches:
        gene = matches[0]
        success = apply_strategy(gene.strategy)
        client.store.update_stats(gene.gene_id, used=True, success=success)
        return success

    # 本地没有 → 尝试从 Hub 拉取
    if client.sync:
        pulled = client.sync.pull_from_hub(
            q=error_signal.get("error_type", ""),
            min_confidence=0.7,
            limit=5,
        )
        if pulled:
            gene = pulled[0]
            success = apply_strategy(gene.strategy)
            client.store.update_stats(gene.gene_id, used=True, success=success)
            return success

    return False

# 2. 成功解决问题后，创建新经验
def record_experience(error_signal: dict, strategy: dict, confidence: float):
    gene = Gene(
        gene_id=f"gene-{uuid.uuid4().hex[:12]}",
        name=f"fix-{error_signal.get('error_type', 'unknown')}",
        task_class="bugfix",
        confidence=confidence,
        strategy=strategy,
        signals=error_signal,
        source="local",
        created_at=datetime.now(timezone.utc).isoformat(),
        updated_at=datetime.now(timezone.utc).isoformat(),
    )
    client.store.save(gene)

# 3. 定期将高质量经验分享到网络（可选）
def share_top_experiences():
    top_genes = client.store.query(
        source="local",
        min_confidence=0.9,
        order_by="success_count DESC",
        limit=10,
    )
    unsynced = [g for g in top_genes if g.synced_at is None]
    if unsynced and client.sync:
        result = client.sync.push_to_hub(gene_ids=[g.gene_id for g in unsynced])
        print(f"Shared {result.pushed} genes, {result.failed} failed")
```

### Go: 微服务集成

```go
package main

import (
    "context"
    "fmt"
    "time"

    "github.com/Colin4k1024/Oris/sdks/go"
)

func main() {
    seed := [32]byte{} // load from secure config
    copy(seed[:], loadSeedFromVault())

    client := oris.NewClient(oris.Config{
        StorePath: "./service_experience.db",
        Experience: &oris.ExperienceSync{
            BaseURL:  "https://repo.oris.network:3000",
            APIKey:   "oris_key_xxxxx",
            Seed:     seed,
            SenderID: "payment-service-001",
        },
    })
    defer client.Close()

    ctx := context.Background()

    // 启动时从 Hub 拉取最新经验
    pulled, err := client.Sync.PullFromHub(ctx, oris.PullOpts{
        TaskClass:     "timeout-recovery",
        MinConfidence: 0.8,
        Limit:         50,
    })
    if err != nil {
        fmt.Printf("Pull failed (non-fatal): %v\n", err)
    } else {
        fmt.Printf("Pulled %d experiences from hub\n", len(pulled))
    }

    // 运行时使用本地经验
    matches, _ := client.Store.Query(ctx, oris.StoreQuery{
        TaskClass:     "timeout-recovery",
        MinConfidence: 0.7,
        Limit:         1,
    })
    if len(matches) > 0 {
        applyRecoveryStrategy(matches[0].Strategy)
    }

    // 定期同步（可选）
    ticker := time.NewTicker(1 * time.Hour)
    go func() {
        for range ticker.C {
            client.Sync.PushToHub(ctx, oris.PushOpts{})
            client.Sync.PullFromHub(ctx, oris.PullOpts{Limit: 100})
        }
    }()
}
```

### TypeScript: Next.js API Route 集成

```typescript
import { OrisClient, Gene } from "@colin4k1024/oris-sdk";

const client = new OrisClient({
  storePath: "./data/oris_experience.db",
  experience: {
    baseUrl: process.env.ORIS_REPO_URL ?? "https://repo.oris.network:3000",
    apiKey: process.env.ORIS_API_KEY!,
    seed: new Uint8Array(Buffer.from(process.env.ORIS_SEED!, "hex")),
    senderId: "webapp-001",
  },
});

// GET /api/experience — 查询本地经验
export async function GET(req: Request) {
  const url = new URL(req.url);
  const taskClass = url.searchParams.get("task_class") ?? "";

  const genes = await client.store.query({
    taskClass,
    minConfidence: 0.5,
    limit: 20,
  });

  return Response.json({ genes, total: genes.length });
}

// POST /api/experience/sync — 手动触发同步
export async function POST(req: Request) {
  const { action } = await req.json();

  if (action === "pull") {
    const pulled = await client.sync.pullFromHub({ limit: 50 });
    return Response.json({ pulled: pulled.length });
  }

  if (action === "push") {
    const result = await client.sync.pushToHub();
    return Response.json(result);
  }

  return Response.json({ error: "unknown action" }, { status: 400 });
}
```

---

## 与现有 SDK 的关系

当前已发布的 SDK（Go/Python/TypeScript）直接对接三个 Oris 服务，属于 **Remote-First** 模式。Local-First 架构是对现有 SDK 的**上层封装**：

```
Local-First Layer (新增)
├── LocalStore      ← 新增：SQLite 本地存储
├── SyncManager     ← 新增：同步协调器
└── OrisClient      ← 新增：统一入口

现有 SDK Layer (保留)
├── HubClient       ← 保留：Hub API 客户端
├── ExecutionClient ← 保留：Execution API 客户端
└── ExperienceClient← 保留：Experience Repo API 客户端

Internal Layer (保留)
├── Signing         ← 保留：Ed25519 签名
├── CanonicalJson   ← 保留：确定性 JSON 序列化
└── Errors          ← 保留：错误类型体系
```

集成方可以：
- 只使用 `OrisClient`（Local-First 推荐方式）
- 直接使用 `ExperienceClient` / `HubClient`（向后兼容，Remote-First 方式）
- 混合使用（本地存储 + 直接调用特定 API）

---

## 包发布计划

| 语言 | 包名 | 新增模块 |
|------|------|----------|
| Go | `github.com/Colin4k1024/Oris/sdks/go` | `store/`, `sync/`, `oris.go`(OrisClient) |
| Python | `oris-rt-sdk` | `oris_sdk/store.py`, `oris_sdk/sync.py`, `oris_sdk/client.py` |
| TypeScript | `@colin4k1024/oris-sdk` | `src/store.ts`, `src/sync.ts`, `src/client.ts` |

### 新增依赖

| 语言 | 依赖 | 用途 |
|------|------|------|
| Go | `modernc.org/sqlite` (CGo-free) | 本地 SQLite |
| Python | `aiosqlite` 或 `sqlite3`(stdlib) | 本地 SQLite |
| TypeScript | `better-sqlite3` 或 `sql.js` | 本地 SQLite |

---

## 安全考虑

1. **Seed 保护**: Ed25519 seed 是私钥，必须安全存储（环境变量、Vault、KeyChain），不能硬编码
2. **本地数据加密**: SQLite 文件默认不加密；如有安全需求，集成方可使用 SQLCipher
3. **API Key 作用域**: Experience Repo 的 API Key 只控制写权限，读是公开的
4. **签名验证**: Hub/Repo 服务端验证签名，防止篡改；本地不验证远端数据签名（信任 Hub）
5. **同步审计**: 所有同步操作记入 SyncLog，可追溯

---

## FAQ

**Q: 没有网络时 SDK 能用吗？**
A: 能。LocalStore 完全本地化，query/save/updateStats 全部离线可用。只有 push/pull 需要网络。

**Q: 自动同步吗？**
A: 不会。所有同步必须由集成方代码显式触发（`pushToHub` / `pullFromHub`）。SDK 不会后台偷偷上传数据。

**Q: 本地数据有大小限制吗？**
A: SQLite 理论上支持 TB 级数据。建议通过 `store.query(limit=...)` 控制查询结果集大小。

**Q: 能只用本地，完全不配置 Hub 吗？**
A: 能。只传 `store_path`，不传 `hub`/`experience` 配置即可。`client.sync` 会是 nil/None/undefined。

**Q: Gene ID 是谁生成的？**
A: 集成方本地生成（UUID-based），推送到 Hub 后保持不变。不同节点不会产生 ID 冲突（UUID 碰撞概率可忽略）。

**Q: Execution Runtime 也走本地优先吗？**
A: 不是。任务执行（`ExecutionClient`）仍然是直接调用远端服务。Local-First 只针对经验/基因数据。
