---
artifact: delivery-plan
task: experience-repo-phase2
date: 2026-04-14
role: project-manager
status: draft
---

# 交付计划 — 经验仓库二期 (Experience Repository Phase 2)

## 1. 版本目标

| 字段 | 内容 |
|------|------|
| **版本** | v0.2.0 |
| **范围** | Share 功能（OEN Envelope + Ed25519 + Key Service）|
| **放行标准** | 单元测试 + 集成测试通过；Share API 可用；Key Management API 可用；文档完成 |

### Scope

| 功能 | 优先级 | 说明 |
|------|--------|------|
| Key Service | P0 | API Key CRUD + 验证 |
| OEN Envelope 支持 | P0 | Envelope 解析 + Ed25519 验签 |
| Share API | P0 | POST /experience |
| Key Management API | P1 | 内部管理接口 |
| CLI 初始化工具 | P2 | 首次部署辅助 |

## 2. 工作拆解

### 阶段 1：Key Service 核心

| 工作项 | 主责 | 依赖 | 计划 |
|--------|------|------|------|
| 定义 Key 类型（ApiKey, KeyId, KeyStatus） | architect | - | Day 1 |
| 设计 KeyStore Schema | architect | Key 类型定义 | Day 1 |
| 实现 KeyStore（SQLite） | backend-engineer | Schema | Day 2 |
| 实现 KeyService（CRUD + 验证） | backend-engineer | KeyStore | Day 2-3 |
| API Key Middleware | backend-engineer | KeyService | Day 3 |

### 阶段 2：OEN Envelope 支持

| 工作项 | 主责 | 依赖 | 计划 |
|--------|------|------|------|
| 扩展 OenEnvelope 类型（添加 Publish） | architect | - | Day 1 |
| 实现 OenVerifier（Envelope 解析） | backend-engineer | Envelope 类型 | Day 3-4 |
| 实现 Ed25519 签名验证 | backend-engineer | OenVerifier | Day 4 |
| 签名缓存优化 | backend-engineer | Ed25519 验证 | Day 4（可选）|

### 阶段 3：Share API 实现

| 工作项 | 主责 | 依赖 | 计划 |
|--------|------|------|------|
| 定义 ShareRequest / ShareResponse | architect | OEN 类型 | Day 1 |
| 实现 ShareHandler | backend-engineer | OenVerifier, KeyService | Day 5 |
| 集成 GeneStore 存储 | backend-engineer | ShareHandler | Day 5 |
| Share API 端点注册 | backend-engineer | ShareHandler | Day 5 |

### 阶段 4：Key Management API

| 工作项 | 主责 | 依赖 | 计划 |
|--------|------|------|------|
| Key Management Handlers | backend-engineer | KeyService | Day 6 |
| API 端点注册 | backend-engineer | Handlers | Day 6 |

### 阶段 5：测试与验证

| 工作项 | 主责 | 依赖 | 计划 |
|--------|------|------|------|
| 单元测试 | qa-engineer | 核心实现 | Day 7 |
| 集成测试 | qa-engineer | Share API | Day 7-8 |
| 性能测试 | qa-engineer | 签名验证 | Day 8 |
| 文档（README + API） | backend-engineer | API 稳定 | Day 8 |

### 阶段 6：CLI 工具

| 工作项 | 主责 | 依赖 | 计划 |
|--------|------|------|------|
| CLI 初始化工具 | backend-engineer | Key Management API | Day 9 |
| 部署文档 | backend-engineer | CLI 工具 | Day 9 |

## 3. 风险与缓解

| 风险 | 影响 | 缓解措施 | Owner |
|------|------|----------|-------|
| Ed25519 验签性能瓶颈 | 高并发延迟 | 签名缓存（TTL 5分钟） | backend-engineer |
| Key 存储单点 | 可用性 | 与 GeneStore 共用 SQLite | architect |
| 首次部署复杂度 | 运维成本 | CLI 初始化工具 | backend-engineer |
| 时间同步问题 | 签名验证失败 | 放宽 timestamp 窗口（±5分钟） | architect |

## 4. 依赖项

| 依赖 | 版本 | 用途 |
|------|------|------|
| `oris-genestore` | 0.2.0 | Gene 存储 |
| `oris-evolution` | 0.4.1 | Gene 类型 |
| `oris-evolution-network` | 0.5.0 | OEN + signing |
| `oris-evo-ipc-protocol` | 0.1.0 | IPC 协议 |

## 5. 节点检查

| 节点 | 目标 | 阻塞项 |
|------|------|--------|
| 方案评审 | arch-design.md 评审通过 | Key Schema、OEN 类型定义 |
| Key Service 完成 | Key CRUD + 验证可用 | - |
| OEN 支持完成 | Envelope 解析 + 验签通过 | - |
| Share API 完成 | 端到端 Share 流程通 | - |
| 测试完成 | 80%+ 覆盖率 | 无 |
| 发布准备 | 文档、CLI、部署说明完成 | - |

## 6. 估算

| 阶段 | 估算工作量 |
|------|-----------|
| Key Service 核心 | 3 天 |
| OEN Envelope 支持 | 2 天 |
| Share API | 1 天 |
| Key Management API | 1 天 |
| 测试 | 2 天 |
| CLI 工具 | 1 天 |
| **总计** | **10 天** |

## 7. 后续待确认项

- [ ] Key TTL 默认值（建议 90 天）
- [ ] 是否需要 rate limiting per key
- [ ] Admin Key 的初始化方式（手动配置 vs 自动生成）
