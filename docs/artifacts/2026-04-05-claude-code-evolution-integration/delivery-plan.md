---
artifact: delivery-plan
task: claude-code-evolution-integration
date: 2026-04-05
role: tech-lead
status: plan-complete
---

# Delivery Plan — Oris 自我进化能力集成到 Claude Code

## 1. 版本目标

| 字段 | 内容 |
|------|------|
| 版本 | v0.1 (MVP) |
| 范围 | 核心进化 pipeline + Gene Pool + IPC 接口 |
| 放行标准 | 集成测试通过、本地 demo 可运行、基本安全验证 |

---

## 2. 工作拆解

### Phase 1: IPC 接口层（~1周）

| 工作项 | 主责 | 依赖 | 交付物 |
|--------|------|------|--------|
| IPC 协议设计 | architect | PRD | `arch-design.md` 接口定义 |
| Unix Domain Socket 服务端 | backend-engineer | 协议设计 | `oris-evo-ipc` crate |
| Claude Code IPC 客户端 | backend-engineer | 协议设计 | harness 插件 |
| 基础连接管理 | backend-engineer | 无 | 连接池、心跳 |

### Phase 2: 进化 Pipeline 集成（~2周）

| 工作项 | 主责 | 依赖 | 交付物 |
|--------|------|------|--------|
| Signal 检测接入 | backend-engineer | Phase 1 | Harness hook → IPC |
| Pipeline 初始化 | backend-engineer | Phase 1 | Server 端 pipeline |
| Mutation 评估接入 | backend-engineer | Pipeline | `MutationEvaluator` 集成 |
| Sandbox 执行接入 | backend-engineer | Pipeline | `oris-sandbox` 集成 |

### Phase 3: Gene Pool 与安全（~1周）

| 工作项 | 主责 | 依赖 | 交付物 |
|--------|------|------|--------|
| Gene Store 持久化 | backend-engineer | Phase 2 | SQLite Gene Pool |
| Ed25519 签名验证 | backend-engineer | Phase 2 | 签名层 |
| 来源标签记录 | backend-engineer | Phase 2 | SourceTag 记录 |
| 自动 Revert 机制 | backend-engineer | Phase 3 | Revert 逻辑 |

### Phase 4: CLI 与测试（~1周）

| 工作项 | 主责 | 依赖 | 交付物 |
|--------|------|------|--------|
| Gene Pool CLI | backend-engineer | Phase 3 | `evolution-cli` 工具 |
| 集成测试 | qa-engineer | Phase 1-3 | 测试用例 |
| 本地 Demo | backend-engineer | Phase 4 | 可运行示例 |

---

## 3. 角色分工

| 角色 | 主责 | 交付物 |
|------|------|--------|
| `tech-lead` | 架构评审、技术选型拍板 | 签署放行 |
| `architect` | IPC 协议、组件拆分 | `arch-design.md` |
| `backend-engineer` | 核心实现 | IPC 层 + Pipeline + Gene Store |
| `qa-engineer` | 测试策略 | 集成测试、E2E 场景 |
| `devops-engineer` | 构建打包 | 产物发布配置 |

---

## 4. 风险与缓解

| 风险 | 影响 | 缓解措施 | Owner |
|------|------|----------|-------|
| IPC 延迟影响体验 | 任务变慢 | 异步执行，结果后台返回 | backend |
| 沙箱执行超时 | 进化失败 | 超时控制 + 重试 | backend |
| Gene Pool 膨胀 | 检索慢 | 容量限制 + LRU 淘汰 | backend |
| 签名验证复杂 | 开发成本高 | 使用现有 Ed25519 实现 | backend |
| 误进化污染 | 知识库质量下降 | 高阈值 + 自动 revert | backend |

---

## 5. 节点检查

| 节点 | 目标日期 | 评审内容 |
|------|----------|----------|
| 方案评审 | +3天 | IPC 协议 + 组件拆分 |
| 开发完成 | +3周 | 全部代码 + 单元测试 |
| 测试完成 | +4周 | 集成测试通过 |
| 发布准备 | +5周 | 产物打包 + 文档 |

---

## 6. 技能装配清单

| 类别 | 技能 | 用途 |
|------|------|------|
| shared | `rust-patterns` | 核心实现模式 |
| shared | `rust-testing` | 测试策略 |
| ecc | `rust-build` | 构建问题排查 |
| ecc | `rust-review` | 代码评审 |
| company | - | 本任务不涉及 |

---

## 7. ADR 需求

| ADR | 标题 | 触发原因 |
|-----|------|----------|
| ADR-001 | IPC vs FFI vs WASM 选择 | 核心架构决策 |
| ADR-002 | SQLite Gene Store 方案 | 数据持久化选型 |

---

## 8. 应用等级（非企业内部应用）

本任务为工具集成，不涉及企业数据风险，无需应用等级评估。
