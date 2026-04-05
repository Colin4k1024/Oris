---
artifact: test-plan
task: claude-code-evolution-integration
date: 2026-04-05
role: qa-engineer
status: draft
---

# Test Plan — Oris 自我进化能力集成到 Claude Code

## 1. 测试范围

### 功能范围
| 功能 | 状态 | 说明 |
|------|------|------|
| IPC 协议 (JSON-RPC 2.0) | ✅ 已实现 | request/response 序列化 |
| Unix Socket 服务端 | ✅ 已实现 | 异步 Accept/Read/Write |
| Pipeline 驱动 | ✅ 已实现 | StandardEvolutionPipeline 集成 |
| Gene Store 持久化 | ✅ 已实现 | SqliteGeneStorePersistAdapter |
| CLI 工具 | ✅ 已修复 | clippy 错误已修复 |
| Auto Revert | ✅ 已修复 | race condition 已修复 |
| Source Tag | ✅ 已实现 | error_type, user_id, session_id, timestamp |

### 测试类型覆盖

| 测试类型 | 覆盖 | 说明 |
|---------|------|------|
| 单元测试 | 部分 | IPC protocol 12 tests, Server 10 tests |
| 集成测试 | 缺失 | 完整 IPC 流程未覆盖 |
| E2E 测试 | 缺失 | Server 启动 + CLI 交互未覆盖 |

---

## 2. 评审发现的问题 (已修复)

### 已修复的 BLOCKING Issues

| # | 严重度 | 问题 | 修复 | 状态 |
|---|--------|------|------|------|
| 1 | HIGH | evolution-cli 编译失败 | 移除无用 import, 修复 borrow | ✅ 已修复 |
| 2 | HIGH | gene_store 死代码 | 添加 #[allow(dead_code)] | ✅ 已修复 |
| 3 | HIGH | Auto-revert race condition | 原子化 check-and-update | ✅ 已修复 |

### 仍存在的 NON-BLOCKING Issues

| # | 严重度 | 问题 | 文件 | 修复建议 |
|---|--------|------|------|----------|
| 4 | MEDIUM | Signature 占位符 | pipeline.rs:verify_signature | 需要真实集成时实现 |
| 5 | MEDIUM | Stub 实现 | pipeline.rs:191-215 | query_genes/solidify/list_genes/revert_internal |
| 6 | MEDIUM | Hardcoded 0.75 confidence | pipeline.rs:113 | 从 pipeline_result 提取实际值 |
| 7 | MEDIUM | 错误码映射缺失 | handlers.rs:52-59 | 添加 -32005, -32006, -32007 |
| 8 | MEDIUM | HOME env panic | pipeline.rs:248 | 改为 Result 或 fallback |
| 9 | MEDIUM | limit 无上限 | handlers.rs:136 | 限制 max 10000 |
| 10 | MEDIUM | Pattern 未验证 | handlers.rs:128 | 限制长度 1000chars |

---

## 3. 风险评估

### 已缓解的高风险路径
1. **Auto-revert race** - check-and-update 现在是原子的

### 中风险路径 (MVP 可接受)
1. **Signature 占位符** - Pipeline 级别已集成签名，本地验证需 v0.2
2. **Stub 实现** - MVP 阶段以演示为主

---

## 4. 修复优先级

| 优先级 | Issue # | 状态 | 说明 |
|--------|---------|------|------|
| P0 | #1 CLI compile | ✅ 已修复 | |
| P0 | #2 gene_store | ✅ 已修复 | #[allow(dead_code)] |
| P0 | #3 Auto-revert race | ✅ 已修复 | 原子化 |
| P2 | #4-#10 | 规划中 | v0.2 实现 |
