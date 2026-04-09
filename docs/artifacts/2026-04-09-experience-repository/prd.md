---
artifact: prd
task: experience-repository
date: 2026-04-09
role: tech-lead
status: challenge-complete
---

# 经验仓库 (Experience Repository) - PRD

## 1. 背景与问题

Oris 是一个自进化执行运行时，现有 `oris-genestore` 提供本地 SQLite 存储，`oris-evolution-network` 提供节点间 Gene/Capsule 的 gossip 同步能力。但当前缺乏一个**集中式的经验仓库服务**，使得：

- 外部 Agent 无法主动从 Oris 经验池中检索匹配的 Gene/Capsule
- 外部 Agent 无法将自己的经验（成功的问题解决方案）贡献给 Oris 经验池
- 缺乏统一的经验仓库访问协议（HTTP/gRPC）

## 2. 目标与成功标准

### 核心目标
构建一个 **Experience Repository Service**，作为 Oris 经验仓库的网络访问层，支持：
1. **查询（Fetch）**：外部 Agent 根据问题信号（signals）从仓库检索匹配的 Gene/Capsule
2. **贡献（Share）**：外部 Agent 将自己的成功经验发布到仓库，供其他 Agent 复用

### 成功指标
- 外部 Agent 能通过 HTTP API 查询匹配的 Gene（按 signals、tags、confidence 过滤）
- 外部 Agent 能通过 HTTP API 贡献新的 Gene 或 Capsule 到仓库
- 仓库支持按 confidence、use_count、quality_score 排序
- 仓库提供翻页和游标查询能力
- 集成 Oris 现有的签名验证机制，确保经验来源可信

## 3. 用户故事

### 用户故事 1：经验查询
> 作为一个外部 Agent，我希望能根据遇到的问题信号（如 "connection timeout", "memory leak"）从 Oris 经验仓库查询匹配的经验，以便快速复用已有的解决方案。

**验收标准**：
- [ ] Agent 发送 FetchQuery（包含 signals、min_confidence）
- [ ] 仓库返回匹配的 Gene/Capsule 列表，按 relevance_score 排序
- [ ] 支持分页（limit + cursor）

### 用户故事 2：经验贡献
> 作为一个外部 Agent，我希望能将自己的成功经验（Gene 或 Capsule）贡献给 Oris 经验仓库，以便其他 Agent 可以复用我的解决方案。

**验收标准**：
- [ ] Agent 发送 PublishRequest（包含 Gene/Capsule + sender_id）
- [ ] 仓库验证签名（可选）并存储
- [ ] 仓库返回发布确认

### 用户故事 3：经验评价反馈
> 作为一个使用过经验仓库中经验的 Agent，我希望能反馈该经验的使用结果（成功/失败），以便仓库调整 confidence 评分。

**验收标准**：
- [ ] Agent 发送 Feedback（包含 gene_id、success、quality_score）
- [ ] 仓库更新 Gene/Capsule 的 confidence 和 quality_score

## 4. 范围

### In Scope
- 新建 `oris-experience-repo` crate，提供 HTTP API 服务
- 实现 `ExperienceRepoServer`：HTTP 服务器（Axum）
- 实现 `ExperienceRepoClient`：供外部 Agent 调用的客户端库
- 复用 `oris-genestore` 的 SQLite 存储层
- 复用 `oris-evolution-network` 的 Envelope 协议（扩展 Fetch/Publish）
- 集成 Ed25519 签名验证（复用 `oris-evolution-network::signing`）
- 提供 OpenAPI 文档

### Out of Scope
- 前端界面（纯后端）
- 节点发现和 gossip 同步（已有 `oris-evolution-network`）
- 多仓库联邦同步
- 复杂的权限和计费系统

## 5. 系统边界与架构

```
┌─────────────────────────────────────────────────────────┐
│  External Agent                                         │
└────────────┬────────────────────────────────────────────┘
             │ HTTP/REST (Fetch/Publish/Feedback)
             ▼
┌─────────────────────────────────────────────────────────┐
│  oris-experience-repo: ExperienceRepoServer (Axum)    │
│  - Fetch API: GET /experience?q=signals&min_conf=0.5 │
│  - Publish API: POST /experience                       │
│  - Feedback API: POST /experience/{id}/feedback       │
└────────────┬────────────────────────────────────────────┘
             │
             ▼
┌─────────────────────────────────────────────────────────┐
│  oris-genestore: SqliteGeneStore                       │
│  - Gene、Capsule 存储                                  │
│  - 索引查询、confidence 衰减                            │
└─────────────────────────────────────────────────────────┘
```

### 关键数据流

1. **Fetch 流程**：
   - Agent → `ExperienceRepoServer::fetch_experiences(query)`
   - → `SqliteGeneStore::search_genes(query)`
   - → 返回匹配结果（按 relevance_score 排序）

2. **Publish 流程**：
   - Agent → `ExperienceRepoServer::publish_experience(envelope)`
   - → 验证签名（可选）
   - → `SqliteGeneStore::store_gene(capsule.gene_id, gene)`
   - → 返回发布确认

## 6. API 契约

### Fetch API
```
GET /experience?q={signals}&min_confidence={0.5}&limit={10}&cursor={optional}
```

Request Query:
- `q`: 逗号分隔的问题信号列表
- `min_confidence`: 最低置信度（默认 0.5）
- `limit`: 返回数量限制（默认 10）
- `cursor`: 分页游标

Response:
```json
{
  "assets": [
    {
      "gene": {
        "id": "uuid",
        "name": "...",
        "signals": ["timeout", "error"],
        "strategy": ["step1", "step2"],
        "confidence": 0.85,
        ...
      }
    }
  ],
  "next_cursor": "optional",
  "sync_audit": { ... }
}
```

### Publish API
```
POST /experience
Content-Type: application/json

{
  "sender_id": "agent-123",
  "assets": [
    {
      "gene": { ... }
    }
  ],
  "signature": "optional_ed25519_signature"
}
```

### Feedback API
```
POST /experience/{gene_id}/feedback
Content-Type: application/json

{
  "sender_id": "agent-456",
  "success": true,
  "quality_score": 0.9,
  "comment": "resolved my issue"
}
```

## 7. 关键约束

- **纯后端**：不涉及前端工程
- **技术栈**：Rust + Axum + SQLite
- **复用优先**：复用 `oris-genestore`、`oris-evolution-network` 的现有能力
- **签名验证**：默认启用 Ed25519 签名验证，可配置关闭
- **向后兼容**：不破坏现有的 local gene store 使用方式

## 8. 风险与待确认项

### 风险
1. **签名验证性能**：高并发下 Ed25519 验证可能成为瓶颈
   - 缓解：签名验证可选，默认关闭

2. **SQLite 并发写入**：多个 Agent 同时发布经验
   - 缓解：`oris-genestore` 已使用 WAL 模式

3. **经验质量控制**：恶意 Agent 贡献低质量经验
   - 缓解：引入 confidence 衰减机制，quality_score 持续更新

### 待确认项
- [ ] 是否需要支持多仓库联邦？（当前仅支持单一仓库）
- [ ] 签名验证是否默认为开启？
- [ ] 是否需要 Rate Limiting？
- [ ] Feedback 的 quality_score 如何影响 Gene 的 promotion？

## 9. 参与角色

| 角色 | 职责 |
|------|------|
| `tech-lead` | 方案评审、架构决策、放行 |
| `architect` | 系统设计、API 契约 |
| `backend-engineer` | 实现 ExperienceRepoServer、Client |
| `qa-engineer` | 测试计划、集成验证 |

## 10. 企业治理（按需）

本项目为 T4 级别原型探索，暂无集团组件约束、无敏感数据、无合规要求。

## 11. 依赖项

| 依赖 | 版本 | 用途 |
|------|------|------|
| `oris-genestore` | 0.2.0 | Gene/Capsule 存储 |
| `oris-evolution` | 0.4.1 | Gene、Capsule 类型定义 |
| `oris-evolution-network` | 0.5.0 | Envelope 协议、签名验证 |
| `axum` | latest | HTTP 服务器 |
| `rusqlite` | latest | SQLite 驱动（通过 genestore） |

## 12. 下一步

1. **需求挑战会**：确认 API 契约细节、签名策略
2. **方案设计**：输出 `arch-design.md`
3. **实现**：创建 `oris-experience-repo` crate
4. **测试**：单元测试 + 集成测试
