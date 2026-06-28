# PRD: Oris 三个月稳定化计划

> Intake 日期：2026-06-28
> Issue：#451
> 主责：tech-lead
> 状态：plan (handoff-ready)

---

## 一、背景

### 业务问题

Oris 作为自进化执行运行时，在架构层面（18 crate / 185K LOC / 清洁 DAG）已经成熟，但存在以下根基性问题阻碍其走向生产可用：

1. **生产路径 unwrap 密度危险** — oris-runtime 1,868 处、oris-evokernel 170 处 unwrap 在非测试路径，任一 panic 导致 session 崩溃而非优雅降级
2. **测试密度不足** — 1,724 个测试覆盖 185K LOC，关键 crate（evokernel 10 tests / 18K LOC）覆盖率极低
3. **hermesx ↔ Oris 信号通路断开** — 进化循环第一步 Detect 缺乏自动化信号摄入，只能 caller 手动提供
4. **核心价值锁在 experimental gate 后面** — `full-evolution-experimental` 的命名让严肃集成方无法基于稳定承诺构建
5. **缺乏可观测性** — 无 OTel 追踪、无 Prometheus metrics、进化循环运行状态不可见

### 触发原因

Q1 2026 audit 已识别上述问题但未提为 P0 执行。v0.54→v0.61 的七个版本全部用于 A2A 协议扩展，核心可靠性问题持续累积。

### 当前约束

- 零外部用户（0 stars / 1 fork），扩展协议面无验证基础
- hermesx（L1）计划 Month 1 实现 ExecutionReceipt 写入，Oris 需在 Month 2 提供摄取端
- SQLite 为当前唯一可用 backend，Postgres backend 状态待确认

---

## 二、目标与成功标准

### 业务目标

将 Oris 从"功能原型"推进到"生产可部署、API 可承诺、行为可观测"状态，为 v1.0.0 发布奠定基础。

### 用户价值

- 框架使用者可基于稳定 API 构建，不担心 panic 崩溃
- 进化循环可从 CI 和 hermesx 自动获取信号，实现真正自主性
- 运维团队可通过标准观测工具监控进化循环健康度

### 成功指标

| 指标 | 当前值 | Month 1 目标 | Month 3 目标 |
|------|--------|-------------|-------------|
| 生产路径 unwrap() | 2,038 | 0 | 0 |
| 总测试数 | 1,724 | 2,100+ | 2,500+ |
| LLM 模块 println!/dbg! | 21 | 0 | 0 |
| hermesx 信号摄入 | 不存在 | — | 上线+集成测试 |
| OTel 追踪覆盖 | 0 阶段 | — | 5/8 阶段 |
| Prometheus 指标 | 0 | — | 5 核心指标 |
| experimental gate 范围 | 全部进化功能 | — | 仅 A2A/OEN/PR-lane |

---

## 三、用户故事

### US-1: 生产可靠性（Month 1）

**作为** Oris 框架的集成开发者
**我希望** 在任何执行路径上遇到错误时得到 Result 返回而非 panic
**以便** 我的应用可以优雅降级而不是崩溃

**验收标准：**
- `grep -r "unwrap()" --include="*.rs" crates/ | grep -v test | grep -v "#\[cfg(test)" | wc -l` 输出 0
- `cargo test --release --all-features` 全部通过，测试数 ≥ 2,100
- LLM 模块无 println!/dbg! 残留

### US-2: 自动信号摄入（Month 2）

**作为** 运行 Oris 进化循环的 DevOps 工程师
**我希望** CI 测试失败和 hermesx 执行异常能自动进入 intake 队列
**以便** 进化循环无需人工干预即可开始 Detect

**验收标准：**
- `POST /v1/ingest/hermesx/execution-event` 端点可用，有 Ed25519 验签
- CI failure parser 正确解析 `cargo test` / `rustc` / `clippy` 输出
- Confidence decay daemon 通过 24h 模拟验证

### US-3: 可观测与稳定发布（Month 3）

**作为** Oris 的潜在采用者
**我希望** 有清晰的稳定 API 边界、可视化的运行追踪和完整的入门教程
**以便** 我可以基于稳定承诺评估和集成 Oris

**验收标准：**
- OTel 追踪覆盖 Detect→Validate 5 阶段，可在 Jaeger 可视化
- `/metrics` 暴露 5 个核心业务指标
- Quickstart tutorial 走通 8 阶段完整循环
- v1.0.0 发布，stable vs experimental 边界文档化

---

## 四、范围

### In Scope

| Month | 工作项 | 优先级 |
|-------|--------|--------|
| 1 | W1: unwrap() 系统性清理（evokernel → runtime/graph → runtime/agent → runtime/llm） | P0 |
| 1 | W2: Postgres backend parity（如确认为 v1.0 目标） | P0/P1 |
| 1 | W3: LLM 调试输出迁移 + deprecated 清理 | P1 |
| 1 | W4: 测试密度提升（evokernel、evolution、genestore 优先） | P1 |
| 2 | W5: hermesx ExecutionReceipt 摄取端点 | P0 |
| 2 | W6: CI failure intake parser | P1 |
| 2 | W7: Confidence decay daemon | P1 |
| 2 | W8: experimental gate 范围收窄 | P2 |
| 3 | W9: OpenTelemetry 全链路追踪 | P1 |
| 3 | W10: Prometheus metrics endpoint | P1 |
| 3 | W11: Quickstart tutorial 重写 | P2 |
| 3 | W12: v1.0.0 稳定化声明 + per-crate 示例 | P2 |

### Out of Scope

- A2A 协议新增 endpoints（Council/bid/project/economic lifecycle 已足够，暂停扩展）
- embedding-based task-class matching（C-3，gene pool 数据不足，Q4 话题）
- bounded PR lane（C-5，前置依赖未完成，风险过高）
- hermesx L2 governance client 消费（依赖 hermesx 侧进度）
- 多租户 / 多集群部署模式

---

## 五、风险与依赖

### 关键依赖

| 依赖 | 来源 | 影响 | 缓解 |
|------|------|------|------|
| hermesx ExecutionReceipt 格式 | hermesx 项目 Month 1 | W5 无法开始 | Month 1 先定义接口契约，Month 2 对接 |
| Postgres backend 完成度 | 当前代码状态 | 决定是否为 v1.0 stable backend | 需决策：SQLite-only for v1.0 可接受? |
| oris-experience-repo Ed25519 基础设施 | 已有 v0.3.0 | W5 验签依赖 | 低风险，已验证 |

### 风险

| 风险 | 概率 | 影响 | 缓解 |
|------|------|------|------|
| unwrap 清理引入行为回归 | 中 | 高 | 每批清理后全量测试，先补测试再改 unwrap |
| hermesx 信号格式 Month 2 前未锁定 | 中 | W5 延期 | 先基于 mock event 开发，真实对接在格式锁定后 |
| 测试密度提升暴露隐藏 bug | 高 | 中 | 按模块渐进，发现 bug 记为新 issue |
| v1.0 稳定承诺范围过大 | 低 | 高 | 严格限制 stable surface 为 graph + agent + tools |

### 待确认项

1. **Postgres 是否为 v1.0.0 stable backend?** — 若是，W2 必须 Month 1 完成；若 SQLite-only 可接受，W2 可推后
2. **oris-experience-repo 是否吸收回 oris-runtime?** — 影响 W5 的架构选择
3. **Oris 是否对自身代码库运行进化循环?** — Month 2 末需决策，影响 v1.0 feature set
4. **hermesx ExecutionReceipt 的字段契约** — Month 1 内需与 hermesx 侧对齐

---

## 六、参与角色清单

| 角色 | 职责 | 输入缺口 |
|------|------|---------|
| `tech-lead` | 整体协调、优先级仲裁、决策 1-3 裁定、v1.0 放行 | 无 |
| `architect` | unwrap 清理策略、错误类型体系设计、OTel 集成方案 | 需确认现有 OrisError 类型是否足够 |
| `backend-engineer` | W1-W7 核心实现、测试编写 | 需 hermesx 侧接口契约 |
| `qa-engineer` | 测试密度验证、回归策略、Month 1 exit criteria 验收 | 需确认测试基线统计口径 |
| `devops-engineer` | W9/W10 观测基础设施、CI intake workflow | 需确认目标部署环境 |

---

## 七、企业治理待确认项

- **不适用** — Oris 为开源框架项目，不属于企业内部应用，无应用等级 / 数据合规 / 集团组件约束

---

## 八、领域技能包启用建议

| 技能包 | 启用原因 |
|--------|---------|
| `oris-maintainer` | 全程适用：issue-driven workflow、validation checklist |
| `rust-patterns` | W1 unwrap 清理的错误处理模式参考 |
| `rust-testing` | W4 测试密度提升的结构参考 |
| `rust-build` | 构建验证、feature flag 管理 |

---

## 九、UI 范围

- **不涉及前端变更** — 纯 Rust backend/library 工作，无 UI 门禁要求

---

## 十、需求挑战会候选分组

鉴于本 issue 跨三个月、12 个工作项，建议分两轮 challenge：

### Round 1: 基础架构决策（建议 intake 后立即）

**参与者：** tech-lead + architect + backend-engineer

**挑战议题：**
1. unwrap 清理策略：逐文件 vs 逐模式 vs 引入统一错误类型？
2. Postgres parity 是否为 v1.0 gate？SQLite-only 的 tradeoff？
3. experimental gate 分拆边界：哪些 API 进入 stable？

### Round 2: 信号连通方案（Month 1 末）

**参与者：** tech-lead + architect + backend-engineer + devops-engineer

**挑战议题：**
1. hermesx ingestion 接口契约：同步 vs 异步？验签粒度？
2. CI failure parser 的解析精度 vs 误报 tradeoff
3. Confidence decay 参数（衰减率、revalidation 阈值）如何校准？

---

## 十一、关键假设（Karpathy Guidelines）

1. **假设现有 1,724 个测试全部有效** — 需验证是否有 flaky test 或无效断言
2. **假设 oris-evokernel 的 170 个 unwrap 大部分是 Mutex::lock()** — 需验证实际分布
3. **假设 hermesx Month 1 能锁定 ExecutionReceipt 格式** — 若延期，W5 需基于 mock 先行
4. **假设 stable API surface = graph + agent + tools** — 需 architect 确认边界
5. **假设 Postgres TODO 已清零** — 当前 grep 显示 0，但需验证功能完整性而非仅代码标记

### 最小可行范围

如果三个月只能完成一件事：**Month 1 的 unwrap 清理 + 测试密度提升**。这是所有后续工作的信任基础。

### 非目标

- 不追求 100% 测试覆盖率，目标是关键路径覆盖
- 不重写现有 A2A 协议实现
- 不引入新的 crate 或重大架构变更
- 不做性能优化（除非发现阻塞性问题）
