# PRD — Oris 进化能力多框架适配 SDK

**状态**: draft  
**日期**: 2026-06-22  
**阶段**: intake  
**主责角色**: tech-lead  
**slug**: evolution-sdk-eino-langchain

---

## 背景

Oris 的核心价值在于**自进化闭环**——通过 Gene（策略 DNA）、Capsule（已验证经验快照）和 EvolutionEvent（事件日志）三层资产模型，实现 Detect→Select→Replay→Validate→Solidify→Reuse 的自动化经验复用与沉淀。

当前 Rust 核心能力已经通过仓库内多语言 SDK 暴露出一部分协议面：`sdks/go` 与 `sdks/python` 已包含 Experience Repo、Hub、Execution、Store、Sync 等基础模块。缺口不再是从零实现 HTTP SDK，而是把 Oris 的经验进化语义适配到 Go/Python 主流 Agent 框架的执行生命周期中。

随着 AI Agent 框架的快速发展，Go 语言的 **Eino**（字节跳动开源）和 Python 的 **LangChain** 已成为主流 Agent 开发框架。让这些框架的开发者能够直接接入 Oris 的进化能力，将显著降低"自进化 Agent"的构建门槛。

**现有 SDK/API 基础（作为适配层底座）：**
- `sdks/go` — 已有 `experience`、`hub`、`execution`、`store`、`sync` 模块
- `sdks/python` — 已有 `oris_sdk.experience`、`hub`、`execution`、`store`、`sync_manager` 模块
- `oris-experience-repo` — Gene/Capsule 共享与获取（当前 SDK 使用 `GET/POST /experience`，公钥注册使用 `POST /public-keys`）
- `sdks/spec/signing-spec.md` — 跨语言 Ed25519 签名规范；Experience Repo 写入使用 body 内 `envelope.signature`，签名为 base64，公钥注册为 hex

---

## 目标与成功标准

### 业务目标
- 让 Go/Eino 和 Python/LangChain 开发者能够直接使用 Oris 的进化能力
- 建立"自进化 Agent"的跨框架参考实现，降低采纳门槛
- 为 Oris 进化能力建立更广泛的生态基础

### 用户价值
- Eino/LangChain 开发者无需了解 Rust 或 Oris 内部结构，通过惯用 API 参与进化闭环
- Agent 遇到已知问题时，自动从进化存储中查找并复用已验证经验
- 成功解决问题后，自动提取 Gene/Capsule 供其他 Agent 复用

### 成功指标
- 两个框架适配层均可通过单元测试覆盖核心进化路径
- 每个适配层有可运行的 quickstart 示例，展示完整的 Detect→Select→Replay→Validate→Solidify 流程
- 示例默认 local-first，不依赖真实 LLM 或远端 Oris 服务即可在 5 分钟内跑通
- 配置 Experience Repo 后，fetch/share happy path 复用现有 SDK

---

## 用户故事

### US-01：Agent 遇到错误时自动查找已验证经验
**作为** Eino/LangChain Agent 开发者  
**我希望** Agent 遇到编译错误、测试失败等信号时，自动从 Oris 进化存储中查找匹配的 Gene  
**验收标准**: 调用 `evolution.detect_signal()` 后，`evolution.select_gene()` 返回匹配的 Gene 列表（按置信度排序）

### US-02：重放已验证的 Capsule
**作为** Eino/LangChain Agent 开发者  
**我希望** 找到匹配 Gene 后，自动重放其 Capsule 中的策略步骤  
**验收标准**: `evolution.replay(capsule_id)` 返回可执行的策略步骤，Agent 可按步骤执行

### US-03：成功解决问题后沉淀经验
**作为** Eino/LangChain Agent 开发者  
**我希望** Agent 成功解决问题后，自动提取 Gene 和 Capsule 并发布到 Experience Repo  
**验收标准**: `evolution.solidify(signal, solution)` 生成 Gene+Capsule 并调用 `POST /experience-repo/share` 成功

### US-04：从共享网络获取外部经验
**作为** Eino/LangChain Agent 开发者  
**我希望** 从 Oris 进化网络获取其他 Agent 分享的高置信度 Gene  
**验收标准**: `evolution.fetch_genes(query)` 调用 `GET /experience-repo/fetch` 返回匹配的 Gene 列表

### US-05：查看进化状态面板
**作为** Eino/LangChain Agent 运营者  
**我希望** 查看本地进化存储的状态（Gene 数量、置信度分布、复用成功率）  
**验收标准**: `evolution.status()` 返回包含 genes_count、avg_confidence、reuse_rate 的结构化数据

---

## 范围

### In Scope（第一阶段 MVP）

**目标框架**:
- Go 语言：Eino 框架（字节跳动开源，GitHub: cloudwego/eino）
- Python：LangChain 框架（GitHub: langchain-ai/langchain）

**覆盖能力组**:
- **信号检测**: `detect_signal(error_message, context)` — 从错误/异常中提取结构化信号
- **Gene 查询**: `select_gene(signal)` — 用信号查询匹配的 Gene 候选
- **Capsule 重放**: `replay(candidate)` — 返回可解释策略步骤或 prompt/tool hint，默认不直接执行危险动作
- **经验沉淀**: `solidify(signal, solution)` — 从成功经验中提取 Gene+Capsule
- **网络共享**: `share(gene/capsule)` — 发布到 Experience Repo
- **网络获取**: `fetch_genes(query)` — 从 Experience Repo 获取 Gene
- **状态查询**: `status()` — 查看本地进化存储状态

**认证支持**:
- API Key（Bearer）
- Ed25519 签名（发布操作）

**错误处理**:
- 统一错误类型，包含 HTTP 状态码和错误信息
- 网络超时、重试策略

**基础文档**:
- 每个 SDK 的 README + quickstart 示例
- 进化闭环流程图解

**SDK 发布形式**:
- Go module：沿用 `github.com/Colin4k1024/Oris/sdks/go`
- Python PyPI 包：沿用 `oris-rt-sdk`，LangChain 依赖作为 optional extra

**示例项目**:
- Go/Eino 示例：一个简单的 Agent 遇到编译错误→查找 Gene→重放→成功→沉淀的完整流程
- Python/LangChain 示例：同上

### Out of Scope（第一阶段不做）

- EvoKernel 完整移植（Rust 核心，不做跨语言移植）
- 新增独立本地 Evolution Store（复用现有 SDK store）
- Worker 协议集成
- 自动心跳循环
- gRPC / WebSocket 传输层
- 移动端 SDK
- TypeScript/JavaScript SDK（已有独立 PRD）
- Governor 治理层本地实现（第一阶段依赖服务端 Governor）

---

## 关键假设

1. Oris Experience Repo 可选；MVP 示例优先 local-first
2. Experience Repo API 向后兼容，不会在 SDK 开发周期内发生破坏性变更
3. Ed25519 签名使用 32-byte seed（与现有 Rust 实现一致）
4. Go 和 Python 生态有成熟的 Ed25519 实现（`crypto/ed25519`、`PyNaCl` 或 `cryptography`）
5. Eino 和 LangChain 的 Agent 执行模型允许在工具调用结果后插入自定义逻辑
6. 进化闭环的"重放"步骤可以映射为框架的工具调用或 Chain 执行

---

## 非目标

- 不修改现有 Rust 服务端代码
- 不改变 Oris 的部署或运维流程
- 不提供托管 SDK 文档站
- 不新增独立本地 Evolution Store（复用现有 SDK store）
- 不移植 EvoKernel 或 Governor 到 Go/Python

---

## 风险与依赖

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| Eino/LangChain Agent 执行模型与进化闭环不兼容 | 高 | 先做 PoC 验证信号检测和重放的集成点 |
| Ed25519 签名在 Go/Python 有实现差异 | 中 | 继续复用现有 SDK signing 测试与 `sdks/spec/signing-spec.md` |
| Experience Repo API 文档与实现漂移 | 中 | 以现有 SDK 和 `sdks/spec` 为事实源，adapter 不直接拼协议 |
| Eino 框架成熟度未知，API 可能不稳定 | 中 | 抽象框架适配层，隔离框架依赖 |
| 进化闭环的"Validate"步骤需要访问本地编译/测试环境 | 高 | 第一阶段仅提供回调接口，由用户实现验证逻辑 |

---

## 待确认项

1. **Eino 框架适配点**: Eino 的 Agent 执行模型中，哪个环节适合插入信号检测和重放逻辑？需要深入研究 Eino 的 Chain/Tool 调用机制
2. **LangChain 适配点**: LangChain 的 `Tool` 或 `Chain` 中，哪个环节适合插入进化逻辑？是作为 Tool 还是作为 Callback？
3. **本地 vs 远程**: 已决策 local-first；远端 Experience Repo 为可选 fetch/share
4. **Ed25519 密钥格式**: 复用现有 SDK 约定：32-byte raw seed；Experience Repo public key hex，signature base64
5. **集成测试环境**: SDK 测试依赖真实 Experience Repo，是否有 docker-compose 或 test fixtures 提供？
6. **版本策略**: SDK 版本是否与 oris-runtime 版本号对齐？
7. **Go module 命名**: 已决策沿用 monorepo `sdks/go`
8. **Python 包命名**: 已决策沿用 `oris-rt-sdk`

---

## 需求挑战会候选分组

### 分组 A：进化闭环与 Agent 框架集成点
- 参与角色: `architect`、`backend-engineer`
- 核心问题: Eino/LangChain 的 Agent 执行模型中，哪个环节适合插入 Detect→Select→Replay→Solidify 逻辑？

### 分组 B：SDK 架构与框架适配层设计
- 参与角色: `architect`、`tech-lead`
- 核心问题: 如何设计框架无关的核心层 + 框架特定的适配层？是否需要中间抽象？

### 分组 C：测试基础设施与示例设计
- 参与角色: `qa-engineer`、`devops-engineer`
- 核心问题: 集成测试如何对接真实 Experience Repo？示例项目如何设计才能清晰展示进化闭环？

---

## 参与角色清单

| 角色 | 职责 | 输入缺口 |
|------|------|----------|
| `tech-lead` | 主导 intake、任务拆解、技术决策 | 需确认 Eino/LangChain 的适配点 |
| `architect` | SDK 架构设计、框架适配层设计 | 需深入研究 Eino/LangChain 执行模型 |
| `backend-engineer` | Go/Python SDK 实现 | 需了解 Experience Repo API 细节 |
| `qa-engineer` | 测试策略、集成测试设计 | 需确认测试环境和 fixtures |
| `devops-engineer` | CI/CD、发布流程 | 需确认 Go module / PyPI 发布流程 |

---

## 领域技能包启用建议

| 技能 | 用途 |
|------|------|
| `evolution-core` | 理解 Gene/Capsule/EvolutionEvent 模型 |
| `golang-patterns` | Go SDK 实现规范 |
| `python-patterns` | Python SDK 实现规范 |
| `api-design` | REST API 客户端设计 |
| `search-first` | 研究 Eino/LangChain 的集成点 |

---

## 企业治理待确认项

无（非企业内部应用）

---

## UI 范围、终端假设与质量门禁

无 UI 变更。SDK 为纯后端库。

质量门禁：
- 单元测试覆盖率 ≥ 80%
- 集成测试覆盖核心进化路径
- 与 Experience Repo 的端到端验证通过
