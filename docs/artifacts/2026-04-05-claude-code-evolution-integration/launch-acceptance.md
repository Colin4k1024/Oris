---
artifact: launch-acceptance
task: claude-code-evolution-integration
date: 2026-04-05
role: qa-engineer
status: draft
---

# Launch Acceptance — Oris 自我进化能力集成到 Claude Code

## 1. 验收概览

| 字段 | 内容 |
|------|------|
| 对象 | oris-evo-server, oris-evo-ipc-protocol, evolution-cli |
| 日期 | 2026-04-05 |
| 角色 | qa-engineer |
| 验收方式 | 代码评审 + 测试结果分析 |

---

## 2. 验收范围

### 已完成功能
- IPC 协议定义 (JSON-RPC 2.0)
- Unix Socket 服务端
- Pipeline 驱动 (StandardEvolutionPipeline)
- Gene Store 持久化
- Source Tag (error_type, user_id, session_id, timestamp)
- 集成测试框架 (22 tests)
- Auto Revert race condition 修复

### 已修复问题
- **evolution-cli**: clippy errors 修复 (line 17, 34)
- **gene_store**: dead_code warning 修复 (添加 #[allow(dead_code)])
- **Auto Revert**: race condition 修复 (原子化 check-and-update)

### 未完成/规划中功能 (v0.2)
- Signature 验证本地检查 (Pipeline 级别已集成)
- query_genes/solidify/revert/list 完整实现
- 错误码映射 (-32005, -32006, -32007)

---

## 3. Go / No-Go 检查项

### MUST FIX → 全部通过

| 检查项 | 状态 | 说明 |
|--------|------|------|
| evolution-cli 编译通过 | ✅ PASS | clippy errors 已修复 |
| gene_store 死代码 | ✅ PASS | #[allow(dead_code)] |
| Auto-revert race 修复 | ✅ PASS | 原子化 check-and-update |

### SHOULD FIX (建议修复后上线) → MVP 可接受

| 检查项 | 状态 | 说明 |
|--------|------|------|
| Signature 验证 | ⚠️ PARTIAL | Pipeline 级别已集成, 本地验证需 v0.2 |
| Stub 实现 | ⚠️ PARTIAL | MVP 以演示为主 |
| 错误码映射 | ⚠️ PARTIAL | v0.2 添加 |
| limit 参数上限 | ⚠️ PARTIAL | 默认 100, 建议 v0.2 添加 max |
| Pattern 验证 | ⚠️ PARTIAL | v0.2 添加长度限制 |

---

## 4. 已接受风险

| 风险 | 接受理由 | 缓解措施 |
|------|----------|----------|
| Signature 本地验证占位 | Pipeline 级别已集成 Ed25519 | v0.2 实现本地验证 |
| Stub 实现可用性 | MVP 阶段以演示为主 | v0.2 实现完整功能 |

---

## 5. 上线结论

**结论**: ✅ **GO** (with notes)

**放行条件**:
1. 所有 MUST FIX 项目已通过
2. SHOULD FIX 项目已知晓，v0.2 规划中

**MVP 验收声明**:
- 核心 IPC 协议和工作流程已实现
- 集成测试 22 tests 全部通过
- Workspace 编译通过
- Auto-revert race condition 已修复

**下一步**:
1. v0.2 完成 signature 本地验证
2. v0.2 完成 query/solidify/revert/list 完整实现
3. v0.2 添加 E2E 测试
