# Audit — Experimental Capabilities to Standard Capabilities

**状态**: implemented-fourth-batch
**日期**: 2026-06-22
**阶段**: audit
**主责角色**: tech-lead / architect
**范围**: `oris-runtime` facade feature gates, execution-server routes, EvoKernel crates, orchestrator release automation, MCP bootstrap

---

## 结论

当前项目中“实验能力”不是一个整体。应拆成三类处理：

1. **可优先标准化**：已经有稳定边界、测试覆盖和生产语义的能力。首选是 `a2a-production` 依赖的 A2A contract/network facade。
2. **需补契约后标准化**：已有实现和示例，但 API、数据契约、错误语义、验证矩阵还不足。包括 `evolution-experimental`、`governor-experimental`、`agent-contract-experimental` 中被 A2A 稳定路径实际依赖的子集。
3. **继续实验隔离**：能力强依赖外部运行环境、自动发布权限或宽路由边界，不能直接改成标准。包括 `release-automation-experimental`、完整 `full-evolution-experimental`、完整 MCP JSON-RPC/session/tool bridge，以及 evolution-network 宽路由。

最短标准化路径不是把所有 `*-experimental` 直接重命名，而是增加标准 feature alias，保留旧 feature 作为兼容入口，并逐步把文档和示例迁移过去。

第一批已落地：新增标准 feature 入口 `evolution`、`governor`、`agent-contract`、`evolution-network`，并让 `a2a-production` 改为依赖标准入口。旧 `*-experimental` 仍作为内部 cfg 与兼容入口保留。

第二批已落地：`oris-evolution` 的 TOML task-class 加载迁移到标准 `task-class-toml`，旧 `evolution-experimental` 作为兼容别名保留。

第三批已落地：MCP bootstrap metadata/capability discovery 迁移到标准 `mcp-bootstrap`，旧 `mcp-experimental` 作为兼容别名保留；完整 MCP JSON-RPC/session/tool bridge 仍不标准化。

第四批已落地：新增标准 feature 入口 `economics` 与 `spec-contract`，分别限定为本地 EVU ledger/reputation baseline 与 OUSL YAML parsing/mutation-plan compiler baseline。旧 `economics-experimental`、`spec-experimental` 作为兼容别名保留；分布式经济结算与 spec migration 仍不纳入稳定承诺。

---

## Evidence

### 当前 feature gate

`crates/oris-runtime/Cargo.toml` 中的实验 feature：

| Feature | 当前依赖 | 观察 |
|---|---|---|
| `mcp-bootstrap` | `mcp-experimental` | 标准 bootstrap/capability discovery 入口；旧名保留兼容 |
| `evokernel-facade` | `dep:oris-evokernel` | 内部 facade，不直接面向用户语义 |
| `evolution-experimental` | `evokernel-facade` | 暴露 `oris_runtime::evolution` |
| `governor-experimental` | `evokernel-facade` | 暴露 `oris_runtime::governor` |
| `evolution-network-experimental` | `evokernel-facade`, `evolution-experimental` | 暴露 OEN / A2A evolution routes |
| `economics` | `economics-experimental` | 标准本地 EVU/reputation baseline |
| `spec-contract` | `spec-experimental` | 标准 OUSL YAML compiler baseline |
| `economics-experimental` | `evokernel-facade` | 旧兼容入口 |
| `spec-experimental` | `evokernel-facade` | 旧兼容入口 |
| `agent-contract-experimental` | `evokernel-facade` | A2A/proposal contracts |
| `full-evolution-experimental` | aggregate | 示例和集成测试使用的全量实验 bundle |

`a2a-production` 已是稳定 feature，但仍依赖两个实验 feature 名称：

```toml
a2a-production = [
    "execution-server",
    "agent-contract-experimental",
    "evolution-network-experimental",
]
```

### 当前稳定边界

`docs/evolution-boundary.md` 已明确：

- `a2a-production` 是生产兼容 A2A workflows 的稳定入口。
- 稳定路由包括 `/a2a/hello`、`/a2a/fetch`、`/a2a/tasks/*`、`/a2a/task/*`、`/a2a/work/*`、`/a2a/heartbeat`。
- `/v1/evolution/publish`、`/v1/evolution/fetch`、`/v1/evolution/revoke`、`/v1/evolution/a2a/*` 等仍在 experimental boundary。

这说明 A2A 稳定路线已经存在，但 feature 命名和 facade 依赖还没有标准化。

### 测试信号

`RELEASE.md` 中多次记录 `a2a-production` 测试通过，包括：

- `a2a_service_`
- `a2a_bid_`
- `a2a_dispute_rule`
- `a2a_project_`
- `a2a_council_`
- `a2a_task_`

`crates/oris-runtime/tests/evolution_feature_wiring.rs`、`agent_self_evolution_travel_network.rs`、`agent_official_experience_reuse.rs` 仍依赖 `full-evolution-experimental`。

### 文档信号

README 和 `docs/evokernel/README.md` 仍将以下能力标注为 experimental 或 in progress：

- Evolution
- Sandbox
- EvoKernel
- Governor
- Evolution Network
- Economics
- Spec
- Agent Contract
- Full stack facade

其中 Evolution/Governor/Agent Contract/Network 已有足够实现基线，但不等于全量 API 可稳定。

---

## 标准化候选分级

### P0：先标准化 A2A production 依赖的 contract/network 子集

**目标**: 消除稳定能力依赖实验 feature 名称的矛盾。

第一批采用的标准 feature：

```toml
evolution = ["evolution-experimental"]
governor = ["governor-experimental"]
agent-contract = ["agent-contract-experimental"]
evolution-network = ["evolution-network-experimental"]
a2a-production = [
    "execution-server",
    "agent-contract",
    "evolution-network",
]
```

兼容策略：

```toml
evolution-experimental = ["evokernel-facade"]
governor-experimental = ["evokernel-facade"]
agent-contract-experimental = ["evokernel-facade"]
evolution-network-experimental = ["evokernel-facade", "evolution-experimental"]
```

这样保留现有大量 `cfg(feature = "...-experimental")` 编译条件，避免一次迁移引入大面积条件编译风险；对外则提供标准 feature 名称。

但实现时不能一次性把所有 `/v1/evolution/*` routes 都宣称 stable。需要在代码层继续区分：

- Stable route group: `/a2a/*`
- Experimental route group: `/v1/evolution/*`, `/evolution/a2a/*`

**为什么优先**:

- `a2a-production` 已是标准入口。
- RELEASE 中已有多组 A2A 测试证据。
- 改动可以做到 backward-compatible。

### P1：标准化核心 evolution facade 的 supervised subset

**目标**: 将 `oris_runtime::evolution` 中已证明的 supervised replay/capture API 标准化。

候选范围：

- proposal-driven mutation capture
- replay-first candidate lookup
- validation-plan based verification
- JSONL evolution store
- confidence lifecycle APIs that已有测试覆盖

不纳入首批标准范围：

- always-on autonomous release
- remote network exchange
- economics / reputation
- spec compiler

准入条件：

- `oris-evolution` 公开 API 文档补全。
- `cargo test -p oris-evolution` 作为独立 release gate。
- `oris-runtime` 中新增 `evolution` 标准 feature。
- 示例从 `full-evolution-experimental` 拆出一个 `evolution` only quickstart。

### P2：标准化 governor policy-only 子集

**目标**: 标准化 promotion/cooldown/revocation 的纯策略层，不标准化自动执行。

准入条件：

- 定义稳定 policy input/output schema。
- 加入最小 backwards compatibility tests。
- 文档明确 governor 只做 policy decision，不做 autonomous mutation。

### P3：继续实验隔离

这些能力暂不转标准：

| 能力 | 原因 |
|---|---|
| `release-automation-experimental` | 涉及自动发布，高风险，需安全/权限/回滚证明 |
| `full-evolution-experimental` | 聚合过宽，不能整体标准化 |
| evolution-network 宽路由 | `/v1/evolution/*`、`/evolution/a2a/*`、session replication/lifecycle 属于网络编排扩展，不能等同于稳定 `/a2a/*` |
| 完整 MCP 协议桥 | JSON-RPC transport、session lifecycle、tool invocation、auth/session identity 尚未形成稳定契约 |
| distributed economics | 跨节点 EVU 结算、一致性和最终性尚未冻结 |
| spec migration | OUSL 版本迁移和兼容策略尚未冻结 |

---

## 推荐迁移计划

### Story 1：Feature alias 标准化

交付：

- 新增标准 feature aliases：
  - `evolution`
  - `governor`
  - `evolution-network`
  - `agent-contract`
- 保留旧 `*-experimental` feature，指向新标准 feature。
- `a2a-production` 改依赖标准 feature。
- README 与 `docs/evolution-boundary.md` 更新 feature guide。

验证：

```bash
cargo fmt --all -- --check
cargo test -p oris-runtime --features "sqlite-persistence,execution-server,a2a-production" a2a_
cargo test -p oris-runtime --test evolution_feature_wiring --features full-evolution-experimental
cargo build --all --release --all-features
```

风险：

- Cargo feature 名称新增后，docs.rs all-features surface 变大但不破坏旧用户。
- 必须保留旧 feature 至少一个 minor cycle。

### Story 2：Stable A2A route contract audit

交付：

- 把 `/a2a/*` stable route DTO 独立成 contract doc。
- 增加 route availability test：启用 `a2a-production` 时暴露 stable routes，不暴露 evolution experimental routes。
- 明确 `/v1/evolution/*` 仍 experimental。

验证：

```bash
cargo test -p oris-runtime --features "sqlite-persistence,execution-server,a2a-production" a2a_
```

### Story 3：Core evolution supervised API standardization

交付：

- 标准 feature `evolution` 只承诺 supervised capture/replay subset。
- 新增 `docs/evolution-stable-api.md`。
- 新增 `crates/oris-runtime/examples/evolution_supervised_quickstart.rs`，仅依赖 `evolution`。
- 更新 README Quick Start，从 `full-evolution-experimental` 降级为标准 quickstart，复杂 demo 仍保留 experimental。

验证：

```bash
cargo test -p oris-evolution
cargo test -p oris-runtime --features evolution evolution_
cargo run -p oris-runtime --example evolution_supervised_quickstart --features evolution
```

### Story 4：Governor policy-only standardization

交付：

- 标准 feature `governor`。
- 明确 policy-only contract。
- 保留所有自动执行或 release automation 相关能力在 experimental。

验证：

```bash
cargo test -p oris-governor
cargo test -p oris-runtime --features governor governor_
```

### Story 5：Deprecation window

交付：

- 文档标注 `*-experimental` feature 是兼容别名。
- 新示例全部使用标准 feature。
- 旧示例在一个 minor 版本周期后迁移或双栈支持。

---

## 不建议立即做的事

| 不建议 | 原因 |
|---|---|
| 直接把 `full-evolution-experimental` 改名为 `full-evolution` | 聚合过宽，包含 economics/spec/network/release 等未稳定能力 |
| 直接删掉 `*-experimental` feature | 会破坏已有用户、示例和 CI |
| 把 `/v1/evolution/*` 全部纳入 `a2a-production` | 当前边界文档明确这些仍 experimental |
| 宣称 autonomous self-evolution 已稳定 | 与 README 的 supervised/bounded 表述冲突 |

---

## 决策建议

优先执行 Story 1 + Story 2。它们能把已经稳定的 `a2a-production` 能力从实验命名中解耦，风险最低、收益最大。

Story 3/4 需要逐个 API 审查 public surface，再做代码迁移。Story 5 是发布治理，不应跳过。
