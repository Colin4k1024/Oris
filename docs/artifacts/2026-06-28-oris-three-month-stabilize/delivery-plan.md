# Delivery Plan: Oris 三个月稳定化计划

> 日期：2026-06-28
> Issue：#451
> 主责：tech-lead
> 状态：plan (handoff-ready)

---

## 一、需求挑战会结论

### Challenge Session 参与者

| 角色 | 挑战重点 |
|------|---------|
| architect | 计划结构、风险排序、vertical slice 替代方案 |
| backend-engineer | 实现可行性、已完成工作识别、scope 修正 |

### 核心假设验证结果

| 假设 (Issue #451) | 验证结果 | 影响 |
|-------------------|---------|------|
| 生产路径 unwrap 2,038 个 | **实际 257 个**（其余全在 `#[cfg(test)]` / `mod tests`） | W1 工作量缩减 60% |
| oris-evokernel 有 155 个生产 unwrap | **0 个**（`core.rs` 全部在 line 9085+ 的 test module） | W1 不触及 evokernel |
| Postgres backend 有大量 TODO | **Kernel 层已完成** (785 行, 8 集成测试全过)；Graph 层缺 `PostgresCheckpointer` | W2 从 Large → 3 天 |
| CI failure parser (W6) 需新建 | **已完成** (437 行, 9 测试, `CiIntakeSource` 实现 trait) | W6 **删除** |
| Confidence decay daemon (W7) 需新建 | **已完成** (442 行, 6 测试, `spawn()` + 生命周期管理) | W7 **删除** |
| api_handlers.rs 有 1,373 个 unwrap | **全在 `mod tests` 块内** (line 10969+) | 不影响生产 |

### 挑战质疑与决策

| # | 质疑 | 提出方 | 决策 |
|---|------|--------|------|
| 1 | unwrap 清理应按稳定性分层，experimental-gated 代码优先级低 | architect | **接受** — 优先清理 stable path (scheduler 7个, kernel stores) |
| 2 | W2 scope 不确定需要 spike | architect | **接受** — 验证确认 kernel 已完成，仅需 graph-layer adapter (3天) |
| 3 | W7/W6 已完成应删除 | backend-engineer | **接受** — 代码验证确认两个模块已生产就绪 |
| 4 | W8 应在 W1 之后而非同月 | architect | **接受** — 不应在清理未完成时提升 gate |
| 5 | 需要 full-loop 进化管线集成测试作为 v1.0 前置 | architect | **接受** — 加入为显式 release blocker |
| 6 | `chain_trait.rs:180` 的 `unimplemented!()` 是 release blocker | architect | **接受** — 加入 W1 scope |
| 7 | 建议 vertical slice 替代 horizontal layer | architect | **部分接受** — 采用混合模式：Month 1 做 vertical slice (stable path 端到端)，Month 2/3 按价值排序 |

### 计划修正摘要

- **删除 2 个工作项**：W6 (CI parser), W7 (Confidence decay) — 已实现
- **缩减 2 个工作项**：W1 (257→优先80-120个), W2 (仅 graph-layer adapter)
- **新增 2 个 release blocker**：full-loop 集成测试, `chain_trait.rs:180` 修复
- **重排序**：W8 从 Month 2 移到 Month 3 (W1 完成后)
- **净回收 ~20 人天**：重新分配到 OTel/metrics 和集成测试

---

## 二、修正后版本目标

### Phase 1 (7月 W1-W4): 稳定路径端到端硬化

**目标**: 让 stable API surface (graph + agent + tools + execution-runtime) 在压力下不 panic

**放行标准**:
- stable 路径 unwrap = 0
- `chain_trait.rs:180` resolved
- Graph-layer PostgresCheckpointer 实现 + 测试
- LLM 调试输出迁移完成
- 测试增量 ≥ 100

### Phase 2 (8月 W5-W8): 信号连通 + 观测基础

**目标**: 进化循环有真实输入源 + 运行状态可见

**放行标准**:
- hermesx ingestion 端点可用 (含 Ed25519 验签)
- OTel 追踪覆盖 Detect→Validate 5 阶段
- Prometheus 5 个核心指标上线
- Full-loop evolution 集成测试 green

### Phase 3 (9月 W9-W10): Gate 降级 + v1.0 发布

**目标**: 稳定承诺可交付，开发者可以 10 分钟入门

**放行标准**:
- experimental gate 范围收窄 (仅 A2A/OEN/PR-lane)
- Quickstart 8 阶段完整 walkthrough
- v1.0.0 publish dry-run 通过
- Stable API 文档化

---

## 三、Story Slice 列表

### Phase 1: 稳定路径硬化 (7月)

| ID | Story | 验收标准 | 依赖 | Owner | 估时 |
|----|-------|---------|------|-------|------|
| S1 | unwrap 分类 + stable-path 清理 | stable-path (scheduler, kernel stores, graph/compiled.rs) unwrap = 0; `chain_trait.rs:180` resolved | 无 | backend-engineer | 3d |
| S2 | unwrap experimental-path 清理 | evolution-network/sync (26), intake/signal (19), genestore/store (12) 清理完成 | 无 | backend-engineer | 3d |
| S3 | Graph-layer PostgresCheckpointer | `PostgresCheckpointer<S>` 实现 `put/get/list`; 3+ 集成测试; feature-gated | 无 | backend-engineer | 3d |
| S4 | LLM 调试输出迁移 | 21 处 println!/dbg! → tracing macros; 0 残留 | 无 | backend-engineer | 1d |
| S5 | 测试密度: evolution pipeline 集成测试 | 新增 full-loop Detect→Solidify integration test (≥3 场景); CI green | S1, S2 | backend-engineer | 3d |
| S6 | 测试密度: genestore + intake 补测试 | 新增 ≥20 测试覆盖 store CRUD、replay hook、intake dedup | 无 | backend-engineer | 2d |

**Phase 1 总估时: 15 人天 (3 周)**

### Phase 2: 信号连通 + 观测 (8月)

| ID | Story | 验收标准 | 依赖 | Owner | 估时 |
|----|-------|---------|------|-------|------|
| S7 | hermesx ingestion 端点 | `POST /v1/ingest/hermesx/execution-event` 可用; Ed25519 验签; 拒绝 success events; 5+ 测试 | 接口契约锁定 | backend-engineer | 5d |
| S8 | OTel tracing 集成 | Detect/Select/Mutate/Execute/Validate 5 阶段 span; Jaeger 可视化验证 | S5 (pipeline 存在) | backend-engineer | 4d |
| S9 | Prometheus metrics | 5 指标 (cycles_total, confidence_distribution, intake_queue_depth, acceptance_rate, replay_hit_rate); `/metrics` endpoint | S7, S8 | devops-engineer | 3d |
| S10 | CI intake enhancement | 补充 clippy 解析 + GitHub Actions log format; 现有 ci_parser.rs 增强 | 无 | backend-engineer | 2d |

**Phase 2 总估时: 14 人天 (3 周)**

### Phase 3: 稳定化发布 (9月)

| ID | Story | 验收标准 | 依赖 | Owner | 估时 |
|----|-------|---------|------|-------|------|
| S11 | experimental gate 分拆 | 新增 `evolution-stable` feature (EvolutionPipeline + EvoKernel 基础); `experimental` 仅含 A2A/OEN/PR-lane | S1, S2, S5 | architect | 3d |
| S12 | Quickstart tutorial 重写 | 8 阶段 walkthrough; 每步有可见 terminal 输出; 10 分钟可完成 | S8, S11 | backend-engineer | 3d |
| S13 | per-crate 示例 + stable API 文档 | genestore/intake/governor 各 1 example; stable surface doc coverage ≥ 80% | S11 | backend-engineer | 3d |
| S14 | v1.0.0 发布准备 | CHANGELOG 更新; publish dry-run 通过; stable vs experimental 边界文档; tag + release | S11, S12, S13 | tech-lead | 2d |

**Phase 3 总估时: 11 人天 (2.5 周)**

---

## 四、Brownfield 上下文快照

### 现有模块边界

```
Stable Path (v1.0 候选):
├── oris-kernel (93 tests, 0 prod unwraps, Postgres complete)
├── oris-execution-runtime (scheduler: 7 unwraps)
├── oris-runtime/graph (75 total, ~4 prod unwraps after test exclusion)
├── oris-runtime/agent (stable API)
└── oris-runtime/tools (stable API)

Experimental Path (保持 gated):
├── oris-evolution (pipeline: 10 unwraps, evolver: 8)
├── oris-evokernel (0 prod unwraps, confidence_daemon complete)
├── oris-evolution-network (sync: 26, gossip: 12)
├── oris-intake (signal: 19, prioritize: 8; ci_parser complete)
├── oris-genestore (store: 12, replay_hook: 5)
├── oris-economics
├── oris-spec
└── oris-orchestrator
```

### 已有错误类型体系

| Crate | Error Type | 覆盖范围 |
|-------|-----------|---------|
| oris-runtime | GraphError, PersistenceError, TaskError, InterruptError | graph 全路径 |
| oris-evolution | EvolutionError, PipelineError | 进化核心 |
| oris-evokernel | EvoKernelError, ValidationError, ReplayError | kernel 操作 |
| oris-intake | IntakeError (in lib.rs) | intake 链路 |
| oris-evolution-network | SigningError (signing.rs) | 网络层 |

结论：错误类型基础设施充足，unwrap 清理主要是 `Result` 传播 + `map_err` 转换，不需要新增错误类型。

### 外部依赖

| 依赖 | 状态 | 风险 |
|------|------|------|
| hermesx ExecutionReceipt 格式 | 未锁定 | S7 需要 mock-first 开发 |
| sqlx (Postgres) | 已集成 | 低 |
| opentelemetry crate | 未引入 | 低 (成熟生态) |
| prometheus crate | 未引入 | 低 |

---

## 五、角色分工

| 角色 | 主责 Story | 交接顺序 |
|------|-----------|---------|
| tech-lead | 整体协调, S14 发布决策 | intake → plan → 监督 → release |
| architect | S11 gate 分拆设计, 错误策略 review | plan → S11 → S14 review |
| backend-engineer | S1-S10, S12-S13 全部实现 | plan → execute → review |
| devops-engineer | S9 metrics 基础设施 | S8 完成后 → S9 |
| qa-engineer | Phase 1/2 exit criteria 验收 | 每 phase 末验收 |

---

## 六、风险与依赖清单

| # | 风险 | 概率 | 影响 | 缓解 | Owner |
|---|------|------|------|------|-------|
| R1 | unwrap 清理引入行为回归 | 中 | 高 | 先 S5/S6 补测试，再清理；每批后全量 `cargo test` | backend-engineer |
| R2 | hermesx 接口契约 Month 2 前未锁定 | 中 | S7 延期 | Mock-first 开发 + trait 抽象 | tech-lead |
| R3 | `chain_trait.rs:180` unimplemented 暴露深层设计问题 | 低 | 高 | S1 内前置调查，若 dead code 直接移除 | backend-engineer |
| R4 | experimental gate 分拆导致编译错误 | 低 | 中 | 渐进式：先添加新 feature，再从 experimental 移除 | architect |
| R5 | OTel 引入增加编译时间 / 二进制体积 | 低 | 低 | Feature-gated (opt-in) | backend-engineer |

---

## 七、Release Blockers (v1.0.0 前置)

- [ ] `chain_trait.rs:180` — `unimplemented!()` resolved (dead code removal 或实现)
- [ ] Full-loop evolution pipeline integration test in CI
- [ ] Stable-path production unwrap = 0
- [ ] `cargo publish --dry-run --all-features` 通过
- [ ] Stable vs experimental boundary 文档化

---

## 八、应用等级 / 技术架构 / ADR

- **应用等级**: 不适用（开源框架）
- **技术架构等级**: 不适用
- **关键组件偏离**: 无
- **ADR 需求**: 建议在 S11 (gate 分拆) 产出 ADR，记录 stable/experimental 边界决策

---

## 九、技能装配清单

| 技能 | 启用原因 | 主责角色 |
|------|---------|---------|
| oris-maintainer | 全程：issue workflow, validation checklist | backend-engineer |
| rust-patterns | S1/S2: 错误处理模式 | backend-engineer |
| rust-testing | S5/S6: 测试结构 | backend-engineer |
| rust-build | feature flag 管理, publish | tech-lead |

---

## 十、Implementation Readiness 结论

### 准入检查

| 检查项 | 状态 | 证据 |
|--------|------|------|
| Challenge session 完成 | ✅ | architect + backend-engineer 双方挑战 |
| 核心假设已验证 | ✅ | 7 项假设全部用代码验证 |
| Scope 修正完成 | ✅ | 删除 2 项、缩减 2 项、新增 2 blockers |
| 依赖明确 | ✅ | 唯一外部依赖 (hermesx) 有 mock-first 缓解 |
| brownfield 调研完成 | ✅ | 错误类型、unwrap 分布、已有实现全部确认 |

### 就绪状态: `handoff-ready`

执行前提：
1. 确认 hermesx 侧是否能在 8 月前锁定 ExecutionReceipt 格式（影响 S7 是否需要 mock-first）
2. Tech-lead 裁定 Postgres 是否为 v1.0 stable backend（当前结论：kernel 已完成，graph-layer adapter 作为 opt-in feature，不阻塞 v1.0）
3. 确认 `chain_trait.rs:180` 的代码路径是否可达（若 dead code 则直接移除）

---

## 十一、时间线总览

```
7月 (Phase 1: 稳定路径硬化)               8月 (Phase 2: 信号+观测)           9月 (Phase 3: v1.0)
│                                          │                                  │
├─ S1: stable-path unwrap (3d)             ├─ S7: hermesx ingestion (5d)     ├─ S11: gate 分拆 (3d)
├─ S2: experimental unwrap (3d)            ├─ S8: OTel tracing (4d)          ├─ S12: quickstart (3d)
├─ S3: PostgresCheckpointer (3d)           ├─ S9: Prometheus metrics (3d)    ├─ S13: examples+docs (3d)
├─ S4: LLM debug migrate (1d)             └─ S10: CI parser enhance (2d)    └─ S14: v1.0.0 release (2d)
├─ S5: evolution integ test (3d)
└─ S6: genestore+intake tests (2d)

总计: 40 人天 (原计划 ~60 人天, 节省 33%)
```
