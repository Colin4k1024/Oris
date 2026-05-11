---
artifact: delivery-plan
task: exp-repo-v040-hardening
date: 2026-05-11
role: tech-lead
status: draft
state: plan
---

# Delivery Plan — Experience Repo v0.4.0 Hardening + Homepage

## 版本目标

| 字段 | 内容 |
|------|------|
| 版本 | v0.4.0（oris-evokernel + oris-evolution-network + oris-experience-repo patch） |
| 放行标准 | cargo test 全绿、F1–F4 验收标准全部满足 |
| 范围说明 | 修复 2 个 P0 功能缺陷（签名 + sender_id）、补全 2 处促进路径推送、新增 homepage |

---

## Requirement Challenge Session

### 核心假设质疑

| 假设 | 质疑人 | 结论 |
|------|--------|------|
| NodeKeypair 可以安全 Clone | architect | ⚠️ NodeKeypair 内部包含 ed25519-dalek SigningKey；SigningKey 自身实现 Clone，但 NodeKeypair 当前 derive 中无 Clone。**结论：需要手动 `impl Clone for NodeKeypair` 并确认 SigningKey::clone() 语义安全** |
| 3 处 GenePromoted 路径缺失推送 | tech-lead | ✅ 代码审计结果：实际只有 2 处（bootstrap:7505 + trusted:7790），replay 路径已有推送。PRD 已修正 |
| 签名链路只需要在 maybe_push_to_network 加一行 | backend-engineer | ⚠️ 需要确认 signing_keypair 字段的存储方式。Arc<NodeKeypair> 最安全（避免 Clone 跨线程语义问题）。**结论：3 个 maybe_push_to_network 载体 struct 均添加 `signing_keypair: Option<Arc<NodeKeypair>>`** |
| ensure_builtin_in_store 可以直接调用 self 方法 | backend-engineer | ❌ 是 standalone fn，没有 self。需要传参。**结论：添加 `publisher: Option<&dyn NetworkPublisher>` 参数** |

### 最简备选路径

- F1 备选：在 maybe_push_to_network 内判断 signing_keypair，直接 borrow Arc 传 &NodeKeypair。无需 Clone，无线程开销。**采用**
- F3 备选：不改 _in_store fn，在包装方法里在促进后再手动调用 maybe_push_to_network。更复杂且需要独立 gene 对象。**放弃，改参数传入更清晰**

### 当前不做项

- NodeKeypair Send+Sync 实现（目前不跨线程传递，Arc 已足够）
- Ed25519 证书链验证（服务端已有，客户端不需要）
- Homepage 生产级样式（纯 HTML + inline CSS，v0.4.0 scope 内可接受）

---

## 工作拆解

| # | 工作项 | 主责角色 | 依赖 | 预估 |
|---|--------|----------|------|------|
| F1 | `NodeKeypair` 添加 `Clone + Send + Sync`；3 个 struct 添加 `signing_keypair: Option<Arc<NodeKeypair>>`；`maybe_push_to_network` 调用 `sign_envelope` | eng-evokernel | 无 | 1h |
| F2 | `maybe_push_to_network` 中 node_id = None 时 warn + return，不发送 envelope | eng-evokernel | 无（可与 F1 并行） |  30min |
| F3 | `ensure_builtin_experience_assets_in_store` + `record_reported_experience_in_store` 添加 `publisher: Option<&dyn NetworkPublisher>` 参数；包装方法传递 Arc | eng-evokernel | F1（需要 Arc<NetworkPublisher> 类型确认）| 45min |
| F4 | oris-experience-repo 添加 `GET /` axum handler，返回 `Html<String>`，版本从 `env!("CARGO_PKG_VERSION")` 读取 | eng-homepage | 无（完全独立）| 30min |
| TEST | `cargo test --release --all-features`，新增单元测试验证 F1/F2/F3/F4 | 两个 agent 分别补测 | F1-F4 完成后 | 30min |

---

## 关键技术决策

### F1：signing_keypair 字段类型

```rust
// 选 Arc<NodeKeypair> 而非直接持有 NodeKeypair
signing_keypair: Option<Arc<NodeKeypair>>
```

**原因：** Arc 避免 Clone 跨线程语义问题，且多个 struct 实例可共享同一个 keypair 实例。

### F1：sign_envelope 调用位置

在 `maybe_push_to_network` 中，`EvolutionEnvelope::publish()` 创建 envelope 后：

```rust
let envelope = if let Some(kp) = &self.signing_keypair {
    sign_envelope(kp, &unsigned_envelope)
} else {
    unsigned_envelope.publish()  // 原有逻辑，签名为 None
};
```

### F2：sender_id skip 策略

```rust
let node_id = match self.node_id.as_deref() {
    Some(id) => id.to_string(),
    None => {
        warn!("Network push skipped: node_id not configured");
        return;
    }
};
```

### F3：_in_store fn 参数扩展

```rust
pub(crate) fn ensure_builtin_experience_assets_in_store(
    store: &SqliteGeneStore,
    publisher: Option<&dyn NetworkPublisher>,  // 新增
    // ... 原有参数
) { ... }
```

调用方（包装方法）传入 `self.network_publisher.as_deref()`。

### F4：Homepage handler

```rust
pub async fn homepage() -> Html<String> {
    let version = env!("CARGO_PKG_VERSION");
    Html(format!(r#"<!DOCTYPE html>...</html>"#, version = version))
}
```

路由注册：`.route("/", get(homepage))` 加入现有 `create_routes()`。

---

## Brownfield 上下文快照

| 模块 | 当前状态 | 改动影响 |
|------|----------|----------|
| `oris-evokernel/src/core.rs` | 3 个 maybe_push_to_network + 2 个 _in_store fn | F1/F2/F3 全部在此文件 |
| `oris-evolution-network/src/signing.rs` | NodeKeypair + sign_envelope 已有 | 仅添加 Clone impl |
| `oris-experience-repo/src/server/handlers.rs` | 无 GET / 路由 | F4 添加 handler + 路由 |
| `oris-evolution-network/Cargo.toml` | path dep 在 evokernel 已引用 | 无需改动 |

---

## 风险与缓解

| 风险 | 影响 | 缓解 | Owner |
|------|------|------|-------|
| NodeKeypair Clone impl 语义问题 | P1 | 验证 ed25519-dalek SigningKey::clone() 是深复制 | eng-evokernel |
| _in_store call sites 遗漏 | P1 | grep 枚举所有调用点后再改参数签名 | eng-evokernel |
| EvolutionEnvelope 签名 API 改动 | P1 | sign_envelope 返回 SignedEnvelope，确认 publish 调用与 sign 调用产物类型一致 | eng-evokernel |
| Homepage 版本号显示错误 | LOW | env! 宏编译期展开，cargo build 后即可验证 | eng-homepage |

---

## 节点检查

| 节点 | 完成条件 |
|------|----------|
| 方案评审 ✅ | 上方 Requirement Challenge Session 已质疑收口 |
| 开发完成 | F1–F4 代码变更完成，no compile error |
| 测试完成 | cargo test 全绿，新单测覆盖签名/skip/push/homepage |
| 发布准备 | 版本号 bump，changelog 更新 |

---

## 技能装配

| 能力 | 说明 |
|------|------|
| backend-engineer (oris-evokernel) | F1/F2/F3 实现 |
| backend-engineer (oris-experience-repo) | F4 实现 |
| qa-engineer | 测试放行 |
| tech-lead | 收口与放行 |

---

## 交接状态

| 字段 | 内容 |
|------|------|
| 当前阶段 | plan |
| 目标阶段 | execute |
| 就绪状态 | handoff-ready |
| 阻塞项 | 无 |
| readiness proof | Requirement Challenge Session 完成，架构决策（F1 Arc 存储、F2 skip 策略、F3 参数传入、F4 Html handler）已收口；brownfield 上下文已验证代码状态 |
