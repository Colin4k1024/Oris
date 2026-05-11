# Backlog

> 真相源：跨任务遗留项、技术债和下一阶段候选统一在此维护。

## 快照信息

| 字段 | 值 |
|------|----|
| 最近更新 | 2026-05-11 |
| 更新角色 | tech-lead |
| 来源任务 | exp-repo-v040-hardening |

---

## 未完成项 / 遗留项

| 优先级 | 事项 | 来源 | 触发条件 | 建议阶段 |
|--------|------|------|----------|----------|
| P0 | **[R1] `publish_envelope` 空签名问题** — `EvolutionEnvelope::publish()` 产生 `signature: None`，`unwrap_or_default()` 发送空字符串，OEN 服务端 Ed25519 验证必然失败；PKI 节点启用 `with_network_publisher()` 前必须修复 | team-review | v0.4.0 前 / 生产 PKI push 开放前 | v0.4.0 |
| P0 | **[R2] sender_id 语义错误** — `maybe_push_to_network` 用 `gene.id` 作为网络层发送者标识，违反 OEN 协议 identity 语义，服务端身份绑定验证会拒绝 | team-review | v0.4.0 前 | v0.4.0 |
| P0 | **[R6] Key 管理端点缺少 `verify_key` 鉴权（预存在 CRITICAL）** — `POST /keys`, `GET /keys`, `DELETE /keys/:id`, `POST /keys/:id/rotate` 均未调用 `verify_key`，任意人可操作 API Key | team-review（预存在）| 立即 | 下一迭代（独立 issue）|
| P1 | **[R3] 三处 GenePromoted 路径未调用 `maybe_push_to_network`** — replay、built-in bootstrap、trusted local report 的晋升事件不触发 push，与 doc comment 不符 | team-review | v0.4.0 | v0.4.0 |
| P1 | **[R4] ALTER TABLE 错误静默** — `let _` 丢弃全部错误（含 SQLITE_READONLY/LOCKED），真实失败隐蔽为后续"列不存在"错误 | team-review | v0.4.0 | v0.4.0 |
| P1 | **[R7] `ClientConfig` Debug 暴露 api_key** — `#[derive(Debug)]` 会将 api_key 输出到日志 | team-review（预存在）| v0.4.0 | v0.4.0 |
| P1 | **[R8] reqwest 无超时** — `Client::new()` 无超时配置，推送时无响应服务端会无限阻塞 | team-review（预存在）| v0.4.0 | v0.4.0 |
| P1 | workspace sibling crate path dep 发版策略文档化 | exp-repo-evokernel-wire | 下次 crate 发版前 | 发版准备 |
| P2 | E2E 集成测试：EvoKernel 晋升 → NetworkPublisher → HTTP push → experience-repo `GET /experience` 可查 | exp-repo-evokernel-wire | 有可用 HTTP 测试环境 | 下一迭代 |
| P2 | **[R5] `contributor_id = Some(...)` GeneStore 持久化正向验证缺失** — 仅覆盖了 None 路径 | team-review | v0.4.0 | v0.4.0 |

---

## 技术债

| 优先级 | 事项 | 影响 |
|--------|------|------|
| P2 | `oris-genestore` bundled SQLite 版本较旧（libsqlite3-sys 0.30.1），不支持 `IF NOT EXISTS` on ALTER TABLE | 限制可用的 SQLite 现代语法，当前已绕过 |
| P2 | `maybe_push_to_network` 缺少 tracing-subscriber 断言测试 — node_id=None warn! 路径无自动验证 | 来源：exp-repo-v040-hardening |
| P2 | ORT/fastembed `--all-features` build 失败（E0599 fn pointer unwrap_or_else）— pre-existing | 影响全量 build，不影响演化相关 crate |

---

## 下一阶段候选

- **oris-experience-repo v0.4.0**：E2E 集成测试 + 发版策略文档 + patch 版本发布
- **EvoKernel 节点身份**：引入节点级别 ID，修复 `sender_id` 语义问题
