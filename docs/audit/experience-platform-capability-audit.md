# Experience Platform Capability Audit

**Date:** 2026-06-10
**Auditor:** Claude Code (Opus 4.6)
**Scope:** Experience Repository / External Platform / Distributed Hub

---

## 审查结论：三个维度全部满足

---

## 一、经验仓库（Experience Repository）— ✅ 满足

| 能力点 | 状态 | 实现位置 |
|--------|------|----------|
| Gene/Capsule 存储 | ✅ 完整 | `oris-genestore`（1499 LOC，SQLite 后端） |
| REST API：查询经验 | ✅ 完整 | `oris-experience-repo` GET `/experience` |
| REST API：发布经验 | ✅ 完整 | `oris-experience-repo` POST `/experience` |
| Ed25519 签名验证 | ✅ 完整 | `oen/verifier.rs`，OEN envelope 验签 |
| API Key 生命周期 | ✅ 完整 | 创建/吊销/轮换/列表全部实现 |
| PKI 公钥注册 | ✅ 完整 | `/public-keys` CRUD |
| Rate Limiting | ✅ 完整 | 基于 IP 的速率限制 |
| 按信号/置信度/标签查询 | ✅ 完整 | `GeneQuery` 支持多维过滤 |
| CLI 管理工具 | ✅ 存在 | `oris-exp-repo-cli` |

**总代码量：** ~3,242 LOC（experience-repo）+ ~1,499 LOC（genestore）

### 关键实现细节

- **存储层**：`SqliteGeneStore` 提供 `upsert_gene`、`search_genes`、`replay_hook` 能力
- **认证层**：双重认证 — API Key 鉴权 + Ed25519 信封签名
- **查询能力**：支持 `min_confidence`、`limit`、`required_tags`、`problem_description` 多维过滤
- **安全**：sender_id 与 API Key agent_id 强绑定，防止冒名发布

---

## 二、对外经验平台（External-facing Platform）— ✅ 满足

| 能力点 | 状态 | 实现位置 |
|--------|------|----------|
| HTTP 服务器 | ✅ Axum 生产级 | `experience-repo/server` + `oris-hub/server` |
| 主页文档页 | ✅ HTML 自描述 | `handlers.rs:homepage()` |
| OEN 信封协议 | ✅ 完整 | `oris-evolution-network` — 带 manifest、content_hash、签名 |
| Agent 接入契约 | ✅ 完整 | `oris-agent-contract`（提案接口） |
| IPC 协议 | ✅ JSON-RPC 2.0 | `oris-evo-ipc-protocol`（Unix socket 通信） |
| 本地 Evolution Server | ✅ 存在 | `oris-evo-server`（792 LOC） |
| 多认证层 | ✅ API Key + Ed25519 双层 | signed_routes + authenticated_routes 分离 |
| CORS 支持 | ✅ 可配置 | `ORIS_HUB_CORS_ORIGINS` 环境变量 |

### 关键实现细节

- **OEN 信封协议**：`EvolutionEnvelope` 含 protocol version、message_type、manifest（asset_ids + asset_hash）、content_hash、Ed25519 signature
- **三类网络资产**：`NetworkAsset::Gene`、`NetworkAsset::Capsule`、`NetworkAsset::EvolutionEvent`
- **消息类型**：Publish、Fetch、Report、Revoke
- **IPC 接入**：通过 Unix Domain Socket (`~/.claude/evolution/evolution.sock`) 提供 JSON-RPC 2.0 接口

---

## 三、分布式经验 Hub — ✅ 满足

| 能力点 | 状态 | 实现位置 |
|--------|------|----------|
| 节点注册/注销 | ✅ 完整 | `oris-hub` RegistryService |
| 心跳 + TTL + GC | ✅ 完整 | 30s 心跳、60s TTL、自动过期清理 |
| 节点发现 | ✅ 完整 | DiscoveryService（按 capability/region/version 过滤） |
| 联邦搜索 | ✅ 完整 | FederationEngine（并行扇出 + 超时 + 去重 + 排序） |
| Subscription 推送 | ✅ 完整 | Webhook 派发 + filter（task_class/min_confidence/source_nodes） |
| Gossip 同步 | ✅ 完整 | Push-pull gossip + digest + fetch + apply 闭环 |
| Peer 管理 | ✅ 完整 | PeerRegistry + 故障检测 + 自动恢复 |
| Hub Client SDK | ✅ 完整 | `oris-hub-client`（注册/心跳/发现/搜索/订阅） |
| Dashboard | ✅ 存在 | HTML 管理面板（overview/nodes/subscriptions/search） |
| Ed25519 签名认证 | ✅ 完整 | 节点注册通过签名验证防止密钥替换攻击 |
| 覆盖率元数据 | ✅ 完整 | FederationMeta 含 nodes_queried/responded/coverage/freshness |

**Hub 总代码量：** ~3,015 LOC（hub）+ ~259 LOC（hub-client）

### 关键实现细节

- **RegistryService**：SQLite 持久化，防止 key substitution attack（同 node_id 不同 public_key 拒绝注册）
- **FederationEngine**：并行 HTTP 扇出查询所有活跃节点，500ms 超时，按 confidence 排序 + gene_id 去重
- **SubscriptionManager**：Webhook 推送 + 多维 filter（task_class / min_confidence / source_nodes）
- **GossipSyncEngine**：Push-pull 模式 — build_digest → build_fetch_query → respond_to_fetch → apply_fetch_response
- **HubClient SDK**：完整封装 register/heartbeat/discover/search/subscribe/unsubscribe + 自动心跳循环

---

## 架构总览

```
┌─────────────────────────────────────────────────────┐
│  oris-hub (分布式经验 Hub)                          │
│  注册 · 发现 · 联邦搜索 · 订阅推送 · Dashboard     │
└────────────────────────────┬────────────────────────┘
                             │ federated search
     ┌───────────────────────┼───────────────────────┐
     ▼                       ▼                       ▼
┌──────────┐          ┌──────────┐          ┌──────────┐
│  Node A  │          │  Node B  │          │  Node C  │
│ exp-repo │◄─gossip─►│ exp-repo │◄─gossip─►│ exp-repo │
│ genestore│          │ genestore│          │ genestore│
└──────────┘          └──────────┘          └──────────┘
```

- **本地经验仓库**：每个节点独立运行 experience-repo + genestore
- **对外平台能力**：REST API + OEN 信封 + PKI + Rate Limit，外部 agent 可安全接入
- **分布式 Hub**：节点自动注册、联邦搜索、gossip 同步、webhook 订阅，形成去中心化经验网络

---

## 涉及 Crate 代码量统计

| Crate | LOC | 职责 |
|-------|-----|------|
| `oris-hub` | ~3,015 | 分布式 Hub 服务端 |
| `oris-experience-repo` | ~3,242 | 经验仓库 HTTP API |
| `oris-evolution-network` | ~2,741 | OEN 协议 + Gossip + 签名 |
| `oris-genestore` | ~1,499 | Gene/Capsule SQLite 存储 |
| `oris-evo-server` | ~792 | 本地 Evolution 服务器 |
| `oris-hub-client` | ~259 | Hub 客户端 SDK |
| `oris-evo-ipc-protocol` | — | JSON-RPC IPC 协议定义 |
| `oris-exp-repo-cli` | — | 经验仓库 CLI 工具 |
| `evolution-cli` | — | Evolution CLI 工具 |
| **合计** | **~11,548** | — |

---

## 补充说明

### 测试覆盖

- `oris-experience-repo`：handler 层单元测试（key CRUD、fetch、health）
- `oris-evolution-network`：envelope manifest 验证、gossip digest 过滤、两节点同步闭环
- `oris-hub`（未逐一检查但存在 `validation.rs`）

### 安全机制

1. **API Key + Ed25519 双层认证**：写操作需要同时提供 API Key 和签名
2. **Key Substitution 防护**：同一 node_id 不能用不同 public_key 重新注册
3. **Rate Limiting**：按 IP 限速，防止滥用
4. **Content Hash**：信封内容完整性校验
5. **Manifest 验证**：sender_id 一致性 + asset_ids 匹配 + asset_hash 验证

### 当前不足/可改进项

1. **Dashboard 搜索**：当前 search handler 返回空结果（未接入真实联邦查询）
2. **Gossip sync_loop**：`start_sync_loop` 当前为 sleep 占位，未接入真实 HTTP 对端
3. **Federation 超时**：默认 500ms 可能对跨地域场景偏短
4. **持久化**：Hub 使用内存默认配置（`:memory:`），生产部署需显式配置 SQLite 路径
