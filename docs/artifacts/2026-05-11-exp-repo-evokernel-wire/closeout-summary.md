---
artifact: closeout-summary
task: exp-repo-evokernel-wire
date: 2026-05-11
role: tech-lead
status: final
state: follow-up-required
---

# Closeout Summary — 经验仓库 × EvoKernel 集成胶水修复

## 收口对象

| 字段 | 值 |
|------|----|
| 关联任务 | exp-repo-evokernel-wire |
| 关联版本 | oris-experience-repo 0.3.0（patch），oris-genestore 0.2.0（patch），oris-evolution-network 0.5.0（patch），oris-evokernel 0.14.1（patch）|
| 观察窗口 | 2026-05-11（当日完成，无独立部署窗口，OSS 本地测试验证） |
| 收口角色 | tech-lead |

---

## 最终验收状态

**follow-up-required** — sprint 范围（R1–R8）全部交付，579 tests 通过，但 team-review 阶段识别出 3 个 P0 blocking 问题和 4 个 P1 问题，需在 v0.4.0 前解决方可开放生产 PKI push 路径。

| 成功指标 | 状态 | 证据 |
|---------|------|------|
| `share_experience()` 客户端方法存在且可调用 | ✅ | `client.rs` 实现 + 单元测试通过 |
| EvoKernel 晋升后触发 `NetworkPublisher`（DIP 抽象） | ✅ | `core.rs` `maybe_push_to_network` call-site |
| `GET /experience` 返回非空 `contributor_id` | ✅ | `handlers.rs` 从 gene 对象取值 |
| 测试全量绿灯 | ✅ | 579 tests passed, 0 failures |
| **P0：`publish_envelope` 产生有效 Ed25519 签名** | ❌ | `signature: None`，`unwrap_or_default()` 发送空串，服务端验证必然拒绝 |
| **P0：`sender_id` 使用节点 ID 而非 gene.id** | ❌ | 当前用 `gene.id` 违反 OEN identity 语义 |
| **P0：Key 管理端点鉴权** | ❌ | `POST/GET/DELETE /keys` 端点未调用 `verify_key`（pre-existing CRITICAL） |

---

## 观察窗口结论

无独立服务部署，无运行时事故，无回滚动作。观察窗口内 CI 保持绿灯。

PKI push 路径（`with_network_publisher` + Ed25519 OEN）**不应在 P0 问题修复前对外启用**，当前 `network_publisher: None` 是安全默认值。

---

## 残余风险处置

| 事项 | 等级 | 处置决策 | 责任人 | 下一步 |
|------|------|----------|--------|--------|
| `publish_envelope` 空签名 | P0 | 延后处理，v0.4.0 前必须修复 | backend-engineer | 打开独立 issue |
| `sender_id` 语义错误（gene.id 非节点 ID） | P0 | 延后处理，v0.4.0 前必须修复 | backend-engineer | 依赖节点身份 feature |
| Key 管理端点缺少 `verify_key` 鉴权 | P0 | 立即开独立 issue（pre-existing CRITICAL） | backend-engineer | 独立 issue，下一迭代 |
| 3 处晋升路径未调用 `maybe_push_to_network` | P1 | 延后处理，v0.4.0 | backend-engineer | 同 P0 issue |
| `ClientConfig` Debug 暴露 api_key | P1 | 延后处理，v0.4.0 | backend-engineer | |
| reqwest 无超时配置 | P1 | 延后处理，v0.4.0 | backend-engineer | |
| ALTER TABLE 错误静默（`let _` 丢弃全部错误） | P1 | 延后处理，v0.4.0 | backend-engineer | |
| `contributor_id = Some(...)` 正向验证缺失 | P2 | 延后处理，v0.4.0 | backend-engineer | |
| E2E 集成测试（晋升 → push → 仓库可查） | P2 | 下一迭代有 HTTP 测试环境时处理 | qa-engineer | |
| workspace sibling path dep 文档化 | P1 | 下次发版前处理 | devops-engineer | |

---

## Backlog 回写

已同步到 `docs/memory/backlog.md`，含以下事项：

- [P0] `publish_envelope` 空签名修复（v0.4.0 前必须）
- [P0] `sender_id` 语义修复，引入 EvoKernel 节点身份（v0.4.0 前）
- [P0] Key 管理端点 `verify_key` 鉴权补全（独立 issue，立即）
- [P1] 3 处 GenePromoted 路径补充 `maybe_push_to_network` 调用
- [P1] `ClientConfig` Debug 脱敏
- [P1] reqwest 客户端超时配置
- [P1] ALTER TABLE 错误处理精细化
- [P2] E2E 集成测试
- [P2] `contributor_id` 持久化正向验证

---

## 知识沉淀

### Lesson 1：Cargo workspace 中 path dep 与 registry dep 不互通

workspace 内 sibling crate 必须用 `{ path = "..." }` 引用，否则新增符号对其不可见，且 `cargo check` 会以 LSP 误报迷惑判断。

### Lesson 2：SQLite `ALTER TABLE ADD COLUMN IF NOT EXISTS` 版本门槛

bundled SQLite（libsqlite3-sys 0.30.1）不支持该语法；幂等迁移统一用 `let _ = conn.execute(...)` 忽略 "duplicate column name" 错误。

### Lesson 3：async 传播需要全量 call-site 审查

将 widely-used struct 方法升格为 `async fn` 前，先 `grep -rn` 枚举所有调用点。分为"已在 async 上下文"和"sync 上下文需递归升格"两类，建议在单独 commit 完成以便追溯。

### Lesson 4：HTTP 鉴权 handler 测试需 bootstrap fixture

引入 API Key 鉴权后，测试文件应提供 `create_test_state_with_key()` + `create_authed_headers()` 两个 fixture；bootstrap key 通过直接调用底层 `KeyStore::create_key()` 插入，不走 HTTP 层。

---

## 任务关闭结论

**follow-up-required** — sprint 目标（R1–R8 集成胶水）已全部交付并通过测试。PKI 网络推送路径受 3 个 P0 问题阻塞，**不得在修复前对外开放**。P0 事项已进入 backlog，计划 v0.4.0 处理。sprint 本身正式关闭，后续由 v0.4.0 sprint 跟踪。
