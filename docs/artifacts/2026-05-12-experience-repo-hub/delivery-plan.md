# Delivery Plan: Oris Experience Repository Hub

## 版本目标

| 字段 | 内容 |
|------|------|
| 里程碑 | oris-hub v0.1.0 (workspace 初版) |
| 目标版本 | v0.5.0 (对应 oris-runtime 里程碑) |
| 范围说明 | Hub 注册发现 + 联邦聚合查询 + 订阅推送 + Web Dashboard |
| 放行标准 | 3 节点联邦查询 E2E 通过；Dashboard 首页可访问；安全基线全覆盖 |

---

## Brownfield 上下文快照

| 现有资产 | 状态 | 与 Hub 关系 |
|----------|------|-------------|
| oris-experience-repo v0.3.0 | 已发布 | Hub 下游节点，提供被联邦查询的端点 |
| oris-evolution-network v0.5.0 | 已发布 | Ed25519 签名复用 |
| oris-genestore v0.2.0 | 已发布 | 节点端 Gene 存储 |
| oris-evokernel v0.14.1 | 已发布 | Gene 晋升事件来源 |
| P0 backlog (空签名/sender_id/Key鉴权) | 未修复 | Phase 3 订阅推送依赖 |

---

## Story Slices

### Phase 1: Hub 注册发现 (可立即启动)

| # | Story | 验收标准 | Owner | 依赖 | 估时 |
|---|-------|----------|-------|------|------|
| S1.1 | 创建 `oris-hub` crate 骨架 | cargo build 通过，模块结构就位 | backend-engineer | 无 | 2h |
| S1.2 | RegistryStore SQLite 实现 | upsert/get/list/gc CRUD 测试通过 | backend-engineer | S1.1 | 4h |
| S1.3 | RegistryService + 心跳 GC | 注册→心跳→超时摘除 单元测试 | backend-engineer | S1.2 | 4h |
| S1.4 | 注册/心跳 API handlers | `POST /hub/nodes`, `PUT /hub/nodes/{id}/heartbeat` 集成测试 | backend-engineer | S1.3 | 3h |
| S1.5 | Ed25519 签名验证中间件 | 伪造签名返回 401 | backend-engineer | S1.4 | 3h |
| S1.6 | DiscoveryService + API | `GET /hub/nodes` 按 capabilities/region 过滤 | backend-engineer | S1.4 | 3h |
| S1.7 | 创建 `oris-hub-client` crate | register + heartbeat_loop + discover 集成测试 | backend-engineer | S1.6 | 4h |
| S1.8 | Rate limit 中间件 | 超频请求返回 429 | backend-engineer | S1.4 | 2h |

**Phase 1 完成标志**：3 个 mock 节点通过 HubClient 注册/发现，GC 超时摘除正常工作。

### Phase 2: 联邦聚合查询

| # | Story | 验收标准 | Owner | 依赖 | 估时 |
|---|-------|----------|-------|------|------|
| S2.1 | FederationEngine scatter-gather | 并行请求 N 个节点，合并结果 | backend-engineer | S1.6 | 5h |
| S2.2 | 超时降级策略 | 单节点超时不影响返回，coverage 标注正确 | backend-engineer | S2.1 | 3h |
| S2.3 | 结果聚合排序 | 按 confidence desc 排序，去重（同 gene_id） | backend-engineer | S2.1 | 2h |
| S2.4 | `POST /hub/search` API | 端到端联邦查询集成测试 | backend-engineer | S2.3 | 3h |
| S2.5 | experience-repo 补充 search 端点 | `POST /experience/search` 供 Hub 调用 | backend-engineer | 无 | 3h |
| S2.6 | HubClient search 方法 | Client SDK 联邦查询封装 | backend-engineer | S2.4 | 2h |

**Phase 2 完成标志**：3 节点联邦查询 E2E，部分节点超时时降级返回正确。

### Phase 3: 订阅与推送

| # | Story | 验收标准 | Owner | 依赖 | 估时 |
|---|-------|----------|-------|------|------|
| S3.1 | SubscriptionStore SQLite 实现 | CRUD 测试通过 | backend-engineer | S1.1 | 3h |
| S3.2 | SubscriptionManager match 逻辑 | 按 task_class/confidence_threshold 匹配 | backend-engineer | S3.1 | 3h |
| S3.3 | Webhook 推送 dispatcher | 推送到 callback_url，带重试（3 次指数退避） | backend-engineer | S3.2 | 4h |
| S3.4 | Gene 晋升事件上报 API | `POST /hub/events/gene_promoted` 触发推送 | backend-engineer | S3.3 | 3h |
| S3.5 | 订阅管理 API | create/list/delete + 集成测试 | backend-engineer | S3.1 | 2h |
| S3.6 | HubClient subscribe 方法 | SDK 订阅封装 | backend-engineer | S3.5 | 2h |

**Phase 3 完成标志**：Node A 订阅 task_class=X，Node B 上报晋升事件，Node A 收到 webhook 回调。

### Phase 4: Web Dashboard

| # | Story | 验收标准 | Owner | 依赖 | 估时 |
|---|-------|----------|-------|------|------|
| S4.1 | Dashboard 项目骨架 | React + Vite + Tailwind + shadcn/ui 初始化 | frontend-engineer | 无 | 2h |
| S4.2 | Hub stats API | `GET /hub/stats` 返回节点数/Gene数/24h晋升数 | backend-engineer | S2.4 | 2h |
| S4.3 | 首页概览面板 | 显示统计卡片 + 活跃节点列表 | frontend-engineer | S4.1, S4.2 | 4h |
| S4.4 | 网络拓扑图 | 节点连接关系可视化 (react-force-graph 或简单 SVG) | frontend-engineer | S4.3 | 5h |
| S4.5 | 节点详情页 | 展示节点 Gene 列表、健康状态、心跳历史 | frontend-engineer | S4.3 | 4h |
| S4.6 | Gene 详情页 | 元数据 + 进化时间线 + confidence 曲线 | frontend-engineer | S4.5 | 5h |
| S4.7 | 搜索页面 | 联邦查询 UI + 结果列表 + 过滤器 | frontend-engineer | S2.4, S4.1 | 4h |
| S4.8 | 节点健康面板 | 在线/离线状态 + 延迟 + 告警标记 | frontend-engineer | S4.5 | 3h |
| S4.9 | embed 静态资源 serve | Hub 进程 ServeDir 嵌入 dashboard dist/ | backend-engineer | S4.1 | 2h |

**Phase 4 完成标志**：Dashboard 首页可访问，能展示节点列表、执行搜索、查看 Gene 详情。

---

## 角色分工

| 角色 | 职责 |
|------|------|
| tech-lead | 方案仲裁、Phase gate 放行、Design Review |
| architect | 架构设计、接口契约锁定 |
| backend-engineer | oris-hub + oris-hub-client 实现 |
| frontend-engineer | hub-dashboard SPA 实现 |
| qa-engineer | 集成测试、安全验证、放行建议 |

---

## 风险与缓解

| 风险 | 影响 | 缓解措施 | Owner |
|------|------|----------|-------|
| v0.4.0 P0 修复延迟 | Phase 3 订阅推送无法 E2E | Phase 1-2 先行；Phase 3 用 mock 签名开发 | tech-lead |
| 联邦查询延迟抖动 | 用户体验 | 全局 500ms 超时 + coverage 降级标注 | backend-engineer |
| Dashboard 需求膨胀 | 交付延迟 | Phase 4 只做核心 5 页面，其他留 v0.6.0 | tech-lead |
| SQLite 并发写入 | 高负载下 Hub 性能 | WAL 模式 + 写入队列 + PG 可选 | backend-engineer |

---

## 节点检查

| 节点 | 准入条件 | 检查人 |
|------|----------|--------|
| 方案评审 | arch-design 完成 + challenge 无阻塞项 | tech-lead |
| Phase 1 完成 | 3 节点注册/发现 E2E | qa-engineer |
| Phase 2 完成 | 联邦查询降级 E2E | qa-engineer |
| Phase 3 完成 | 订阅推送 webhook E2E | qa-engineer |
| Phase 4 完成 | Dashboard 5 页面可用 | frontend-engineer + qa-engineer |
| 发布准备 | 安全 baseline 通过 + 集成测试绿 | tech-lead |

---

## 技能装配清单

| 层级 | 技能 | 触发原因 | 主责角色 |
|------|------|----------|----------|
| shared | rust-patterns | Rust crate 开发 | backend-engineer |
| shared | rust-testing | 测试策略 | qa-engineer |
| shared | api-design | Hub REST API 设计 | architect |
| shared | frontend-engineering | Dashboard SPA | frontend-engineer |
| shared | frontend-ui-ux-system | 可视化设计 | frontend-engineer |
| shared | security-review | 安全 baseline | qa-engineer |
| ecc | rust-build | 构建验证 | backend-engineer |
| ecc | rust-review | 代码审查 | tech-lead |

---

## 前端交付物与检查点

| 交付物 | 阶段 | 状态 |
|--------|------|------|
| 产品类型 | intake | 确定：工具型 SPA Dashboard |
| 视觉方向 | plan | 极简深色/浅色主题，数据密度中等 |
| 设计 token | execute | 使用 shadcn/ui 默认 + 自定义 brand color |
| 响应式基线 | execute | 桌面优先（1280px 基准），平板降级（768px） |
| A11y 基线 | execute | 键盘导航、ARIA labels、对比度 4.5:1 |
| ui-review-checklist | review | Phase 4 完成后提交 |

---

## Implementation Readiness 结论

| 检查项 | 状态 |
|--------|------|
| Requirement Challenge 完成 | ✅ 6 项质疑已收敛 |
| Design Review | ✅ arch-design 产出 |
| 接口契约锁定 | ✅ Hub API 14 个端点已定义 |
| 前置依赖明确 | ✅ Phase 1-2 无阻塞，Phase 3 依赖 v0.4.0 |
| 安全基线定义 | ✅ Ed25519 + API Key + TLS + Rate Limit |
| Story slices 可执行 | ✅ 30 个 story，每个 2-5h |

**就绪状态**：`handoff-ready`
**下一跳**：backend-engineer 开始 Phase 1 执行

---

## 最后更新

2026-05-12 | tech-lead
