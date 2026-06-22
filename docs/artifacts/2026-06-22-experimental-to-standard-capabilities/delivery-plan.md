# Delivery Plan — Standardize Experimental Capabilities

**状态**: implemented-fifth-batch
**日期**: 2026-06-22
**阶段**: story-5-complete
**关联审查**: `audit.md`

---

## 本轮目标

将已经具备生产语义的实验能力逐步转为标准能力，避免继续让稳定路径依赖 `*-experimental` 命名。

第一批不追求“全量自进化标准化”，只做低风险、可验证、向后兼容的标准化。

---

## 第一批范围

### In Scope

- 新增标准 feature aliases：
  - `evolution`
  - `governor`
  - `evolution-network`
  - `agent-contract`
- 保留旧 `*-experimental` features 作为兼容别名。
- 将 `a2a-production` 依赖从实验 feature 切换到标准 feature。
- 增加文档说明：稳定 `/a2a/*` 与实验 `/v1/evolution/*` 的边界。
- 增加/保留 route availability test，证明 `a2a-production` 不暴露 experimental evolution routes。

### Out of Scope

- 不标准化 `full-evolution-experimental`。
- 不标准化完整 MCP 协议，仅标准化 bootstrap/capability discovery 切片。
- 不标准化 `release-automation-experimental`。
- 不宣称 always-on autonomous self-evolution 稳定。
- distributed economics、spec migration 等后续成熟度项不纳入第一批。

---

## Story 1：Feature Alias Migration

**目标**: 增加标准 feature 名称，并保留旧实验 feature 兼容。

文件：

- `crates/oris-runtime/Cargo.toml`
- `crates/oris-execution-server/Cargo.toml`
- `README.md`
- `docs/evolution-boundary.md`
- `docs/open-source-onboarding-zh.md`

实际改法：

```toml
evolution = ["evolution-experimental"]
governor = ["governor-experimental"]
evolution-network = ["evolution-network-experimental"]
agent-contract = ["agent-contract-experimental"]

evolution-experimental = ["evokernel-facade"]
governor-experimental = ["evokernel-facade"]
evolution-network-experimental = ["evokernel-facade", "evolution-experimental"]
agent-contract-experimental = ["evokernel-facade"]

a2a-production = [
    "execution-server",
    "agent-contract",
    "evolution-network",
]
```

说明：当前代码仍有大量内部 `cfg(feature = "...-experimental")`。第一批先建立标准外部入口并迁移 `a2a-production` 的直接依赖；内部 cfg 重命名留给后续独立 Story，避免大面积条件编译风险。

验收标准：

- 旧命令仍可运行：`--features full-evolution-experimental`
- 新命令可运行：`--features a2a-production`
- `a2a-production` 不再直接依赖 `*-experimental`

验证：

```bash
cargo fmt --all -- --check
cargo test -p oris-runtime --features "sqlite-persistence,execution-server,a2a-production" a2a_
cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental
```

---

## Story 2：Stable Route Boundary Tests

**目标**: 用测试锁住稳定/实验路由边界。当前复用现有测试覆盖。

文件：

- `crates/oris-runtime/src/execution_server/api_handlers.rs`
- 测试：`a2a_production_route_boundary_hides_evolution_network_routes`

验收标准：

- `a2a-production` feature 下 `/a2a/*` route 可用。
- `a2a-production` feature 下 `/v1/evolution/publish`、`/v1/evolution/fetch`、`/v1/evolution/revoke` 不可用，除非显式启用 evolution experimental route group。
- 测试名明确包含稳定边界语义。

验证：

```bash
cargo test -p oris-runtime --features "sqlite-persistence,execution-server,a2a-production" a2a_production_route_boundary_hides_evolution_network_routes -- --nocapture
```

---

## Story 3：Supervised Evolution Stable API

**目标**: 把 supervised capture/replay 子集从实验 facade 中拆成标准 feature。

候选文件：

- `crates/oris-runtime/src/evolution.rs`
- `crates/oris-runtime/Cargo.toml`
- `docs/evolution-stable-api.md`
- `crates/oris-runtime/examples/evolution_supervised_quickstart.rs`

验收标准：

- `--features evolution` 可编译。
- 示例不依赖 `full-evolution-experimental`。
- 文档明确标准范围仅是 supervised capture/replay，不包含 autonomous release。

验证：

```bash
cargo test -p oris-evolution
cargo test -p oris-runtime --features evolution evolution_
cargo run -p oris-runtime --example evolution_supervised_quickstart --features evolution
```

---

## Story 4：Governor Policy-Only Stable API

**目标**: 标准化 governor policy decision，不标准化自动执行。

验收标准：

- `--features governor` 可编译。
- `oris_runtime::governor` 文档说明 policy-only。
- 测试覆盖 promotion/cooldown/revocation decision。

验证：

```bash
cargo test -p oris-governor
cargo test -p oris-runtime --features governor governor_
```

---

## Story 5：Release and Deprecation Notes

**目标**: 给用户明确迁移路径。

交付：

- Release note: `*-experimental` 保留为兼容别名。
- README: 新用户使用标准 feature；老用户命令仍有效。
- Deprecation window: 至少一个 minor 版本周期。

---

## 第一批验证结果

已执行：

```bash
cargo metadata --no-deps --format-version 1 --features evolution,governor,agent-contract,evolution-network,a2a-production
cargo test -p oris-runtime --features evolution --lib --no-run
cargo test -p oris-runtime --features "sqlite-persistence,execution-server,a2a-production" a2a_production_route_boundary_hides_evolution_network_routes -- --nocapture
cargo fmt --all -- --check
cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental
cargo test -p oris-execution-server --features evolution-network --no-run
```

结果：

- 标准 feature 组合可解析。
- `evolution` 标准入口可编译。
- `a2a-production` 稳定边界测试通过。
- 旧 `full-evolution-experimental` wiring 测试通过。
- `oris-execution-server/evolution-network` 标准入口可编译。

---

## 第二批处理范围

目标：

- 将 `oris-evolution` 中 TOML task-class 加载从过宽的 `evolution-experimental` 命名迁出。
- 新增标准 feature `task-class-toml`，旧 `evolution-experimental` 保留为兼容别名。
- 修正 README 中不存在的 `intake-experimental` gate，改为 `standalone crate`。
- 同步 EvoKernel 文档中的标准 feature 指引。

实现：

```toml
task-class-toml = ["dep:toml"]
evolution-experimental = ["task-class-toml"]
```

保留实验隔离：

- `full-evolution-experimental`
- `release-automation-experimental`
- evolution-network 宽路由：`/v1/evolution/*` 和 `/evolution/a2a/*`

已执行：

```bash
cargo fmt --all -- --check
cargo test -p oris-evolution
cargo test -p oris-evolution --features task-class-toml load_task_classes_from_toml_parses_standard_task_class_file
cargo test -p oris-evolution --features evolution-experimental load_task_classes_from_toml_parses_standard_task_class_file
cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental
```

结果：

- `oris-evolution` 默认测试通过。
- 标准 `task-class-toml` feature 可编译并通过 TOML 加载测试。
- 旧 `evolution-experimental` 兼容入口仍可编译并通过同一测试。
- `oris-runtime/full-evolution-experimental` wiring 仍通过。

---

## 第三批处理范围

目标：

- 将 MCP bootstrap/capability discovery 从 `mcp-experimental` 推荐入口迁出。
- 新增标准 feature `mcp-bootstrap`，旧 `mcp-experimental` 保留为兼容别名。
- starter Axum 示例改用 `mcp-bootstrap`。
- 文档明确：标准化范围只包含 bootstrap metadata 与 capability discovery，不声明完整 MCP 协议生命周期稳定。

实现：

```toml
mcp-bootstrap = ["mcp-experimental"]
mcp-experimental = ["execution-server"]
```

保留实验隔离：

- 完整 MCP JSON-RPC transport/session lifecycle
- MCP tool invocation bridge
- MCP auth/session identity bridge
- `full-evolution-experimental`
- `release-automation-experimental`

已执行：

```bash
cargo fmt --all -- --check
cargo test -p oris-runtime --features "execution-server,mcp-bootstrap" mcp_
cargo test -p oris-runtime --features "execution-server,mcp-experimental" mcp_
cargo test -p oris-execution-server --features mcp-bootstrap --no-run
cargo test -p oris_starter_axum --no-run
```

结果：

- 标准 `mcp-bootstrap` 入口通过 MCP bootstrap/capability discovery 测试。
- 旧 `mcp-experimental` 兼容入口通过同一测试。
- `oris-execution-server/mcp-bootstrap` 可编译。
- starter Axum 示例使用标准 `mcp-bootstrap` 后可编译。

---

## 第四批处理范围

目标：

- 将 `oris-runtime` 的 economics/spec 推荐入口从实验名迁出。
- 新增标准 feature `economics` 与 `spec-contract`。
- 保留旧 `economics-experimental` 与 `spec-experimental` 作为兼容别名。
- 明确标准范围只包含本地 EVU ledger/reputation baseline 与 OUSL YAML parsing/mutation-plan compiler baseline。

实现：

```toml
economics = ["economics-experimental"]
spec-contract = ["spec-experimental"]
economics-experimental = ["evokernel-facade"]
spec-experimental = ["evokernel-facade"]
```

保留实验隔离：

- distributed economics：跨节点 EVU 结算、一致性和最终性。
- spec migration：OUSL 版本迁移和兼容策略。
- `full-evolution-experimental`：聚合 demo/test surface。
- `release-automation-experimental`：真实发布执行器。
- evolution-network 宽路由：`/v1/evolution/*`、`/evolution/a2a/*`、session replication/lifecycle。

验证：

```bash
cargo fmt --all -- --check
cargo test -p oris-economics
cargo test -p oris-spec
cargo test -p oris-runtime --test economics_feature_wiring --features economics
cargo test -p oris-runtime --test spec_contract_feature_wiring --features spec-contract
cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental
```

---

## 第五批处理范围

目标：

- 将已经标准化的 runtime facade 与 execution-server 条件编译从“只识别实验名”迁移到 `any(standard, legacy)`。
- 覆盖 `evolution`、`governor`、`agent-contract`、`evolution-network`、`mcp-bootstrap`。
- 保持旧 `*-experimental` 和 `mcp-experimental` 编译入口兼容。
- 不迁移 `full-evolution-experimental`，因为它仍是刻意保留的聚合实验边界。

实现：

```rust
#[cfg(any(feature = "evolution", feature = "evolution-experimental"))]
#[cfg(any(feature = "mcp-bootstrap", feature = "mcp-experimental"))]
#[cfg(all(
    any(feature = "agent-contract", feature = "agent-contract-experimental"),
    any(feature = "evolution-network", feature = "evolution-network-experimental"),
))]
```

验证：

```bash
cargo fmt --all -- --check
cargo test -p oris-runtime --features evolution --lib --no-run
cargo test -p oris-runtime --features governor --lib --no-run
cargo test -p oris-runtime --features agent-contract --lib --no-run
cargo test -p oris-runtime --features evolution-network --lib --no-run
cargo test -p oris-runtime --features "execution-server,mcp-bootstrap" mcp_
cargo test -p oris-runtime --features "sqlite-persistence,execution-server,a2a-production" a2a_production_route_boundary_hides_evolution_network_routes
cargo test -p oris-runtime --test architecture --features "execution-server,agent-contract,evolution-network" gep_
cargo test -p oris-runtime --features "execution-server,mcp-experimental" mcp_
cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental
```

---

## 推荐执行顺序

1. Story 1 + Story 2 先合并：低风险，直接解决稳定路径依赖实验命名问题。
2. Story 3 单独 PR：涉及 runtime public facade，需要更完整 API 审查。
3. Story 4 单独 PR：policy-only 标准化。
4. Story 5 随每个 release 递进执行。
