# PRD — Oris 多语言 SDK

**状态**: draft  
**日期**: 2026-05-14  
**阶段**: intake  
**主责角色**: tech-lead  
**slug**: oris-multi-lang-sdk

---

## 背景

Oris 是一个自演化执行运行时，当前已有完整的 Rust 核心实现与 HTTP API。目前的三大核心服务端客户端能力（Hub Client、Execution Runtime Client、Experience Repo Client）均仅以 Rust crate 形式提供，对 Go、Python、TypeScript 等主流生态的开发者不可直接使用。

随着 Oris 生态扩展，需要让使用其他语言的开发者、外部系统集成方和 AI Agent 平台能够方便地接入 Oris 的核心能力，而不必依赖 Rust 工具链。

**现有 HTTP API 基础（可作为 SDK 底层协议）：**
- `oris-hub` — 节点注册、心跳、节点发现、联邦搜索、订阅管理（REST + Ed25519 签名 + API Key）
- `oris-execution-runtime` — 作业生命周期管理（提交、查询、取消、恢复、回放）、Worker 协议、中断管理（REST）
- `oris-experience-repo` — Gene/Capsule 共享与获取（REST + OEN 信封）
- `oris-hub-client` — 现有 Rust 参考实现（Go/Python/TS SDK 的行为参照）

---

## 目标与成功标准

### 业务目标
- 打破语言壁垒，让 Go、Python、TypeScript 开发者可以直接接入 Oris 核心能力
- 降低外部集成成本，支持 AI 平台、DevOps 工具、研究团队快速采纳

### 用户价值
- 用户无需了解 Rust 或 Oris 内部结构，通过惯用 API 调用 Oris 服务
- SDK 提供与原生 Rust 客户端对等的功能覆盖

### 成功指标
- 三个语言 SDK 均可通过集成测试覆盖核心功能路径
- 每个 SDK 有可运行的 quickstart 示例
- 与 Hub / Execution Server / Experience Repo 的 happy path 通过端到端验证

---

## 用户故事

### US-01：注册节点到 Hub
**作为** Go/Python/TypeScript 应用开发者  
**我希望** 调用 SDK 的 `register()` 方法将我的节点注册到 Oris Hub  
**验收标准**: 提供 `node_id`、`endpoint`、`capabilities` 和密钥材料后，SDK 能完成 Ed25519 签名并成功调用 `POST /hub/nodes`，返回注册结果

### US-02：提交并跟踪执行作业
**作为** 平台集成开发者  
**我希望** 使用 SDK 提交一个作业到 Execution Runtime，并轮询其状态  
**验收标准**: 通过 `run_job()` 提交后，可用 `get_job()` / `list_jobs()` 获取作业状态；支持 `cancel_job()`、`resume_job()`

### US-03：发现与搜索网络节点
**作为** 运维或编排系统开发者  
**我希望** 通过 SDK 发现当前注册的 Oris 节点并执行联邦搜索  
**验收标准**: `discover_nodes()` 返回可用节点列表；`search()` 支持 FederatedQuery

### US-04：共享与拉取 Gene/Capsule
**作为** AI 研究者或 Oris 节点运营者  
**我希望** 通过 SDK 向 Experience Repo 发布或拉取 Gene/Capsule  
**验收标准**: `share()` 上传成功并返回 ShareResponse；`fetch()` 按条件返回 NetworkAsset 列表

### US-05：管理订阅
**作为** 需要事件通知的集成系统  
**我希望** 通过 SDK 创建、查询和取消对 Hub 事件的订阅  
**验收标准**: `create_subscription()`、`list_subscriptions()`、`delete_subscription()` 均通过验证

---

## 范围

### In Scope（第一阶段 MVP）
- **目标语言**: Go、Python、TypeScript（Node.js）
- **覆盖 API 组**:
  - **Hub Client**: `register`, `heartbeat`, `discover_nodes`, `get_node`, `federated_search`, `create_subscription`, `list_subscriptions`, `delete_subscription`
  - **Execution Client**: `run_job`, `get_job_state`, `get_job_detail`, `list_jobs`, `cancel_job`, `resume_job`, `replay_job`, `get_job_history`, `get_job_timeline`
  - **Experience Repo Client**: `share`, `fetch`
- **认证支持**: API Key（Bearer）+ Ed25519 签名（Hub 写操作）
- **错误处理**: 统一错误类型，包含 HTTP 状态码和错误信息
- **基础文档**: 每个 SDK 的 README + quickstart 示例
- **SDK 发布形式**: Go module / Python PyPI 包 / npm 包（各自独立仓库或 monorepo 子目录）

### Out of Scope（第一阶段不做）
- Worker 协议（`poll`, `heartbeat`, `extend_lease`, `report_step`, `ack`）— 复杂度高，第二阶段
- 中断管理 API（`list_interrupts`, `resume_interrupt`, `reject_interrupt`）— 第二阶段
- 自动心跳循环（background goroutine/thread/task）— 第二阶段
- gRPC / WebSocket 传输层
- SDK 自动代码生成（OpenAPI codegen）— 当前 API 尚无 OpenAPI spec，手写更可控
- 移动端（iOS/Android）SDK

---

## 关键假设

1. Oris Hub、Execution Server、Experience Repo 均已在某个可寻址地址运行
2. Ed25519 签名使用 32-byte seed（与现有 Rust 实现一致）
3. API 向后兼容，不会在 SDK 开发周期内发生破坏性变更
4. 三语言 SDK 共享相同的 HTTP API 协议，不需要独立的 transport layer

---

## 非目标

- 不修改现有 Rust 服务端代码
- 不改变 Oris 的部署或运维流程
- 不提供托管 SDK 文档站（docs.rs 等）

---

## 风险与依赖

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| Hub API 的 Ed25519 签名实现在各语言有差异 | 高 | 参照 Rust 实现编写兼容性测试 |
| API schema 文档缺失，需逆向读取 Rust 代码 | 中 | 从 `api_models.rs` 和 `api_contract.rs` 提取，优先生成内部 spec |
| 三个语言同时开发，资源协调复杂 | 中 | 分阶段：先完成一个语言验证流程，再并行 |
| 尚无 OpenAPI spec，SDK 字段可能遗漏 | 中 | 以 `generate_runtime_api_contract()` 输出的 JSON 为参照 |
| Worker 协议复杂（有状态 lease）导致 scope 膨胀 | 高 | 明确 Out of Scope，专注 Job CRUD 和 Hub 注册 |

---

## 待确认项

1. **优先语言顺序**: 是先完成 Go，再并行 Python + TypeScript，还是三者同步推进？
2. **SDK 发布位置**: 独立仓库（oris-sdk-go / oris-sdk-python / oris-sdk-ts）还是 Oris monorepo 的 `sdks/` 子目录？
3. **OpenAPI spec 生成**: 是否在 SDK 开发之前先补充 oris-hub 和 oris-execution-runtime 的 OpenAPI spec？
4. **Ed25519 密钥格式**: SDK 接受什么格式的密钥输入（PEM、raw bytes、hex、base64 seed）？需统一约定
5. **集成测试环境**: SDK 测试依赖真实服务，是否有 docker-compose 或 test fixtures 提供？
6. **版本策略**: SDK 版本是否与 oris-runtime 版本号对齐？
7. **Experience Repo OEN 信封**: `oris-experience-repo` 的 `share()` 需要 OEN 信封和 Ed25519 签名，是否纳入 MVP？

---

## 需求挑战会候选分组

### 分组 A：API 契约与 Ed25519 签名机制
- 参与角色: `architect`、`backend-engineer`
- 核心问题: Hub 签名协议的跨语言一致性如何验证？能否先输出语言无关的 API 规范？

### 分组 B：SDK 架构与包发布策略
- 参与角色: `architect`、`tech-lead`
- 核心问题: monorepo vs 独立仓库？包命名约定？向后兼容策略？

### 分组 C：测试基础设施
- 参与角色: `qa-engineer`、`devops-engineer`
- 核心问题: 集成测试如何对接真实服务？CI 中如何启动依赖服务？
