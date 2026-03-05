# Oris vs EvoMap 实现差异与拉齐设计

Date: 2026-03-05  
Status: Draft for review

## 1. 目标

对比当前 Oris（仓库实现）与 `evomap.ai`（公开协议与 `evolver` 开源实现）差异，给出可执行的拉齐设计，目标是：

- 对外 A2A/GEP 协议兼容度提升
- 任务分发与回放反馈语义对齐
- 在不削弱 Oris 现有治理与安全能力前提下完成对齐

## 2. 基线快照（截至 2026-03-05）

### 2.1 Oris（本仓库）

- 当前状态明确为“`constrained replay-driven self-evolution`”，尚非闭环自治系统。
- 已有能力：回放优先、治理决策、远端资产隔离再验证、A2A 会话与复制路由、受监督 devloop。
- 明确边界：未内建自动 issue intake / 自动分支-PR-发布编排。

主要证据：

- `docs/evokernel/implementation-roadmap.md`
- `docs/evokernel/README.md`
- `crates/oris-evokernel/src/core.rs`
- `crates/oris-runtime/src/execution_server/api_handlers.rs`

### 2.2 EvoMap（公开资料）

- 官方文档公开了 GEP/A2A 架构、消息类型与端点（含 `hello/distribute/claim/report`）。
- 文档标注协议版本为 `oris.a2a` `1.0.0`。
- 开源 `evolver` 提供 CLI 与 worker/autogen 模式，任务可由 worker claim 并上报。

主要证据：

- `https://evomap.ai/llms.txt`
- `https://evomap.ai/llms-full.txt`
- `https://github.com/autogame-17/evolver`

## 3. 差异矩阵

| 维度 | Oris 当前实现 | EvoMap 公开实现 | 差异结论 |
| --- | --- | --- | --- |
| 协议版本 | `oris.a2a` `0.1.0-experimental` | `oris.a2a` `1.0.0` | 存在版本代差，需要兼容层 |
| A2A 路由形态 | `/v1/evolution/a2a/sessions/*`（start/dispatch/progress/complete/snapshot/replicate） | `/evolution/a2a/*`（含 `hello` + `tasks/distribute/claim/report`） | 路由与语义模型不一致 |
| 任务分发模型 | 会话驱动（显式 start->dispatch->progress->complete） | 分发队列驱动（distribute->claim->report） | Oris 缺少标准 claim 队列接口 |
| 回放反馈 | `ReplayFeedback` + planner directive（Skip/PlanFallback）已落地 | 文档同样强调回放反馈字段 | 语义接近，可直接映射 |
| 资产治理 | Governor + 置信度衰减 + 隔离/撤销 + 远端声誉偏置 | 开源侧以 pipeline+promote 脚本为主，治理较轻 | Oris 更强，可作为超集保留 |
| 网络资产流 | publish/fetch/revoke + 远端隔离本地验证后释放 | publish/fetch/revoke + GEP 消息流 | 大方向一致，端点/消息需对齐 |
| 自动化闭环 | Runtime 内未内建 issue->PR->release 全流程 | `evolver` 提供 autogen/worker 流程 | 需补 orchestrator/queue 层 |

## 4. 拉齐方案候选

### 方案 A：仅 API 兼容层（最快）

做法：

- 保留 Oris 内核不动
- 增加 EvoMap 兼容路由与 payload 适配

优点：

- 交付快，风险最低

缺点：

- 仅“能对接”，任务编排行为仍不一致

### 方案 B：全量行为对齐（最重）

做法：

- 在 runtime 内直接引入 distribute/claim/report 队列与 worker 模式
- 尽量贴齐 EvoMap 工作流

优点：

- 行为最像 EvoMap

缺点：

- 改动侵入大，破坏当前职责边界风险高

### 方案 C：双层对齐（推荐）

做法：

- 层 1：协议与端点兼容（A）
- 层 2：新增独立 orchestrator/queue 组件承接 claim/report（不污染 EvoKernel 内核）

优点：

- 兼顾兼容度、演进速度和架构稳定性

缺点：

- 需要多一个组件和部署面

## 5. 推荐设计（方案 C）

## 5.1 设计原则

- 内核不降级：保留 Oris 现有 governor / quarantine / revoke / metrics 强能力
- 协议先行：先实现 `oris.a2a@1.0.0` 兼容
- 编排外置：队列与 worker 协调放到 orchestrator 层

## 5.2 合约拉齐

### 5.2.1 协议版本

- 新增 `A2A_PROTOCOL_VERSION_V1 = "1.0.0"`
- handshake 支持双栈协商：`0.1.0-experimental` 与 `1.0.0`
- 出参回传 `negotiated_protocol`

### 5.2.2 端点兼容

新增 EvoMap 兼容路由（保持现有 `/v1/...` 不变）：

- `POST /evolution/a2a/hello` -> 映射到现有 handshake
- `POST /evolution/a2a/tasks/distribute` -> 转写为 session start + dispatch
- `POST /evolution/a2a/tasks/claim` -> 读取 orchestrator queue claim
- `POST /evolution/a2a/tasks/report` -> 转写为 progress/complete

保留现有路由：

- `/v1/evolution/a2a/sessions/*` 继续作为原生 API

## 5.3 控制平面拉齐（新增组件）

新增 `a2a-task-queue`（可先内存版，后续 SQLite）：

- `distribute(task)`：写入待 claim 队列
- `claim(worker_id)`：原子领取 + lease
- `report(task_id, status)`：写回生命周期并驱动 session 更新

Orchestrator 负责：

- 任务分发策略（优先级/重试/超时）
- worker lease 续约与抢占恢复
- 与 EvoKernel 的执行结果对账

## 5.4 演化资产与治理映射

- `report: success` -> 触发 `capture_from_proposal` / `capture_mutation_with_governor`
- `report: failed` -> 触发验证失败事件与回退
- 远端导入仍保持“先隔离，回放成功后释放”的 Oris 策略（不降级）

## 5.5 安全与权限

- 继续使用现有 capability + privilege profile
- 将 `tasks/claim` 归属为 `Coordination` 能力
- 所有兼容端点写入统一审计日志（含 sender_id、actor、request_id）

## 5.6 可观测性

新增指标：

- `oris_a2a_task_queue_depth`
- `oris_a2a_task_claim_latency_ms`
- `oris_a2a_task_lease_expired_total`
- `oris_a2a_report_to_capture_latency_ms`

复用现有演化指标：

- `confidence_revalidations_total`
- promote/revoke 相关指标

## 6. 分阶段落地

### Phase 1（1-2 周）：协议兼容

- 双版本握手
- `hello/distribute/report` 兼容端点（claim 暂由 stub 返回 not-ready）
- 回归：现有 `/v1` API 全量不回归

### Phase 2（2-3 周）：队列与 claim

- 落地 `a2a-task-queue` + lease + 重试
- 打通 `claim -> report -> session lifecycle`

### Phase 3（1-2 周）：治理与发布强化

- 兼容路径纳入权限审计
- 端到端回放反馈一致性测试
- 灰度开关与回滚文档

## 7. 验收标准

- EvoMap worker 可通过兼容端点完成 `distribute -> claim -> report` 闭环
- Oris 原生 `/v1/evolution/a2a/sessions/*` 行为无回归
- 远端资产仍满足“隔离 -> 本地验证 -> 释放”流程
- 回放反馈字段（used_capsule/planner_directive/task_class）跨两套端点一致

## 8. 风险与缓解

- 风险：协议兼容导致历史客户端行为漂移  
  缓解：双栈协商 + endpoint feature flag

- 风险：新增队列破坏 runtime 职责边界  
  缓解：queue/orchestrator 外置，runtime 仅执行与记录

- 风险：权限模型被兼容端点绕过  
  缓解：统一走 `ensure_a2a_authorized_action` 与审计日志

## 9. 参考链接

- https://evomap.ai/llms.txt
- https://evomap.ai/llms-full.txt
- https://github.com/autogame-17/evolver
- https://github.com/autogame-17/evolver/blob/main/README.md

