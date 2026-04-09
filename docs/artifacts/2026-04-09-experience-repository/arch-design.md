---
artifact: arch-design
task: experience-repository
date: 2026-04-09
role: architect
status: draft
---

# 架构设计 — 经验仓库 (Experience Repository)

## 1. 系统边界

```
┌─────────────────────────────────────────────────────────┐
│  External Agent (Caller)                                │
│  - 持有 API Key                                        │
│  - 通过 HTTP 调用                                      │
└────────────┬──────────────────────────────────────────┘
             │ HTTP/REST
             ▼
┌─────────────────────────────────────────────────────────┐
│  oris-experience-repo: ExperienceRepoServer            │
│  - Axum HTTP Server                                   │
│  - API Key 验证中间件                                  │
│  - Fetch 端点: GET /experience                         │
└────────────┬──────────────────────────────────────────┘
             │
             ▼
┌─────────────────────────────────────────────────────────┐
│  oris-genestore: SqliteGeneStore                       │
│  - Gene、Capsule 持久化                                │
│  - 索引查询、confidence 衰减                           │
└─────────────────────────────────────────────────────────┘
```

### 外部依赖

| 依赖 | 版本 | 用途 |
|------|------|------|
| `oris-genestore` | 0.2.0 | Gene/Capsule 存储 |
| `oris-evolution` | 0.4.1 | Gene、Capsule 类型定义 |
| `axum` | latest | HTTP 服务器 |
| `rusqlite` | latest | SQLite（通过 genestore 间接） |

### 边界划分

- **外部接口**：HTTP REST API（ExperienceRepoServer）
- **内部接口**：SqliteGeneStore trait（复用现有实现）
- **不涉及**：OEN gossip 同步（由 oris-evolution-network 独立处理）

## 2. 组件拆分

```
oris-experience-repo/
├── src/
│   ├── lib.rs              # 库入口，导出 Server/Client
│   ├── server/
│   │   ├── mod.rs          # Axum router + 路由定义
│   │   ├── handlers.rs     # HTTP handler 实现
│   │   └── middleware.rs    # API Key 验证中间件
│   ├── client/
│   │   ├── mod.rs          # Client 库入口
│   │   └── client.rs        # HTTP Client 实现
│   ├── api/
│   │   ├── mod.rs          # API types
│   │   ├── request.rs      # FetchQuery, PublishRequest
│   │   └── response.rs     # FetchResponse, ErrorResponse
│   └── error.rs            # 错误类型定义
└── Cargo.toml
```

### 组件职责

| 组件 | 职责 |
|------|------|
| `server/` | Axum HTTP 服务器，路由定义，handler 实现 |
| `middleware.rs` | API Key 验证，X-Api-Key header 提取与校验 |
| `client/` | 供外部 Agent 使用的客户端库 |
| `api/` | HTTP 请求/响应类型定义 |

## 3. 关键数据流

### Fetch 流程

```
Agent
  │
  │ GET /experience?q=timeout,error&min_confidence=0.5&limit=10
  │ X-Api-Key: {api_key}
  │
  ▼
Middleware (API Key 验证)
  │
  ▼
Handler::fetch_experiences
  │ - 解析 query string
  │ - 构建 GeneQuery
  │
  ▼
SqliteGeneStore::search_genes(query)
  │ - keyword matching
  │ - confidence 过滤
  │ - relevance scoring
  │
  ▼
FetchResponse { assets: Vec<NetworkAsset>, next_cursor, sync_audit }
  │
  ▼
HTTP 200 OK
```

### API Key 验证流程

```
请求进入
  │
  ▼
提取 X-Api-Key header
  │
  ▼
查询 ApiKeyStore (内存 HashMap，配置文件初始化)
  │
  ├── 找到且有效 → 提取 agent_id → 请求上下文
  │
  └── 未找到或无效 → HTTP 401 Unauthorized
```

## 4. 接口约定

### 基础信息

| 字段 | 值 |
|------|---|
| Base URL | `http://localhost:8080` |
| Content-Type | `application/json` |
| 认证 | `X-Api-Key` header |

### API 端点

#### GET /experience

查询匹配的经验。

**Request Headers**:
```
X-Api-Key: {api_key}
```

**Query Parameters**:
| 参数 | 类型 | 必填 | 默认值 | 说明 |
|------|------|------|--------|------|
| `q` | string | 是 | - | 逗号分隔的问题信号 |
| `min_confidence` | f64 | 否 | 0.5 | 最低置信度 |
| `limit` | usize | 否 | 10 | 返回数量上限 |
| `cursor` | string | 否 | - | 分页游标 |

**Response 200**:
```json
{
  "assets": [
    {
      "gene": {
        "id": "uuid",
        "name": "timeout recovery",
        "signals": ["timeout", "error"],
        "strategy": ["step1", "step2"],
        "validation": ["cargo test"],
        "confidence": 0.85,
        "quality_score": 0.9,
        "use_count": 12,
        "created_at": "2026-04-01T00:00:00Z"
      }
    }
  ],
  "next_cursor": "optional_cursor_token",
  "sync_audit": {
    "scanned_count": 100,
    "applied_count": 5,
    "skipped_count": 95,
    "failed_count": 0
  }
}
```

**Error Responses**:
| Status | 说明 |
|--------|------|
| 400 | 参数解析错误 |
| 401 | API Key 无效或缺失 |
| 500 | 服务器内部错误 |

#### GET /health

健康检查。

**Response 200**:
```json
{
  "status": "ok",
  "version": "0.1.0"
}
```

## 5. 技术选型

| 选择 | 原因 |
|------|------|
| **Axum 0.8** | 异步 HTTP框架，与 tokio 生态集成好，tower 兼容 |
| **API Key 认证** | 简化外部 Agent 身份验证，与 OEN Ed25519 签名正交 |
| **内存 HashMap 存 API Key** | 第一期简化实现；可演进为 SQLite 表或外部 Key Service |
| **复用 SqliteGeneStore** | 避免重复建设存储层，复用现有索引和查询能力 |

## 6. 错误处理

```rust
#[derive(Debug, thiserror::Error)]
pub enum ExperienceRepoError {
    #[error("api key missing")]
    ApiKeyMissing,

    #[error("invalid api key")]
    InvalidApiKey,

    #[error("query parse error: {0}")]
    QueryParseError(String),

    #[error("gene store error: {0}")]
    GeneStoreError(#[from] GeneStoreError),

    #[error("internal error: {0}")]
    InternalError(String),
}
```

## 7. 风险与约束

| 风险 | 影响 | 缓解 |
|------|------|------|
| API Key 硬编码在配置文件 | 安全风险 | 第一期仅内部使用；二期引入 Key Service |
| keyword matching 无法语义搜索 | 查询质量 | 标注限制条件，二期引入向量搜索 |
| SQLite WAL 写入瓶颈 | 并发能力 | 监控连接数，必要时引入连接池 |

## 8. 第一期 MVP 排除项

以下功能因 P0 阻断项（身份凭证体系未定义）延后：

- POST /experience（经验贡献）
- POST /experience/{id}/feedback（反馈）
- OEN Envelope 签名验证
- 向量语义搜索

## 9. 后续演进方向

1. **二期**：引入 Key Service，支持 API Key 发放和轮换
2. **三期**：实现 Share 和 Feedback 功能
3. **四期**：集成向量搜索（SQLite VSS 或 pgvector）
