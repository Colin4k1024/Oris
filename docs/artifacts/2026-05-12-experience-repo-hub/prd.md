# PRD: Oris Experience Repository Hub

## 背景

### 业务问题

当前 `oris-experience-repo` 是一个单实例 HTTP API，只能服务本地节点的 Gene/Capsule 查询与贡献。在多节点、多组织场景下：

1. **孤岛问题**：各节点的经验库相互隔离，无法发现和复用其他节点已验证的高质量 Gene。
2. **手动管理**：缺少可视化工具来观察 Gene 的健康状态、传播路径和使用频率。
3. **扩展瓶颈**：单实例无法满足跨地域、跨团队的经验共享需求。

### 触发原因

- `exp-repo-evokernel-wire` 已完成集成胶水（579 tests 通过），具备跨节点推送基础。
- OEN 网络层（`oris-evolution-network`）已提供 Ed25519 签名和 gossip 同步能力。
- 社区和内部对"经验市场"能力有明确期望。

### 当前约束

- P0 问题（空签名、sender_id 语义、Key 管理端点鉴权）须在 v0.4.0 修复后才能开放网络推送。
- 本 Hub 能力属于 v0.5.0 里程碑，依赖 v0.4.0 P0 修复完成。
- 前端 Dashboard 可选独立部署或 embed 到 experience-repo 进程内。

---

## 目标与成功标准

### 业务目标

- 让任意 Oris 节点可以**自动发现并连接**其他经验仓库实例。
- 提供**联邦聚合查询**能力，跨多个经验仓库搜索最优 Gene/Capsule。
- 提供**可视化 Dashboard**，降低运维和洞察门槛。

### 用户价值

| 用户 | 价值 |
|------|------|
| 节点运维者 | 无需手动配置对等节点，自动注册发现 |
| 开发者 | 跨组织复用高质量 Gene，减少重复进化开销 |
| 管理者/观察者 | 通过 Dashboard 直观了解经验流转状态 |

### 成功指标

| 指标 | 目标 |
|------|------|
| 节点注册到 Hub 的延迟 | < 5s（局域网）/ < 30s（广域网） |
| 联邦查询响应时间 | P95 < 500ms（覆盖 5 个下游节点） |
| Dashboard 首屏加载 | < 2s |
| 注册发现零配置率 | 本地网络 100% 自动发现 |

---

## 用户故事

### US-1: 节点注册

> 作为 Oris 节点运维者，我希望启动 experience-repo 时自动注册到 Hub，以便其他节点可以发现我的经验库。

**验收标准**：
- 节点启动时携带 Ed25519 公钥和元数据向 Hub 发送注册请求。
- Hub 验证签名后记录节点信息（endpoint, capabilities, version）。
- 注册信息带 TTL，节点定期心跳续约，超时自动摘除。

### US-2: 节点发现

> 作为 Oris 节点，我希望通过 Hub 发现其他活跃的经验仓库实例，以便向它们查询 Gene。

**验收标准**：
- 提供 `GET /hub/nodes` 接口返回活跃节点列表。
- 支持按 capabilities、version、region 过滤。
- Client SDK 提供 `discover_peers()` 方法，返回连接就绪的 peer 列表。

### US-3: 联邦聚合查询

> 作为开发者，我希望通过 Hub 的单一接口搜索所有已注册节点的 Gene/Capsule，找到最匹配的经验。

**验收标准**：
- `POST /hub/search` 接口将查询分发到所有活跃节点，聚合结果按 confidence 排序返回。
- 支持超时和降级：单个节点响应超时不影响整体结果。
- 结果包含来源节点标识，支持后续直连获取。

### US-4: Gene 订阅与推送

> 作为节点运维者，我希望订阅特定类型的 Gene 晋升事件，当其他节点产出高置信度 Gene 时自动接收。

**验收标准**：
- 提供 `POST /hub/subscriptions` 创建订阅（按 task_class、confidence_threshold 过滤）。
- Hub 收到 Gene 晋升事件时，向匹配订阅的节点推送通知。
- 推送使用 webhook 回调或 SSE 长连接。

### US-5: Web Dashboard - Gene 概览

> 作为管理者，我希望通过 Web 界面查看所有节点的 Gene 数量、健康状态和传播路径。

**验收标准**：
- Dashboard 首页展示：活跃节点数、总 Gene 数、最近 24h 晋升数、网络拓扑图。
- 点击节点可下钻查看该节点的 Gene 列表。
- 支持按 confidence、task_class、创建时间排序和搜索。

### US-6: Web Dashboard - Gene 详情

> 作为开发者，我希望查看某个 Gene 的完整生命周期：创建→变异→验证→晋升→传播。

**验收标准**：
- Gene 详情页展示：元数据、源代码片段、进化历史时间线、传播到哪些节点。
- 展示 confidence 变化曲线和关联的 Capsule。

### US-7: Web Dashboard - 网络健康

> 作为运维者，我希望监控 Hub 与各节点的连接健康状态。

**验收标准**：
- 节点健康面板：在线/离线/降级状态、最近心跳时间、延迟 P50/P95。
- 告警：节点持续 > 5min 无心跳时标红。

---

## 范围

### In Scope

| 领域 | 内容 |
|------|------|
| Hub 服务端 | 注册中心 API、联邦查询路由、订阅管理、健康检查 |
| Hub Client SDK | Rust crate `oris-hub-client`，封装注册、发现、订阅 |
| 联邦查询引擎 | 并行分发、超时降级、结果聚合排序 |
| Web Dashboard | SPA 前端（Gene 概览、详情、网络健康、搜索） |
| 数据持久化 | Hub 元数据使用 SQLite，可选 PostgreSQL |
| 安全 | Ed25519 节点身份验证、API Key 鉴权、TLS |

### Out of Scope

| 内容 | 原因 |
|------|------|
| Gene 的直接存储/复制 | Hub 只做元数据索引和路由，Gene 内容仍存储在各节点 |
| 付费/计量系统 | 属于 `oris-economics` 后续里程碑 |
| 多租户/ACL | v0.6.0 候选 |
| P0 问题修复（空签名、sender_id） | 属于 v0.4.0 sprint，是本 Hub 的前置依赖 |

---

## 风险与依赖

### 关键依赖

| 依赖 | 状态 | 影响 |
|------|------|------|
| v0.4.0 P0 修复（空签名 + sender_id + Key 鉴权） | 未开始 | Hub 的节点推送能力完全依赖此修复 |
| OEN 网络层 Ed25519 签名 | 已就绪 | 节点身份验证基础 |
| oris-genestore SQLite | 已就绪 | Hub 可直接复用存储层 |

### 风险

| 风险 | 概率 | 影响 | 缓解措施 |
|------|------|------|----------|
| v0.4.0 修复延迟 | 中 | Hub 无法完成 E2E 验证 | Hub API 层可先开发，推送路径留 mock |
| 联邦查询延迟不可控 | 中 | 用户体验差 | 设定超时 + 降级 + 缓存热点结果 |
| Dashboard 技术栈选择 | 低 | 维护成本 | 选择 Rust 生态友好方案（Leptos/Dioxus 或 embed static SPA） |
| 节点频繁注册/注销风暴 | 低 | Hub 压力 | Rate limit + 最小心跳间隔 |

### 待确认项

1. Dashboard 是 embed 到 Hub 进程（静态资源 serve）还是独立前端部署？
2. Hub 是否需要支持多 Hub 级联（Hub of Hubs）？当前建议 v0.5.0 只做单 Hub。
3. 联邦查询是否需要支持"只查指定节点子集"？

---

## 里程碑建议

| 阶段 | 内容 | 前置 |
|------|------|------|
| Phase 0 | v0.4.0 P0 修复 | 无 |
| Phase 1 | Hub 注册发现 API + Client SDK | Phase 0 |
| Phase 2 | 联邦聚合查询引擎 | Phase 1 |
| Phase 3 | 订阅与推送（webhook/SSE） | Phase 1 |
| Phase 4 | Web Dashboard | Phase 2 |

---

## 最后更新

2026-05-12 | tech-lead intake
