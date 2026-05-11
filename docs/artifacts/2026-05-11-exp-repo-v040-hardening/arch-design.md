---
artifact: arch-design
task: exp-repo-v040-hardening
date: 2026-05-11
role: architect
status: draft
state: plan
---

# Arch Design — Experience Repo v0.4.0 Hardening + Homepage

## 系统边界

```
┌─────────────────────────────────────────────────────┐
│  oris-evokernel (core.rs)                           │
│                                                     │
│  EvolutionNetworkNode / StoreReplayExecutor /       │
│  NetworkNodeConfig                                  │
│    ├─ signing_keypair: Option<Arc<NodeKeypair>> [NEW]│
│    └─ maybe_push_to_network()                       │
│         ├─ F2: node_id=None → warn + return         │
│         └─ F1: keypair.is_some() → sign_envelope()  │
│                                                     │
│  ensure_builtin_experience_assets_in_store()        │
│    └─ publisher: Option<&dyn NetworkPublisher> [NEW] │
│         └─ F3: if promoted → push via publisher     │
│                                                     │
│  record_reported_experience_in_store()              │
│    └─ publisher: Option<&dyn NetworkPublisher> [NEW] │
│         └─ F3: if promoted → push via publisher     │
└─────────────────────────────────────────────────────┘
         │ signs via
         ▼
┌──────────────────────────────┐
│  oris-evolution-network      │
│    signing.rs                │
│    ├─ NodeKeypair [+Clone]   │
│    └─ sign_envelope()        │
└──────────────────────────────┘

┌──────────────────────────────┐
│  oris-experience-repo        │
│    handlers.rs               │
│    └─ GET /  → homepage() [NEW]│
│         └─ Html<String>      │
└──────────────────────────────┘
```

## 组件拆分

| 组件 | 职责 | 改动 |
|------|------|------|
| `NodeKeypair` (signing.rs) | Ed25519 密钥封装 | + `impl Clone` |
| `StoreReplayExecutor` (core.rs ~L1062) | replay 路径网络推送 | + `signing_keypair` 字段，`maybe_push_to_network` 改签名/skip 逻辑 |
| `EvolutionNetworkNode` (core.rs ~L2240) | bootstrap/trusted/replay 路径 | 同上 + F3 参数传入 |
| `NetworkNodeConfig` (core.rs ~L2509) | 节点配置 | 同上 |
| `ensure_builtin_experience_assets_in_store` (core.rs ~L7428) | bootstrap gene 促进 | + `publisher` 参数，促进后推送 |
| `record_reported_experience_in_store` (core.rs ~L7685) | trusted local 促进 | + `publisher` 参数，促进后推送 |
| `homepage` handler (handlers.rs) | HTTP GET / | 新增 Html<String> handler |

## 关键数据流

### F1 — 签名路径

```
maybe_push_to_network(gene, node_id)
  → 构建 EvolutionEnvelope
  → if signing_keypair.is_some()
       → sign_envelope(&keypair, &envelope)  → SignedEnvelope
     else
       → envelope.publish()  → SignedEnvelope (sig=None)
  → publisher.publish(signed_envelope)
```

### F2 — sender_id skip 路径

```
maybe_push_to_network(gene, node_id_opt)
  → match node_id_opt
       Some(id) → 继续
       None     → warn!("Network push skipped: node_id not configured") + return
```

### F3 — bootstrap/trusted push 路径

```
ensure_builtin_experience_assets_in_store(store, publisher, ...)
  → gene = GeneStore::upsert(...)
  → event = GenePromoted { ... }
  → store.append(event)
  → if let Some(pub) = publisher
       → pub.publish(envelope_from(gene))  // 新增
```

### F4 — Homepage 路径

```
GET /
  → homepage()
  → Html(format!(html_template, version=env!("CARGO_PKG_VERSION")))
  → 200 Content-Type: text/html
```

## 接口约定

### NodeKeypair Clone

```rust
// signing.rs
impl Clone for NodeKeypair {
    fn clone(&self) -> Self {
        NodeKeypair {
            signing_key: self.signing_key.clone(),  // ed25519-dalek SigningKey: Clone
            path: self.path.clone(),
        }
    }
}
```

### EvolutionNetworkNode 新字段

```rust
pub struct EvolutionNetworkNode {
    // ... 原有字段
    pub signing_keypair: Option<Arc<NodeKeypair>>,  // 新增
}

impl EvolutionNetworkNode {
    pub fn with_signing_keypair(mut self, kp: NodeKeypair) -> Self {
        self.signing_keypair = Some(Arc::new(kp));
        self
    }
}
```

### _in_store fn 新签名

```rust
pub(crate) fn ensure_builtin_experience_assets_in_store(
    store: &SqliteGeneStore,
    publisher: Option<&dyn NetworkPublisher>,  // 新增
    // ... 原有参数保持不变
) { ... }
```

### Homepage handler

```rust
// handlers.rs
pub async fn homepage() -> Html<String> {
    let version = env!("CARGO_PKG_VERSION");
    Html(format!(include_str!("../templates/homepage.html") 或 inline template))
}

// 路由注册（create_routes 或 build_router）
.route("/", get(homepage))
```

## 技术选型

| 决策 | 选择 | 原因 |
|------|------|------|
| keypair 存储方式 | `Option<Arc<NodeKeypair>>` | Arc 允许多 struct 共享，不需要 Clone 语义问题 |
| publisher 参数类型 | `Option<&dyn NetworkPublisher>` | 借用比 Arc 传参轻量，_in_store fn 是 pub(crate) 同步函数 |
| homepage 版本号 | `env!("CARGO_PKG_VERSION")` | 编译期宏，无运行时开销，与 Cargo.toml 保持一致 |
| homepage HTML | inline format string | 无外部 CDN 依赖，无模板引擎，满足"纯 HTML + inline CSS"要求 |

## 风险与约束

| 风险 | 处理 |
|------|------|
| signing.rs 中 `NodeKeypair.path` 字段类型未知 | 读代码确认 path 字段是 PathBuf 或 Option<PathBuf>，Clone 均支持 |
| _in_store fn 调用点 — call sites 可能超过预期 | grep 确认后再改签名 |
| ensure_builtin 被 rebuild_projection 调用（无 publisher） | 调用点传 `None` 即可，行为退化为静默 |
