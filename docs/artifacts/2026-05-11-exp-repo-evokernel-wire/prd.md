---
artifact: prd
task: exp-repo-evokernel-wire
date: 2026-05-11
role: tech-lead
status: completed
state: closed
---

# 经验仓库 × EvoKernel 集成胶水修复 — PRD

## 1. 背景与问题

经验仓库三期主链路（HTTP 服务端、OEN 签名、PKI Key 管理、GeneStore）已于 2026-04-14 全部就绪并发布（v0.3.0）。EvoKernel 本地 publish/fetch 语义同样完整（178 个测试通过）。

**当前阻断**：两层能力之间缺少运行时连接胶水，导致：

1. `ExperienceRepoClient` 只有 `fetch_experiences()` + `health()` 两个方法，**缺少 `share_experience()` 写入方法**，客户端 SDK 读写不对称。
2. EvoKernel 晋升资产后生成的 `EvolutionEnvelope` **没有任何代码将其推送到 experience-repo HTTP 端点**，缺少 peer 地址配置注入点和后台推送循环。
3. `GET /experience` 响应中 `contributor_id` 字段固定返回 `None`（代码注释 `// TODO: enrich from metadata`），贡献者溯源信息缺失。

### 关键假设（Karpathy 收敛）

- HTTP 服务端所有端点行为正确，不需要修改 handler 逻辑。
- OEN 签名/验签协议完整，不在本次范围内调整。
- P2P gossip mesh 是独立能力，不属于本次范围。
- EvoKernel 推送采用可选配置 + 同步调用方式（非强制依赖），不改变 EvoKernel 的核心 trait 签名。

---

## 2. 目标与成功标准

### 业务目标

打通经验仓库到 EvoKernel 的全链路：资产晋升 → 自动推送经验 → 其他节点可查询 → 贡献者可溯源。

### 成功指标

| 指标 | 验收标准 |
|------|---------|
| 客户端写入 | `ExperienceRepoClient::share_experience()` 调用成功，服务端返回 `gene_id` |
| 自动推送 | EvoKernel 资产晋升后，经验仓库 `GET /experience` 可查询到该资产 |
| 贡献者溯源 | `GET /experience` 响应中 `contributor_id` 非空，可追溯到贡献 Agent |
| 测试覆盖 | 三项能力均有对应单元测试，oris-experience-repo 测试数量 ≥ 42 |

---

## 3. 用户故事

### 用户故事 1：客户端写入经验（Client SDK）
> 作为一个外部 Agent 开发者，我希望通过 `ExperienceRepoClient` 向经验仓库提交经验，无需手工构造 HTTP 请求。

**验收标准**：
- `ExperienceRepoClient::share_experience(envelope, api_key)` 存在且可调用
- 返回 `ShareResponse { gene_id, status, published_at }`
- 网络错误、HTTP 4xx/5xx 映射为 `ClientError`
- 单元测试覆盖成功路径和 HTTP 错误路径

### 用户故事 2：EvoKernel 资产晋升自动推送
> 作为一个运行 EvoKernel 的节点，我希望在资产晋升（Promoted）时，系统自动将其推送到配置好的经验仓库，无需手动触发。

**验收标准**：
- `EvoKernelRunner` 或等效推送点支持可选注入 `ExperienceRepoClient` 配置
- 当配置存在时，晋升事件触发 `share_experience()` 调用
- 推送失败不阻断晋升主路径（降级为日志 + 指标，不 panic）
- 集成测试可验证：晋升 → 推送 → 仓库可查到

### 用户故事 3：贡献者元数据溯源
> 作为一个查询经验仓库的 Agent，我希望在获取基因时能看到贡献者 ID，以便评估来源可信度。

**验收标准**：
- `GET /experience` 响应中 `contributor_id` 字段从 GeneStore 元数据或 KeyStore 中正确填充
- 贡献者 ID 与 `POST /experience` 时的 `sender_id` 一致
- 单元测试覆盖：有贡献者 → 返回 ID，无记录 → 返回 None（向后兼容）

---

## 4. 范围

### In Scope
- `oris-experience-repo`：`client/client.rs` 新增 `share_experience()` 方法
- `oris-experience-repo`：`server/handlers.rs` 中 `contributor_id` 元数据填充逻辑
- `oris-evokernel` 或 `oris-experience-repo` 中新增推送胶水模块（实现方案待 Plan 阶段确认）
- 各项能力对应的单元测试

### Out of Scope
- 修改 `POST /experience` 或 `GET /experience` HTTP 接口契约
- P2P gossip mesh 节点发现与同步
- 新增认证机制或 PKI 流程变更
- 性能优化或批量推送

---

## 5. 风险与依赖

| 风险 | 影响 | 缓解 |
|------|------|------|
| EvoKernel 推送注入点设计影响 API 稳定性 | 高 | Plan 阶段确认注入方式（构造函数注入 vs. 运行时注册），避免改变 trait 签名 |
| `contributor_id` 来源字段在 GeneStore schema 中可能缺失 | 中 | Plan 前确认 `SqliteGeneStore` 是否存储 `contributor_id`，必要时补 migrate |
| 推送失败静默化可能掩盖配置错误 | 低 | 推送失败记录结构化 tracing warn，暴露可观测指标 |

### 待确认项（已收口 2026-05-11）

- [x] **`SqliteGeneStore` schema**：`genes` 表和 `Gene` struct 均不含 `contributor_id`/`sender_id` 字段。T3 需要：① `Gene` struct 新增 `contributor_id: Option<String>`；② `genes` 表补 `ALTER TABLE … ADD COLUMN contributor_id TEXT`（幂等迁移）；③ `upsert_gene` / `get_gene` / `search_genes` 同步更新。
- [x] **T2 注入点**：独立 `NetworkPublisher` trait（DIP 解耦），`EvoKernelRunner` 持有 `Option<Box<dyn NetworkPublisher>>`，晋升路径调用 trait method。
- [x] **T2 推送策略**：晋升时同步推一次；失败 tracing warn，不阻断晋升主路径。

---

## 6. 参与角色

| 角色 | 职责 |
|------|------|
| `tech-lead` | 注入点设计方案确认、跨 crate 依赖审批 |
| `backend-engineer` | T1/T2/T3 全部实现（Rust） |
| `qa-engineer` | 测试覆盖验收、集成测试设计 |

---

## 7. 任务清单（Intake 分解）

| ID | 任务 | 主责 | 依赖 | 优先级 |
|----|------|------|------|--------|
| T1 | `ExperienceRepoClient::share_experience()` 实现 + 单元测试 | backend-engineer | 无 | P0 |
| T2a | `NetworkPublisher` trait 定义（`oris-experience-repo` crate 内） | backend-engineer | 无 | P0 |
| T2b | `ExperienceRepoClient` 实现 `NetworkPublisher` trait | backend-engineer | T1, T2a | P0 |
| T2c | `EvoKernelRunner` 注入 `Option<Box<dyn NetworkPublisher>>`，晋升路径同步调用，失败 warn 不 panic | backend-engineer | T2a | P0 |
| T3a | `oris-genestore`：`Gene` struct 新增 `contributor_id: Option<String>` + schema `ALTER TABLE` 幂等迁移 | backend-engineer | 无 | P1 |
| T3b | `upsert_gene` / `get_gene` / `search_genes` 同步更新持久化逻辑 + 现有测试回归 | backend-engineer | T3a | P1 |
| T3c | `fetch_experiences` handler 从 Gene 对象填充 `contributor_id` 响应字段 | backend-engineer | T3a | P1 |

> **并行路径**：T1 + T2a + T3a 可同时开始；T2b 依赖 T1+T2a；T2c 依赖 T2a；T3b/T3c 依赖 T3a。

---

## 8. 需求挑战会候选分组

**所有待确认项已于 2026-05-11 收口，无需挑战会。**

已确认事项汇总：
- T2 注入点：`NetworkPublisher` trait（DIP 解耦）
- T2 推送策略：晋升时同步推一次，失败降级
- T3 存储路径：GeneStore schema 新增 `contributor_id` 列（非 KeyStore 查询）

---

## 9. 企业治理

- 无涉及企业内控补充项（纯 OSS 项目内部 crate 集成）
- 无数据合规风险（基因/胶囊为技术资产，无 PII）
- 无前端变更
