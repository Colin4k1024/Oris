# Release Plan

## 发布信息

| 字段 | 内容 |
|------|------|
| 任务 slug | exp-repo-evokernel-wire |
| 日期 | 2026-05-11 |
| 版本目标 | 补全 ExperienceRepo ↔ EvoKernel 运行时连接胶水 |
| 主责 | devops-engineer |
| 发布类型 | Library patch（非独立服务） |

## 变更范围

### 已完成（R1–R8 全部）

| 编号 | 变更 | 影响 crate |
|------|------|------------|
| R1 | `ExperienceRepoClient::share_experience()` POST /experience 方法实现 | oris-experience-repo |
| R2 | `EvolutionNetworkNode` 增加 `network_publisher` / `node_id` 字段 + builder | oris-evokernel |
| R3 | `ensure_builtin_experience_assets` / `record_reported_experience` 异步化 + 网络推送 | oris-evokernel |
| R4 | `GeneStore::get_gene()` 方法补全 | oris-genestore |
| R5 | `NetworkPublisher` DIP 抽象 + `EvolutionNetworkPublisher` 实现 | oris-evolution-network |
| R6 | `contributor_id` 字段 fetch handler 枚举化（从 None 到实际值） | oris-experience-repo |
| R7 | 全量 call-site async 传播修复（15 个调用点） | oris-evokernel / oris-runtime |
| R8 | Handler 测试修复（KeyStore bootstrap + authed headers） | oris-experience-repo |

### 变更文件（核心）

- `crates/oris-evokernel/src/core.rs`
- `crates/oris-evokernel/src/adapters.rs`
- `crates/oris-evolution-network/src/publisher.rs`
- `crates/oris-evolution-network/src/lib.rs`
- `crates/oris-experience-repo/src/client/client.rs`
- `crates/oris-experience-repo/src/server/handlers.rs`
- `crates/oris-genestore/src/store.rs` / `types.rs`
- `crates/oris-runtime/src/execution_server/api_handlers.rs`
- 4 个测试文件

## 风险与缓解

| 风险 | 等级 | 缓解 |
|------|------|------|
| async 传播遗漏 call site | LOW | 已通过全量 `cargo build --all-features` 验证，579 tests 通过 |
| NetworkPublisher 未配置时行为 | LOW | `is_some()` guard，无配置时静默跳过，无副作用 |
| `ort`/ONNX 第三方构建错误 | LOW | Pre-existing，与本次变更无关，已验证隔离 |

## 执行步骤

### 发布前检查

```bash
# 1. 格式检查
cargo fmt --all -- --check

# 2. 演化核心测试
cargo test -p oris-evokernel --release --all-features

# 3. 经验仓库测试
cargo test -p oris-experience-repo --release

# 4. 完整发布构建
cargo build -p oris-evokernel --release --all-features
cargo build -p oris-runtime --release --features "full-evolution-experimental,execution-server,sqlite-persistence,a2a-production"

# 5. 全量集成测试（目标 crates）
cargo test -p oris-genestore -p oris-evolution-network -p oris-evolution -p oris-kernel -p oris-governor --release
```

### 发布执行

```bash
# Dry run
cargo publish -p oris-experience-repo --dry-run
cargo publish -p oris-evokernel --dry-run
cargo publish -p oris-runtime --all-features --dry-run

# 正式发布（按依赖顺序）
cargo publish -p oris-genestore
cargo publish -p oris-evolution-network
cargo publish -p oris-experience-repo
cargo publish -p oris-evokernel
cargo publish -p oris-runtime --all-features
```

## 验证与监控

| 验证项 | 预期结果 | 责任方 |
|--------|----------|--------|
| 全量测试通过 | 579 tests pass, 0 failures | devops-engineer |
| `cargo fmt` 清洁 | No diff | devops-engineer |
| crates.io 发布成功 | 各 crate 版本可拉取 | devops-engineer |
| 下游集成方编译 | No breaking changes | 48h 观察 |

## 回滚方案

| 触发条件 | 方案 |
|----------|------|
| crates.io 发布后下游报编译错误 | `cargo add <crate>@<prev-version>` 降版本，回滚时间 < 30min |
| 运行时 async panic 出现 | git revert + hotfix patch |

## 放行结论

**Go — 允许发布**

- 所有 579 测试通过（0 failures）
- R1–R8 功能全部实现并通过回归验证
- async 传播已全量修复，无遗漏 call site
- `network_publisher` 可选设计确保向后兼容
- 无 CRITICAL 安全问题，无阻塞项

**观察重点**：发布后 48h 关注 CI 绿灯状态和下游使用方编译报告。
