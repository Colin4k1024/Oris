---
artifact: execute-log
task: exp-repo-evokernel-wire
date: 2026-05-11
role: backend-engineer
status: completed
state: execute
---

# Execute Log — 经验仓库 × EvoKernel 集成胶水修复

## 计划 vs 实际

| 任务 | 原计划 | 实际结果 | 偏差说明 |
|------|--------|----------|----------|
| T1 — `share_experience()` + tests | 实现 POST /experience 客户端方法及 mockito 单测 | ✅ 完成 | 无偏差 |
| T2a — `NetworkPublisher` trait | 在 oris-evolution-network 定义抽象 trait | ✅ 完成 | 无偏差 |
| T2b — `impl NetworkPublisher for ExperienceRepoClient` | 实现 trait，映射 EvolutionEnvelope → OenEnvelope | ✅ 完成 | 无偏差 |
| T2c — EvoKernel 注入 + 晋升调用 | 在 core.rs 注入 `Option<Arc<dyn NetworkPublisher>>`，晋升后调用 | ✅ 完成 | 无偏差 |
| T3a — GeneStore `contributor_id` struct 字段 | `types.rs` 新增字段 + `#[serde(default)]` | ✅ 完成 | 无偏差 |
| T3b — GeneStore 持久化层同步更新 | upsert/get/search + 幂等迁移 | ✅ 完成，迁移方式有调整（见下）| `IF NOT EXISTS` 换为忽略重复列错误 |
| T3c — handler contributor_id 填充 | `fetch_experiences` 从 gene 对象取值 | ✅ 完成 | 无偏差 |

---

## 关键决策

### 1. Cargo path dep 替代 registry dep

**问题**：`oris-experience-repo/Cargo.toml` 原先以 crates.io 版本锁定三个 sibling crate（`oris-genestore`, `oris-evolution`, `oris-evolution-network`），导致新增的本地符号（`NetworkPublisher` trait、`contributor_id` 字段）在编译时不可见。

**决策**：将三个依赖改为 `{ path = "../..." }` 本地路径引用，与其他 workspace sibling 保持一致。

**影响**：`Cargo.lock` 中 registry 条目被替换为 workspace 本地引用，编译链统一，无发布回归。

### 2. `ALTER TABLE ADD COLUMN` 幂等方案

**问题**：`ALTER TABLE genes ADD COLUMN IF NOT EXISTS contributor_id TEXT` 语法要求 SQLite ≥ 3.37.0，但 `libsqlite3-sys` v0.30.1 bundled SQLite 版本低于该阈值，导致所有依赖 `SqliteGeneStore::open()` 的测试 panic。

**决策**：改为 `let _ = conn.execute("ALTER TABLE genes ADD COLUMN contributor_id TEXT", []);`，静默忽略"duplicate column name"错误，兼容所有 SQLite 3.x。

**影响**：行为等价，无功能差异；预先存在 `contributor_id` 列的数据库不受影响。

### 3. NetworkPublisher trait 位于 oris-evolution-network

**问题**：trait 可以放在 oris-evolution-network 或单独抽象 crate。

**决策**：放在 `oris-evolution-network::publisher` 模块，从 lib.rs re-export。理由：trait 的入参是 `EvolutionEnvelope`（定义在 `oris-evolution-network`），co-location 避免额外 crate 依赖。

### 4. `maybe_push_to_network` 非阻断设计

**决策**：推送失败时 `tracing::warn!`，不 return Err，不 panic。晋升主路径（`GenePromoted` 事件追加）先于推送完成，推送是 best-effort side-effect。

---

## 阻塞与解决

| 阻塞 | 根因 | 解决方式 |
|------|------|----------|
| `NetworkPublisher` 在 LSP 中显示未找到 | oris-experience-repo 使用 registry 版 oris-evolution-network，无本地新 trait | 切换为 path dep |
| `ALTER TABLE IF NOT EXISTS` 语法错误 | bundled SQLite 版本 < 3.37 | 改为忽略错误方式 |

---

## 影响面

| 模块 | 变更类型 | 向后兼容 |
|------|----------|----------|
| `oris-evolution-network/src/publisher.rs` | 新增文件（trait 定义） | ✅ 纯新增 |
| `oris-evolution-network/src/lib.rs` | re-export `NetworkPublisher`, `NetworkPublishError` | ✅ 纯新增 |
| `oris-evolution-network/Cargo.toml` | 新增 `async-trait`, `thiserror` dep | ✅ 已在 workspace |
| `oris-experience-repo/src/client/client.rs` | 新增 `share_experience()` + `impl NetworkPublisher` | ✅ 纯新增 |
| `oris-experience-repo/src/api/request.rs` | 新增 `ShareRequest` struct | ✅ 纯新增 |
| `oris-experience-repo/Cargo.toml` | sibling crate 改为 path dep | ✅ 仅影响本地编译链 |
| `oris-evokernel/src/core.rs` | 新增 `network_publisher` 字段 + builder + call-site | ✅ 字段 `Option`，默认 `None`，不改变现有行为 |
| `oris-genestore/src/types.rs` | `Gene` 新增 `contributor_id: Option<String>` | ✅ `serde(default)`，JSON/TOML 向后兼容 |
| `oris-genestore/src/store.rs` | 持久化层 + 幂等迁移 | ✅ 已有 DB 通过 ALTER TABLE 自动升级 |
| `oris-experience-repo/src/server/handlers.rs` | `contributor_id: gene.contributor_id` | ✅ 修复 None → 真实值 |

---

## 测试结果

```
cargo test -p oris-experience-repo -p oris-genestore -p oris-evokernel
→ 247 passed, 0 failed (10 suites)
```

| Crate | 通过 | 失败 |
|-------|------|------|
| oris-experience-repo | 74 | 0 |
| oris-genestore | 81 | 0 |
| oris-evokernel | 92 | 0 |

---

## 未完成项

无。所有 PRD 范围内任务均已完成并测试通过。
