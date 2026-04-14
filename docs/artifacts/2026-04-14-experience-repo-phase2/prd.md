---
artifact: prd
task: experience-repo-phase2
date: 2026-04-14
role: tech-lead
status: draft
---

# 经验仓库二期 (Experience Repository Phase 2) - PRD

## 1. 背景与问题

经验仓库 MVP（v0.1.0）已完成 Fetch 只读查询功能。二期目标：**启用 Share 功能**，允许外部 Agent 将经验贡献到仓库。

**P0 阻断项已解决**：经过 2026-04-14 需求挑战会，决定采用**完整方案**（方案 B）：
- OEN Envelope + Ed25519 签名验证
- Key Service（API Key 生成、存储、验证、轮换、撤销）

## 2. 目标与成功标准

### 核心目标
实现 **Share 功能**，使外部 Agent 能够：
1. 通过 OEN Envelope 发布经验（含 Ed25519 签名）
2. 通过 Key Service 验证 API Key 和 Agent 身份
3. 贡献的基因经验可被其他 Agent 查询和复用

### 成功指标
- 外部 Agent 能通过签名验证成功发布 Gene/Capsule 到仓库
- Key Service 提供完整的 Key 管理能力（CRUD + 轮换）
- 仓库正确拒绝无效签名或过期 Key 的请求
- Share 后的 Gene 可通过 Fetch API 被查询到

## 3. 用户故事

### 用户故事 1：经验贡献（Share）
> 作为一个外部 Agent，我希望能将自己的成功经验（Gene 或 Capsule）通过 OEN Envelope 签名后贡献给 Oris 经验仓库，以便其他 Agent 可以复用我的解决方案。

**验收标准**：
- [ ] Agent 构造 OEN Envelope（包含 Gene + Ed25519 签名）
- [ ] ExperienceRepoServer 验证 Envelope 签名
- [ ] Key Service 验证 API Key 有效性
- [ ] 验证通过后，Gene 存储到 SqliteGeneStore
- [ ] 返回发布确认（含 gene_id）

### 用户故事 2：Key 管理
> 作为一个 Oris 运维人员，我希望能管理 API Keys（创建、查看、撤销），以确保只有授权的 Agent 才能访问经验仓库。

**验收标准**：
- [ ] 运维人员可以创建新的 API Key（关联 agent_id）
- [ ] 运维人员可以查看所有 API Key 状态
- [ ] 运维人员可以撤销（revoke）无效或泄露的 Key
- [ ] Key 支持轮换（rotation）

### 用户故事 3：经验查询（增强）
> 作为一个外部 Agent，我已经贡献了经验，现在希望能查询到我贡献的经验以及他人的经验。

**验收标准**：
- [ ] Fetch API 返回所有已验证的经验（不区分贡献者）
- [ ] 响应中包含贡献者信息（sender_id）

## 4. 范围

### In Scope
- **Key Service**：API Key 管理服务（CRUD + 轮换）
- **OEN Envelope 支持**：解析和验证 OEN Envelope
- **Ed25519 签名验证**：复用 oris-evolution-network 的 signing 模块
- **Share API**：POST /experience 端点
- **Key Management API**：Key 的管理接口（内部使用）

### Out of Scope
- **Feedback 功能**：延后至三期
- **向量语义搜索**：延后至四期
- **多仓库联邦同步**：已有 oris-evolution-network 处理
- **复杂权限和计费系统**：当前版本仅做身份验证

## 5. 系统边界与架构

```
┌─────────────────────────────────────────────────────────────┐
│  External Agent                                              │
│  - 持有 API Key + Ed25519 私钥                             │
└─────────────┬───────────────────────────────────────────────┘
              │ OEN Envelope（签名）
              ▼
┌─────────────────────────────────────────────────────────────┐
│  oris-experience-repo: ExperienceRepoServer                 │
│                                                              │
│  ┌─────────────┐    ┌──────────────┐    ┌───────────────┐ │
│  │ Key Service  │    │ OEN Verifier │    │ Share Handler │ │
│  │ (API Key)    │    │ (Ed25519)    │    │               │ │
│  └─────────────┘    └──────────────┘    └───────────────┘ │
└─────────────┬───────────────────────────────────────────────┘
              │
              ▼
┌─────────────────────────────────────────────────────────────┐
│  oris-genestore: SqliteGeneStore                            │
│  - Gene、Capsule 持久化                                      │
└─────────────────────────────────────────────────────────────┘
```

### 关键数据流

1. **Share 流程**：
   - Agent → 构造 OEN Envelope（包含 Gene + Ed25519 签名）
   - → ExperienceRepoServer::share_experience(envelope)
   - → Key Service 验证 API Key
   - → OEN Verifier 验证 Ed25519 签名
   - → SqliteGeneStore::store_gene(gene)
   - → 返回发布确认

2. **Key 管理流程**：
   - 运维人员 → KeyService::create_key(agent_id)
   - → 生成新 Key，存储到 KeyStore
   - → 返回 Key（仅显示一次）

## 6. API 契约

### Share API
```
POST /experience
Content-Type: application/json
X-Api-Key: {api_key}

{
  "envelope": {
    "sender_id": "agent-123",
    "message_type": "Publish",
    "payload": {
      "gene": { ... }
    },
    "signature": "base64_ed25519_signature",
    "timestamp": "2026-04-14T00:00:00Z"
  }
}
```

### Key Management API（内部）
```
POST /keys              # 创建 Key
GET    /keys            # 列出所有 Key
DELETE /keys/{key_id}   # 撤销 Key
POST   /keys/{key_id}/rotate  # 轮换 Key
```

## 7. 关键约束

- **纯后端**：不涉及前端工程
- **技术栈**：Rust + Axum + SQLite + oris-evolution-network
- **签名验证**：必须通过 Ed25519 签名验证才允许 Share
- **Key 存储**：SQLite 表（独立于 Gene 表）
- **向后兼容**：不破坏现有的 Fetch API

## 8. 风险与待确认项

### 风险
1. **Ed25519 签名验证性能**：高并发下可能成为瓶颈
   - 缓解：签名验证结果可缓存（TTL 5分钟）

2. **Key Service 单点故障**：如果 Key Service 不可用，所有请求失败
   - 缓解：KeyStore 使用 SQLite，与 GeneStore 同一 DB

3. **首次部署复杂度**：需要同时部署 ExperienceRepoServer 和初始化 Key
   - 缓解：提供 CLI 工具初始化第一个 Key

### 待确认项
- [x] 签名验证方案：OEN Envelope + Ed25519（已确认）
- [x] Key 存储方案：SQLite 表（已确认）
- [ ] Key 的 TTL策略：是否需要过期机制？
- [ ] 是否需要 Key 配额（rate limiting per key）？

## 9. 参与角色

| 角色 | 职责 |
|------|------|
| `tech-lead` | 方案评审、架构决策、放行 |
| `architect` | 系统设计、OEN Envelope 定义 |
| `backend-engineer` | 实现 Key Service、Share API |
| `qa-engineer` | 测试计划、集成验证 |

## 10. 企业治理

本项目为 T4 级别原型探索，暂无集团组件约束、无敏感数据、无合规要求。

## 11. 依赖项

| 依赖 | 版本 | 用途 |
|------|------|------|
| `oris-genestore` | 0.2.0 | Gene/Capsule 存储 |
| `oris-evolution` | 0.4.1 | Gene、Capsule 类型定义 |
| `oris-evolution-network` | 0.5.0 | OEN Envelope、Ed25519 签名验证 |
| `oris-evo-ipc-protocol` | 0.1.0 | IPC 协议定义 |

## 12. 下一步

1. **方案设计**：输出 `arch-design.md`
2. **实现**：Key Service → OEN Verifier → Share Handler
3. **测试**：签名验证测试、Key CRUD 测试、集成测试
4. **部署**：CLI 初始化工具、配置说明
