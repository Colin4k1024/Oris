---
artifact: prd
task: exp-repo-v040-hardening
date: 2026-05-11
role: tech-lead
status: draft
state: intake
---

# PRD — Experience Repo v0.4.0 Hardening + Homepage

## 背景

上一个 sprint（exp-repo-evokernel-wire）完成了 R1–R8 集成胶水，team-review 后发现 3 个 P0 阻塞问题
和若干 P1 问题，PKI 网络推送路径当前不可用。同时用户提出新需求：为 oris-experience-repo 添加 homepage
页面，方便在浏览器直接了解服务状态和 API 说明。

### 代码现状审计（intake 时已验证）

| Backlog 事项 | 代码现状 | 结论 |
|-------------|----------|------|
| Key 管理端点缺 verify_key 鉴权 | handlers.rs 中 list_keys/create_key/revoke_key/rotate_key 均已调用 verify_key | ✅ 已解决，无需处理 |
| ClientConfig Debug 暴露 api_key | client.rs 已有自定义 `impl fmt::Debug` 输出 `***REDACTED***` | ✅ 已解决，无需处理 |
| reqwest 无超时配置 | client.rs 已配置 `.timeout(10s)` + `.connect_timeout(5s)` | ✅ 已解决，无需处理 |
| ALTER TABLE 错误静默 | store.rs 已精细化：仅忽略 "duplicate column name"，其他错误继续传播 | ✅ 已解决，无需处理 |
| publish_envelope 空签名 | maybe_push_to_network 创建未签名 envelope，sign_envelope() 未调用 | ❌ 待修复 |
| sender_id 使用 gene.id | 当前 fallback 为 "oris-node-unknown"（非 gene.id），但仍会发送无效 sender_id | ❌ 待修复（策略改为 skip）|
| 3 处 GenePromoted 路径未推送 | ensure_builtin_experience_assets (bootstrap) + record_reported_experience (trusted local) 缺 push | ❌ 待修复（2 处，非 3 处）|

## 目标与成功标准

### 业务目标

1. PKI 网络推送路径完全可用（签名有效，sender_id 语义正确）
2. bootstrap 和 trusted local 晋升事件触发网络推送
3. oris-experience-repo 服务提供人类可读的 homepage

### 成功指标

| 指标 | 验收标准 |
|------|----------|
| Ed25519 签名完整 | `maybe_push_to_network` 使用 NodeKeypair sign_envelope，服务端可验证 |
| sender_id 语义正确 | node_id 未配置时跳过推送（而非发送 "oris-node-unknown"）|
| bootstrap/trusted 路径覆盖 | ensure_builtin + record_reported 两处促进后触发推送 |
| Homepage 可访问 | GET / 返回 200 HTML，包含：服务名称、版本、状态、API 摘要 |
| 全量测试绿灯 | cargo test 无新 failures |

## 用户故事

### US-1：运维工程师查看服务状态
> 作为运维工程师，我希望打开浏览器访问 experience-repo 根路径，可以立即看到服务状态、版本号和 API 端点摘要，而不需要查阅代码或文档。

**验收标准**：
- GET / 返回 200，Content-Type: text/html
- 页面包含服务名称（"Oris Experience Repository"）、版本号、健康状态
- 页面列出主要 API 端点（GET/POST /experience, /keys, /public-keys, /health）
- 页面风格简洁，无外部 CDN 依赖（纯 HTML + inline CSS）

### US-2：PKI 节点配置 Ed25519 签名后推送有效 envelope
> 作为 PKI 节点运维，我希望配置 `with_signing_keypair()` 后，每次晋升推送到 experience-repo 的 envelope 都包含有效 Ed25519 签名，服务端可以验证通过。

**验收标准**：
- `with_signing_keypair(keypair)` builder 方法存在于 `EvolutionNetworkNode` 和 `NetworkNodeConfig`
- `maybe_push_to_network` 在 keypair 存在时调用 `sign_envelope()`
- 单元测试验证签名 envelope 可被 `verify_envelope()` 验证通过

### US-3：未配置 node_id 时不发送无效 sender_id
> 作为系统，当 node_id 未配置时，network push 应当跳过并记录 warn 日志，而不是发送 "oris-node-unknown" 作为虚假 sender_id。

**验收标准**：
- node_id 为 None 时，`maybe_push_to_network` 记录 `warn` 日志并返回，不发送 envelope
- 单元测试验证该行为

## 范围

### In Scope

- **F1**：Ed25519 签名集成（signing_keypair 字段 + sign_envelope 调用）— oris-evokernel
- **F2**：sender_id skip 策略（node_id = None 时跳过推送）— oris-evokernel
- **F3**：bootstrap + trusted local 路径补充 push 调用 — oris-evokernel（2 处）
- **F4**：Homepage HTML 页面（GET / axum handler）— oris-experience-repo
- 相应的单元测试

### Out of Scope

- TLS/mTLS 传输层（oris-evolution-network gossip 层另行处理）
- 前端框架（Vue/React），homepage 使用纯 HTML + inline CSS
- 已解决的 backlog 事项（P0-fix-1, P1-fix-5, P1-fix-6, P1-fix-7 均已在代码中修复）
- crates.io 发版（本 sprint 仅交付代码，发版由后续决策）

## 风险与依赖

| 风险 | 等级 | 缓解 |
|------|------|------|
| ensure_builtin / record_reported 是 standalone 函数，需传入 publisher 参数 | P1 | 函数签名扩展需检查所有 call sites |
| NodeKeypair 在 oris-evolution-network crate，需确认 oris-evokernel Cargo.toml 已引用 | P1 | 检查 Cargo.toml path dep |
| Homepage HTML inline CSS 未来维护成本 | LOW | v0.4.0 scope 内可接受，后续可迁移到模板 |

## 待确认项

- F3 中 ensure_builtin 和 record_reported 改为接受 `Option<&dyn NetworkPublisher>` 参数还是通过关联结构体传递？（推荐：参数传入，保持函数式风格）
- Homepage 版本号是否动态读取（从 Cargo.toml 的 env!("CARGO_PKG_VERSION")）？（推荐：是）

## 参与角色

| 角色 | 分工 |
|------|------|
| backend-engineer (oris-evokernel) | F1 签名集成、F2 sender_id skip、F3 push 路径补全 |
| backend-engineer (oris-experience-repo) | F4 Homepage handler |
| qa-engineer | 回归测试、放行建议 |
| tech-lead | 收口与放行 |
