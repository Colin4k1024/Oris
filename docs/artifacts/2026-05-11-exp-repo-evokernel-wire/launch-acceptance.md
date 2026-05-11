---
artifact: launch-acceptance
task: exp-repo-evokernel-wire
date: 2026-05-11
role: qa-engineer
status: completed
state: accepted
---

# Launch Acceptance — 经验仓库 × EvoKernel 集成胶水修复

## 验收概览

| 字段 | 值 |
|------|----|
| 验收对象 | exp-repo-evokernel-wire（7 个集成 task） |
| 验收时间 | 2026-05-11 |
| 验收角色 | qa-engineer（评审）/ tech-lead（放行确认）|
| 验收方式 | 代码评审 + 自动化测试（247 passed）+ 并行安全审计 |

---

## 验收范围

### 业务范围（In）

- `ExperienceRepoClient::share_experience()` 客户端写入方法 ✅
- `NetworkPublisher` DIP trait 抽象 ✅
- `ExperienceRepoClient impl NetworkPublisher` ✅
- `EvoKernel::with_network_publisher()` 可选注入 + 晋升后 best-effort push ✅
- `GeneStore::contributor_id` 字段 + 幂等 schema 迁移 ✅
- `GET /experience` contributor_id 从 gene 元数据填充 ✅

### 不在验收范围（Out）

- E2E 集成测试（晋升 → HTTP push → 仓库可查）
- 生产 PKI 签名完整 push 路径（待 R1/R2 修复后）
- 预存在安全缺口的修复（R6 Key 管理鉴权，R7 api_key 日志，R8 reqwest 超时）

---

## 验收证据

| 证据类型 | 内容 | 状态 |
|----------|------|------|
| 单元测试 | 247 passed, 0 failed（3 crates，10 suites）| ✅ |
| code-reviewer | 3 HIGH + 4 MEDIUM + 2 LOW（详见 test-plan.md）| ✅ 已分类 |
| security-reviewer | 1 CRITICAL(预存在) + 3 HIGH(1新+2预存在) + 4 MEDIUM | ✅ 已分类 |
| cargo fmt | 格式一致 | ✅ |
| cargo check | 0 error, 0 warning（受影响 3 crates）| ✅ |

---

## 风险判断

### 已满足项

| 验收标准 | 证据 |
|---------|------|
| `share_experience()` 存在且可调用 | `client.rs` + 2 条 mockito 单测 |
| EvoKernel 晋升后 best-effort push（失败不阻断）| `maybe_push_to_network` + warn 日志设计 |
| `contributor_id` 从 GeneStore 填充 | `handlers.rs` line 163 |
| 全量回归测试通过 | 247/247 |
| 现有数据库向后兼容（幂等迁移）| `let _ = conn.execute(ALTER TABLE...)` |

### 本次可接受风险

| 风险 | 理由 | Owner | 目标迭代 |
|------|------|-------|----------|
| R1 — 空签名 push 失败 | best-effort side-effect，不影响晋升；已有 warn log；OSS 项目当前无生产 PKI 节点 | backend-engineer | v0.4.0 |
| R2 — sender_id 语义错误 | 同上；需 EvoKernel 节点身份 feature 前置 | backend-engineer | 视节点身份排期 |
| R3 — 三处 GenePromoted 路径未推送 | 热路径晋升（主要路径）覆盖；replay/bootstrap 路径 push 非 P0 | backend-engineer | v0.4.0 |
| R4 — ALTER TABLE 错误静默 | 影响范围有限；仅 SQLITE_READONLY 等极端情况；已在 backlog | backend-engineer | v0.4.0 |
| R5 — contributor_id Some(...) 无测试 | None 路径已覆盖；功能已实现；补测低风险 | backend-engineer | v0.4.0 |

### 阻塞项（须在 R1/R2 修复前不开放生产 PKI push）

| 阻塞 | 影响 | 解除条件 |
|------|------|----------|
| R1 空签名 | NetworkPublisher push 在 PKI 节点上必然失败 | `publish_envelope` 调用前完成 Ed25519 签名 |
| R2 sender_id 语义 | OEN 协议身份验证拒绝 | EvoKernel 持有节点 ID 并传入 envelope |
| R6 Key 管理端点无鉴权（预存在）| 任意人可生成/失效/轮换 API Key | 独立 issue 修复，与本次变更解耦 |

---

## 上线结论

> **条件 Go — 允许合并到 main，但生产 PKI push 能力须待 v0.4.0 修复后才可开放。**

**判断依据：**

1. **晋升主路径安全**：7 项集成 task 的核心功能（客户端写入 SDK、GeneStore contributor_id 溯源、handler 填充）均已实现并测试通过，不引入晋升路径回归。

2. **Push 路径是 best-effort**：`maybe_push_to_network` 失败仅 warn，不 panic、不回滚晋升。OSS 项目当前无生产 PKI 注册节点，R1/R2 的实际影响为"push 静默失败"，不影响基本功能。

3. **预存在安全问题已登记**：R6（Key 管理无鉴权）是预存在 CRITICAL，与本次 diff 无关，已独立登记为高优先 issue。

4. **限制条件（强制）**：
   - 任何注册了 Ed25519 公钥的生产节点启用 `with_network_publisher()` 之前，**必须先完成 R1（签名集成）修复**。
   - R6 Key 管理端点鉴权问题须在下一迭代 P0 处理。

---

**放行人：** qa-engineer / 最终放行确认：tech-lead

**放行时间：** 2026-05-11

**观察重点（上线后）：** `network push failed after promotion` warn 日志计数、GeneStore contributor_id 字段出现 Some 值的时机。
