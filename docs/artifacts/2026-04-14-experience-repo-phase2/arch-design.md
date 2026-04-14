---
artifact: arch-design
task: experience-repo-phase2
date: 2026-04-14
role: architect
status: draft
---

# 架构设计 — 经验仓库二期 (Experience Repository Phase 2)

## 1. 系统边界

```
┌─────────────────────────────────────────────────────────────────────┐
│  External Agent (Caller)                                              │
│  - 持有 API Key + Ed25519 私钥                                      │
│  - 构造 OEN Envelope 并签名                                          │
└─────────────┬───────────────────────────────────────────────────────┘
              │ HTTP/REST + OEN Envelope
              ▼
┌─────────────────────────────────────────────────────────────────────┐
│  oris-experience-repo: ExperienceRepoServer                           │
│                                                                       │
│  ┌────────────────┐  ┌─────────────────┐  ┌───────────────────────┐  │
│  │ KeyService     │  │ OenVerifier     │  │ ShareHandler          │  │
│  │ - API Key 验证 │  │ - Ed25519 验签 │  │ - POST /experience    │  │
│  │ - Key CRUD     │  │ - Envelope解析 │  │ - Gene 存储           │  │
│  └────────────────┘  └─────────────────┘  └───────────────────────┘  │
│                                                                       │
│  ┌────────────────────────────────────────────────────────────────┐  │
│  │ FetchHandler (MVP 已有)                                          │  │
│  │ - GET /experience                                               │  │
│  └────────────────────────────────────────────────────────────────┘  │
└─────────────┬───────────────────────────────────────────────────────┘
              │
              ▼
┌─────────────────────────────────────────────────────────────────────┐
│  oris-genestore: SqliteGeneStore                                      │
│  - Gene、Capsule 持久化                                               │
└─────────────────────────────────────────────────────────────────────┘
```

### 外部依赖

| 依赖 | 版本 | 用途 |
|------|------|------|
| `oris-genestore` | 0.2.0 | Gene/Capsule 存储 |
| `oris-evolution` | 0.4.1 | Gene、Capsule 类型定义 |
| `oris-evolution-network` | 0.5.0 | OEN Envelope、Ed25519 signing |
| `oris-evo-ipc-protocol` | 0.1.0 | IPC 协议定义 |
| `axum` | latest | HTTP 服务器 |

### 边界划分

- **外部接口**：HTTP REST API + OEN Envelope
- **内部接口**：SqliteGeneStore trait、KeyStore trait
- **签名验证**：复用 oris-evolution-network::signing 模块

## 2. 组件拆分

```
oris-experience-repo/
├── src/
│   ├── lib.rs                    # 库入口
│   ├── error.rs                  # 错误类型（扩展）
│   │
│   ├── server/
│   │   ├── mod.rs               # Axum router + 路由定义
│   │   ├── handlers.rs          # HTTP handler（Fetch + Share）
│   │   ├── middleware.rs        # API Key 验证中间件
│   │   └── key_management.rs     # Key 管理 handler
│   │
│   ├── client/
│   │   ├── mod.rs
│   │   └── client.rs            # HTTP Client（扩展支持 Share）
│   │
│   ├── api/
│   │   ├── mod.rs
│   │   ├── request.rs           # FetchQuery, ShareRequest, KeyRequests
│   │   └── response.rs          # FetchResponse, ShareResponse, ErrorResponse
│   │
│   ├── key_service/
│   │   ├── mod.rs               # KeyService 模块入口
│   │   ├── keystore.rs          # Key 存储（SQLite）
│   │   ├── key_types.rs         # ApiKey, KeyId, KeyStatus
│   │   └── error.rs             # KeyService 错误
│   │
│   └── oen/
│       ├── mod.rs               # OEN Envelope 处理
│       ├── verifier.rs          # Ed25519 签名验证
│       └── envelope_types.rs    # OEN 消息类型
│
├── examples/
│   └── server.rs                # 服务器示例
│
└── Cargo.toml
```

### 组件职责

| 组件 | 职责 | 公开 API |
|------|------|----------|
| `server/` | Axum HTTP 服务器，路由定义，handler 实现 | 启动服务器 |
| `key_service/` | API Key 全生命周期管理 | create_key, verify_key, revoke_key, rotate_key, list_keys |
| `oen/` | OEN Envelope 解析和 Ed25519 签名验证 | verify_envelope, parse_envelope |
| `middleware.rs` | API Key 验证中间件 | 从 header 提取并验证 Key |
| `client/` | 供外部 Agent 使用的客户端库 | ExperienceRepoClient |

## 3. 关键数据流

### Share 流程（完整）

```
Agent
  │
  │ 1. 构造 OEN Envelope
  │    Envelope {
  │      sender_id: "agent-123",
  │      message_type: "Publish",
  │      payload: { gene: Gene },
  │      signature: Ed25519::sign(payload),
  │      timestamp: now()
  │    }
  │
  │ 2. POST /experience
  │    Header: X-Api-Key: {api_key}
  │    Body: { envelope: {...} }
  │
  ▼
Middleware (API Key 验证)
  │
  ▼
ShareHandler::share_experience
  │
  ├─→ KeyService::verify_key(api_key)
  │     │
  │     ▼
  │     查询 KeyStore，验证：
  │     - Key 存在
  │     - Key 状态为 Active
  │     - Key 未过期
  │     - Key 属于请求的 agent_id
  │
  ├─→ OenVerifier::verify_envelope(envelope)
  │     │
  │     ▼
  │     验证：
  │     - message_type == "Publish"
  │     - sender_id 与 Key 的 agent_id 匹配
  │     - timestamp 在允许范围内（±5分钟）
  │     - Ed25519 签名有效
  │
  ├─→ SqliteGeneStore::store_gene(gene)
  │     │
  │     ▼
  │     存储 Gene，返回 gene_id
  │
  ▼
ShareResponse { gene_id, status: "published" }
  │
  ▼
HTTP 201 Created
```

### Key 管理流程

```
运维人员
  │
  │ POST /keys (agent_id, ttl optional)
  │
  ▼
KeyService::create_key
  │
  ├─→ 生成新 KeyId (uuid)
  ├─→ 生成新 ApiKey (secure_random 32 bytes)
  ├─→ 存储到 KeyStore (status: Active)
  │
  ▼
返回 { key_id, api_key, agent_id, created_at }
（api_key 仅在此刻可见一次）
```

### Key 验证流程

```
请求进入
  │
  ▼
提取 X-Api-Key header
  │
  ▼
KeyService::verify_key(api_key)
  │
  ├─→ 查询 KeyStore WHERE api_key_hash = hash(input_key)
  │
  ├── Key 存在且有效 → 返回 ApiKeyInfo { agent_id, key_id, status }
  │
  ├── Key 不存在 → Err(InvalidKey)
  │
  ├── Key 已撤销 → Err(RevokedKey)
  │
  └── Key 已过期 → Err(ExpiredKey)
```

## 4. 接口约定

### 基础信息

| 字段 | 值 |
|------|---|
| Base URL | `http://localhost:8080` |
| Content-Type | `application/json` |
| 认证 | `X-Api-Key` header |
| 签名协议 | OEN Envelope + Ed25519 |

### API 端点

#### POST /experience（Share）

发布经验到仓库。

**Request Headers**:
```
X-Api-Key: {api_key}
Content-Type: application/json
```

**Request Body**:
```json
{
  "envelope": {
    "sender_id": "agent-123",
    "message_type": "Publish",
    "payload": {
      "gene": {
        "id": "optional-uuid",
        "name": "timeout recovery",
        "signals": ["timeout", "connection"],
        "strategy": ["step1: increase timeout", "step2: add retry"],
        "validation": ["cargo test"],
        "confidence": 0.85,
        "quality_score": 0.9
      }
    },
    "signature": "base64_ed25519_signature",
    "timestamp": "2026-04-14T10:30:00Z"
  }
}
```

**Response 201**:
```json
{
  "gene_id": "uuid-of-stored-gene",
  "status": "published",
  "published_at": "2026-04-14T10:30:01Z"
}
```

**Error Responses**:
| Status | Error Code | 说明 |
|--------|------------|------|
| 400 | InvalidEnvelope | Envelope 格式错误 |
| 401 | InvalidApiKey | API Key 无效 |
| 401 | RevokedKey | Key 已撤销 |
| 401 | ExpiredKey | Key 已过期 |
| 403 | SignatureMismatch | Ed25519 签名验证失败 |
| 403 | SenderMismatch | sender_id 与 Key owner 不匹配 |
| 409 | DuplicateGene | Gene ID 冲突（已存在） |
| 500 | InternalError | 服务器内部错误 |

#### GET /experience（Fetch，MVP 已有）

查询匹配的经验。

**Response 200**:
```json
{
  "assets": [
    {
      "gene": { ... },
      "contributor_id": "agent-123"
    }
  ],
  "next_cursor": null,
  "sync_audit": { ... }
}
```

#### Key Management API

##### POST /keys

创建新 API Key（内部管理接口）。

**Request Body**:
```json
{
  "agent_id": "agent-123",
  "ttl_days": 90,
  "description": "CI/CD pipeline key"
}
```

**Response 201**:
```json
{
  "key_id": "uuid",
  "api_key": "sk_live_xxx...（仅此一次可见）",
  "agent_id": "agent-123",
  "created_at": "2026-04-14T10:30:00Z",
  "expires_at": "2026-07-13T10:30:00Z"
}
```

##### GET /keys

列出所有 Key（不包含 api_key 明文）。

**Response 200**:
```json
{
  "keys": [
    {
      "key_id": "uuid",
      "agent_id": "agent-123",
      "status": "Active",
      "created_at": "2026-04-14T10:30:00Z",
      "expires_at": "2026-07-13T10:30:00Z",
      "last_used_at": "2026-04-14T12:00:00Z"
    }
  ]
}
```

##### DELETE /keys/{key_id}

撤销 Key。

**Response 204**: No Content

##### POST /keys/{key_id}/rotate

轮换 Key（撤销旧 Key，生成新 Key）。

**Response 200**:
```json
{
  "key_id": "uuid",
  "api_key": "sk_live_yyy...（新 Key，仅此一次可见）",
  "rotated_at": "2026-04-14T10:30:00Z"
}
```

## 5. 数据模型

### KeyStore Schema（SQLite）

```sql
CREATE TABLE api_keys (
    key_id TEXT PRIMARY KEY,
    api_key_hash TEXT NOT NULL UNIQUE,  -- SHA-256 hash of api_key
    agent_id TEXT NOT NULL,
    description TEXT,
    status TEXT NOT NULL DEFAULT 'Active',  -- Active | Revoked | Expired
    created_at TEXT NOT NULL,
    expires_at TEXT,
    revoked_at TEXT,
    last_used_at TEXT
);

CREATE INDEX idx_api_keys_agent_id ON api_keys(agent_id);
CREATE INDEX idx_api_keys_status ON api_keys(status);
```

### OEN Envelope 结构

```rust
pub struct OenEnvelope {
    pub sender_id: String,
    pub message_type: MessageType,  // Publish | Fetch | Feedback
    pub payload: serde_json::Value,
    pub signature: Vec<u8>,  // Ed25519 签名
    pub timestamp: DateTime<Utc>,
}

pub enum MessageType {
    Publish,
    Fetch,
    Feedback,
}
```

## 6. 技术选型

| 选择 | 原因 |
|------|------|
| **Key 存储：SQLite** | 与 GeneStore 共用 DB，简化部署 |
| **Key 验证：Hash 对比** | 不存储明文 Key，仅存 SHA-256 hash |
| **签名缓存** | TTL 5分钟，减少重复验签开销 |
| **OEN Envelope** | 复用 oris-evolution-network 的协议定义 |
| **Ed25519** | 复用 oris-evolution-network 的 signing 模块 |

## 7. Key Service 设计

### Key 全生命周期

```
创建 → Active → (轮换) → Active (新)
              ↘ (撤销) → Revoked
              ↘ (过期) → Expired
```

### Key 验证算法

```rust
fn verify_key(api_key: &str, agent_id: &str) -> Result<ApiKeyInfo, KeyError> {
    let key_hash = sha256(api_key);
    let record = key_store.get_by_hash(&key_hash)?;

    match record.status {
        Status::Active if record.expires_at > now() => {
            if record.agent_id == agent_id {
                key_store.update_last_used(key_id)?;
                Ok(record.into())
            } else {
                Err(KeyError::AgentMismatch)
            }
        }
        Status::Active => Err(KeyError::Expired),
        Status::Revoked => Err(KeyError::Revoked),
        Status::Expired => Err(KeyError::Expired),
    }
}
```

### OEN Envelope 验证算法

```rust
fn verify_envelope(envelope: &OenEnvelope, expected_agent_id: &str) -> Result<(), OenError> {
    // 1. 检查 message_type
    if envelope.message_type != MessageType::Publish {
        return Err(OenError::InvalidMessageType);
    }

    // 2. 检查 sender_id 与 agent_id 匹配
    if envelope.sender_id != expected_agent_id {
        return Err(OenError::SenderMismatch);
    }

    // 3. 检查 timestamp 在允许范围内（±5分钟）
    let now = Utc::now();
    let diff = (now - envelope.timestamp).abs();
    if diff > Duration::minutes(5) {
        return Err(OenError::TimestampExpired);
    }

    // 4. 验证 Ed25519 签名
    let payload_bytes = serde_json::to_vec(&envelope.payload)?;
    signing::verify(&envelope.sender_id, &payload_bytes, &envelope.signature)?;

    Ok(())
}
```

## 8. 错误处理

### KeyService 错误

```rust
#[derive(Debug, thiserror::Error)]
pub enum KeyError {
    #[error("key not found")]
    KeyNotFound,

    #[error("invalid api key")]
    InvalidKey,

    #[error("key expired")]
    Expired,

    #[error("key revoked")]
    Revoked,

    #[error("agent_id mismatch")]
    AgentMismatch,

    #[error("store error: {0}")]
    StoreError(String),
}
```

### OEN 错误

```rust
#[derive(Debug, thiserror::Error)]
pub enum OenError {
    #[error("invalid envelope format")]
    InvalidEnvelope,

    #[error("invalid message type")]
    InvalidMessageType,

    #[error("sender_id mismatch")]
    SenderMismatch,

    #[error("timestamp expired")]
    TimestampExpired,

    #[error("signature verification failed")]
    SignatureFailed,

    #[error("signing module error: {0}")]
    SigningError(String),
}
```

### HTTP 错误映射

| 内部错误 | HTTP Status | Error Code |
|----------|-------------|-------------|
| KeyError::InvalidKey | 401 | INVALID_API_KEY |
| KeyError::Expired | 401 | KEY_EXPIRED |
| KeyError::Revoked | 401 | KEY_REVOKED |
| KeyError::AgentMismatch | 403 | AGENT_MISMATCH |
| OenError::SignatureFailed | 403 | INVALID_SIGNATURE |
| OenError::TimestampExpired | 403 | TIMESTAMP_EXPIRED |
| KeyError::KeyNotFound | 404 | KEY_NOT_FOUND |

## 9. 风险与缓解

| 风险 | 影响 | 缓解措施 | 优先级 |
|------|------|----------|--------|
| Ed25519 验签性能瓶颈 | 高并发时延迟增加 | 签名缓存（TTL 5分钟） | P1 |
| Key 存储单点 | Key Service 不可用 | 与 GeneStore 共用 SQLite | P2 |
| 首次部署复杂度 | 运维成本 | 提供 CLI init 工具 | P2 |
| Key 泄露风险 | 安全风险 | Hash 存储、仅首次显示明文 | P1 |
| 时间同步问题 | 签名验证失败 | 放宽 timestamp 窗口（±5分钟） | P3 |

## 10. 部署考量

### 初始化流程

```bash
# 1. 启动服务器
cargo run -p oris-experience-repo --example server

# 2. 初始化第一个 Admin Key（一次性）
oris-exp-repo-cli admin init --agent-id admin

# 3. 使用 Admin Key 创建 Agent Key
curl -X POST /keys \
  -H "X-Api-Key: $ADMIN_KEY" \
  -d '{"agent_id": "agent-123"}'
```

### 配置

```yaml
# experience-repo.yaml
server:
  host: "0.0.0.0"
  port: 8080

database:
  path: ".oris/experience_repo.db"

key_service:
  # Key 默认 TTL（天）
  default_ttl_days: 90
  # 时间戳容忍窗口（秒）
  timestamp_tolerance_secs: 300

signing:
  # Ed25519 签名验证缓存 TTL（秒）
  signature_cache_ttl_secs: 300
```

## 11. 测试策略

### 单元测试
- KeyService: CRUD + 验证逻辑
- OenVerifier: Envelope 解析 + 签名验证
- ShareHandler: 请求验证 + 响应构造

### 集成测试
- Share 完整流程（从 Envelope 构建到 Gene 存储）
- Key 管理完整流程
- 错误路径（无效 Key、无效签名、超时）

### 性能测试
- Ed25519 验签吞吐量
- 并发 Share 请求

## 12. 后续演进

### 三期（Feedback）
- POST /experience/{id}/feedback
- 基于 Share 的签名验证扩展

### 四期（向量搜索）
- 集成 SQLite VSS 或 pgvector
- 替代 keyword matching
