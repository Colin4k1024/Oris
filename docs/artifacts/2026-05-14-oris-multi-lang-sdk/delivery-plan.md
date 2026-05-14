# Delivery Plan — Oris 多语言 SDK

**状态**: draft → handoff-ready  
**日期**: 2026-05-14  
**阶段**: plan  
**主责角色**: tech-lead  
**slug**: oris-multi-lang-sdk  
**关联 PRD**: `prd.md`

---

## 需求挑战会收口（Requirement Challenge Session）

### 挑战分组与关键发现

**分组 A — API 契约与认证机制（architect + product-manager）**

| # | 质疑 | 结论 | 阻断？ |
|---|------|------|--------|
| A-1 | Execution Client 是否需要 Ed25519 签名？PRD 描述混淆了三个服务的认证机制 | Execution Server 仅需 Bearer API Key；Hub 写操作需 Ed25519 + X-OEN-Signature；Hub 读操作需 Bearer；Experience Repo 用 X-Api-Key（非 Bearer）。三套机制独立，SDK 不应共享统一入口 | Pre-flight |
| A-2 | OEN 信封字段与签名 payload 定义缺失，三语言无法独立实现 `share()` | **硬阻断**：必须先从 Rust 源码逆向提取 OEN 信封 schema，输出 `spec/oen-envelope-spec.md`，才允许任何语言开始 `share()` 实现 | **硬阻断** |
| A-3 | Rust 代码中 Hub 用 Base64 签名，Experience Repo 用 hex 签名，SDK 须准确复制而非统一 | 接受现状，SDK 按服务各自约定实现；`signing-spec.md` 中显式标注每个服务的编码格式差异 | Pre-flight |
| A-4 | `serde_json::to_vec(&payload)` 是签名覆盖范围，各语言 JSON 序列化默认行为不同（key 排序、whitespace） | 三语言使用 canonical JSON 序列化（key lexicographic 排序、无 whitespace），golden-file 测试验证跨语言一致性 | Pre-flight |
| A-5 | 订阅 API 传输语义不明（pull / SSE / WebSocket？） | v1 仅支持 HTTP CRUD（注册 filter + pull 模式），不做长连接；接口签名标注 `v1: pull only` | 已决策 |

**分组 B — 并行开发可行性（project-manager）**

| # | 质疑 | 结论 | 阻断？ |
|---|------|------|--------|
| B-1 | 三个 spec 工作量不对称（Experience Repo 有 openapi.yaml 基线，Execution Runtime 有 JSON Schema，Hub 从零开始），步骤 0 不应串行化 | 步骤 0 拆为三条独立 spec 任务，各自完成即解锁对应 SDK 实现；Hub spec 工作量最大，优先启动 | 已决策 |
| B-2 | "并行"假设为单人串行执行，三语言同时踩 Ed25519 签名规范问题会产生重复返工 | 签名规范文档（`signing-spec.md`）在任何语言开始 Ed25519 实现前完成，共享 golden test vectors | Pre-flight |
| B-3 | 成功指标"集成测试"与"无 docker-compose CI"矛盾 | 降低成功标准：MVP 阶段以 mock server + golden-file 测试为验证基础；docker-compose 集成测试为第二阶段可选项 | 已决策 |

**分组 C — 安全与技术架构（architect）**

| # | 质疑 | 结论 | 阻断？ |
|---|------|------|--------|
| C-1 | GC 运行时（Go / Python / TS）无法等价保证 raw seed 安全清零 | 接受安全降级，文档中标注"GC 运行时无法保证 seed 内存清零；生产环境建议使用 HSM 或密钥管理服务"；SecureSeed 包装类作为可选增强 | 已决策 |
| C-2 | 无 OpenAPI spec 时三语言字段语义漂移无法自动检测 | 以 `spec/` 目录下的 golden JSON fixtures 作为字段定义事实源，所有语言的 model 定义必须与 golden 保持一致 | Pre-flight |

### challenge 收口结论

> 所有 Pre-flight 阻塞项在 Story 0 阶段解决。Story 0 完成后，`implementation-readiness` 状态提升为 `handoff-ready`，Story 1-4 可并行执行。

---

## Brownfield 上下文快照

| 维度 | 现状 |
|------|------|
| 现有 Rust 客户端 | `oris-hub-client` v0.1.0（完整参考实现）；`oris-experience-repo/src/client/` v0.3.0；无独立 Execution HTTP client crate |
| 现有 spec | Experience Repo: `docs/openapi.yaml` v0.1.0（不完整）；Execution Runtime: `docs/runtime-api-contract.json`（自定义 JSON Schema，非 OpenAPI 3.x）；Hub: 无 spec |
| 认证头名称差异 | Hub 写: `X-OEN-Signature`；Hub 读: `Authorization: Bearer`；Experience Repo: `X-Api-Key`；Execution: 待确认 |
| Ed25519 编码差异 | Hub: Base64（签名 + 公钥）；Experience Repo: hex（签名）/ `public_key_hex`（公钥注册字段名） |
| `sdks/` 目录 | 不存在，需新建 |
| 版本策略 | SDK 初始版本：Go `v0.1.0`，Python `0.1.0`，npm `0.1.0`；changelog 中标注"对应 oris-runtime vX.Y.Z API surface" |

---

## 版本目标

**v0.1.0 (MVP)**：Go + Python + TypeScript SDK 均覆盖 Hub Client + Execution Client + Experience Repo Client 的核心接口，通过 golden-file 测试验证跨语言一致性。

**放行标准**：
1. 三语言 SDK 各自通过 golden-file 测试（request 构造 + response 解析）
2. Ed25519 signing golden vectors 在三语言中验证通过
3. 每个 SDK 有可运行的 quickstart 示例（README + examples/ 目录）
4. CI 在 push 时自动运行三语言测试

---

## Story Slices

### Story 0：规范先行（Pre-flight，阻断后续所有 Story）

**目标**: 生成三语言实现的单一事实源，解除所有 Pre-flight 阻断条件

**验收标准**:
- [ ] `sdks/spec/oen-envelope-spec.md` — 从 `oris-experience-repo` Rust 实现逆向，含：信封完整字段列表、签名 payload 覆盖范围（仅 `payload` 字段 JSON bytes）、`X-OEN-Signature` header 格式
- [ ] `sdks/spec/signing-spec.md` — Hub（Base64）vs Experience Repo（hex）编码差异；Ed25519 golden test vectors（seed → public_key → signature），三语言各提供一份验证用例
- [ ] `sdks/spec/openapi.yaml` (Hub) — 手工从 `oris-hub/src/api/routes.rs` + `handlers.rs` + 数据类型提取，至少覆盖 8 个核心端点
- [ ] `sdks/spec/execution-openapi.yaml` — 从 `docs/runtime-api-contract.json` 转换为 OpenAPI 3.x，覆盖 Job CRUD 9 个端点
- [ ] `sdks/spec/golden/` — 每个端点至少一个 request + response golden JSON fixture

**主责**: backend-engineer（OEN 逆向 + Hub spec）/ architect（Execution spec 转换）  
**依赖**: 无  
**估时**: 3-4 天

---

### Story 1：Go SDK 骨架 + Hub Client

**目标**: Go SDK 完整实现 Hub Client，通过 golden-file 测试

**验收标准**:
- [ ] `sdks/go/oris/hub/` — `HubClient`、`HubClientConfig`、`register()`、`heartbeat()`、`discover_nodes()`、`get_node()`、`federated_search()`
- [ ] `sdks/go/oris/hub/` — subscription CRUD: `create_subscription()`、`list_subscriptions()`、`delete_subscription()`
- [ ] `sdks/go/oris/auth/` — `Ed25519Auth`（Base64 签名）、`CompositeAuth`（写操作用签名，读操作用 Bearer）
- [ ] `sdks/go/oris/errors.go` — `OrisError`、`AuthError`、`ApiError`（含 `request_id`）、`NetworkError`
- [ ] `sdks/go/tests/golden_hub_test.go` — 覆盖 register/heartbeat/discover/search
- [ ] `sdks/go/examples/hub_quickstart/main.go`

**主责**: backend-engineer  
**依赖**: Story 0（oen-envelope-spec + signing-spec + Hub openapi.yaml + golden fixtures）  
**估时**: 3 天

---

### Story 2：Go SDK — Execution Client

**目标**: Go SDK 实现 Execution Client

**验收标准**:
- [ ] `sdks/go/oris/execution/` — `ExecutionClient`、`run_job()`、`get_job_state()`、`get_job_detail()`、`list_jobs()`、`cancel_job()`、`resume_job()`、`replay_job()`、`get_job_history()`、`get_job_timeline()`
- [ ] `ApiEnvelope[T]` 泛型响应包装（Go generics 1.18+）
- [ ] `sdks/go/tests/golden_execution_test.go`
- [ ] `sdks/go/examples/execution_quickstart/main.go`

**主责**: backend-engineer  
**依赖**: Story 0（execution-openapi.yaml + golden fixtures）  
**估时**: 2 天

---

### Story 3：Go SDK — Experience Repo Client

**目标**: Go SDK 实现 Experience Repo Client（含 OEN 签名）

**验收标准**:
- [ ] `sdks/go/oris/exprepo/` — `ExperienceRepoClient`、`share()`（含 OEN 信封构造 + hex Ed25519 签名）、`fetch()`
- [ ] `sdks/go/oris/exprepo/oen.go` — OEN 信封构造，canonical JSON 序列化
- [ ] golden-file 验证 OEN 签名与 Rust 实现互通
- [ ] `sdks/go/examples/exprepo_quickstart/main.go`

**主责**: backend-engineer  
**依赖**: Story 0（oen-envelope-spec.md + signing golden vectors）  
**估时**: 2 天

---

### Story 4：Python SDK（并行，参照 Go 参考实现）

**目标**: Python SDK 实现三个 Client，复用 Go SDK 的 golden fixtures

**验收标准**:
- [ ] `sdks/python/oris/hub/` — Hub Client（同 Story 1 接口覆盖）
- [ ] `sdks/python/oris/execution/` — Execution Client（同 Story 2 接口覆盖）
- [ ] `sdks/python/oris/exprepo/` — Experience Repo Client + OEN 签名
- [ ] 使用 `pydantic v2` 做模型定义，`httpx` 做 HTTP client
- [ ] Ed25519 使用 `cryptography` 库，canonical JSON 用 `json.dumps(sort_keys=True, separators=(',',':'))`
- [ ] `pyproject.toml`（package: `oris-sdk`）
- [ ] `sdks/python/tests/` — golden-file 测试覆盖三个 Client
- [ ] `sdks/python/examples/`

**主责**: backend-engineer  
**依赖**: Story 0 + Story 1（Go 参考实现作为行为参照）  
**估时**: 4 天

---

### Story 5：TypeScript SDK（并行，参照 Go 参考实现）

**目标**: TypeScript SDK 实现三个 Client

**验收标准**:
- [ ] `sdks/typescript/src/hub/`、`src/execution/`、`src/exprepo/` — 三个 Client（接口覆盖同 Go SDK）
- [ ] `ApiEnvelope<T>` 泛型响应包装
- [ ] Ed25519 使用 `node:crypto` built-in，不引入第三方签名库
- [ ] canonical JSON 使用 `JSON.stringify(obj, Object.keys(obj).sort())` 或独立 `canonicalize()` 函数
- [ ] `package.json`（name: `@oris/sdk`）+ `tsconfig.json`（strict mode）
- [ ] `sdks/typescript/tests/` — golden-file 测试（Vitest 或 Jest）
- [ ] `sdks/typescript/examples/`

**主责**: backend-engineer  
**依赖**: Story 0 + Story 1（Go 参考实现）  
**估时**: 4 天

---

### Story 6：CI 集成 + 包发布配置

**目标**: 三语言测试在 GitHub Actions CI 中自动运行；发布配置就绪

**验收标准**:
- [ ] `.github/workflows/sdk-test.yml` — Push 时运行 Go test / Python pytest / TS vitest
- [ ] Go module path：`oris.io/sdk`（在 `go.mod` 中声明）
- [ ] Python：`pyproject.toml` 中 `version = "0.1.0"`，标注 `# Corresponds to oris-runtime vX.Y.Z`
- [ ] npm：`package.json` 中 `"version": "0.1.0"`，标注对应 oris-runtime 版本
- [ ] `sdks/README.md` — 三语言安装命令 + quickstart 链接

**主责**: devops-engineer  
**依赖**: Story 1-5 通过  
**估时**: 1 天

---

## 角色分工

| 角色 | Story | 职责摘要 |
|------|-------|---------|
| tech-lead | 全程 | PRE-FLIGHT 验证、方案仲裁、版本策略收口 |
| architect | Story 0 | Execution Runtime JSON Schema → OpenAPI 3.x 转换；整体架构约束 |
| backend-engineer | Story 0-5 | OEN 逆向、Hub spec 手写、三语言 SDK 实现 |
| qa-engineer | Story 0 | golden fixtures 设计；Ed25519 互操作测试向量设计 |
| devops-engineer | Story 6 | CI 配置、包发布管道 |

---

## 风险与缓解

| 风险 | 概率 | 影响 | 缓解措施 | Owner |
|------|------|------|----------|-------|
| OEN 信封 canonical JSON 序列化跨语言不一致 | 高 | 高 | Story 0 产出 golden vectors + 三语言互操作测试；签名失败即 blocker | backend-engineer |
| Hub spec 手写遗漏隐式认证规则 | 中 | 高 | spec 完成后 code review 比对 `routes.rs` 逐行核对 | architect |
| Python / TS Ed25519 库选型变更 | 低 | 中 | 预先锁定：Python=`cryptography`，TS=`node:crypto` | backend-engineer |
| Go 泛型版本要求（1.18+）与下游兼容 | 低 | 中 | `go.mod` 中声明 `go 1.21`，文档说明最低版本要求 | backend-engineer |
| Experience Repo OEN 签名规范变更 | 低 | 高 | `oen-envelope-spec.md` 与 Rust 代码版本绑定；变更时触发三语言 SDK breaking change | tech-lead |

---

## 检查节点

| 节点 | 条件 | 负责人 |
|------|------|--------|
| Story 0 Gate | oen-envelope-spec、signing-spec、Hub openapi.yaml、golden fixtures 全部完成并通过 code review | tech-lead |
| Story 1-3 Gate（Go SDK Complete） | Go SDK 三个 Client 通过 golden-file 测试；quickstart 可运行 | qa-engineer |
| Story 4-5 Gate（Multi-lang Complete） | Python + TS SDK 通过 golden-file 测试；三语言 Ed25519 互操作向量一致 | qa-engineer |
| Release Gate | CI 绿；三语言包发布配置就绪；`sdks/README.md` 完整 | tech-lead + devops-engineer |

---

## Implementation Readiness

**当前状态**: `not-ready` → 完成 Story 0 后提升为 `handoff-ready`

**执行前提证据**:
- challenge session 已完成（三角色，见上方表格）
- 硬阻断条件（OEN 信封 schema）已识别，恢复路径明确（从 Rust 源码逆向）
- 认证机制差异已明确（三套，各自独立）
- 版本策略已收口（SDK 独立 v0.1.0，changelog 标注对应 oris-runtime 版本）
- 测试策略已降级对齐（mock server + golden-file，无 docker-compose）

**进入 Story 1 的 handoff 前提**:
1. `sdks/spec/oen-envelope-spec.md` 存在且通过 review
2. `sdks/spec/signing-spec.md` 存在，golden vectors 包含 Hub（Base64）和 ExperienceRepo（hex）两套
3. `sdks/spec/openapi.yaml`（Hub）至少覆盖 8 个端点
4. `sdks/spec/golden/` 包含所有端点的 request + response fixture

---

## 就绪状态

**当前阶段**: plan  
**就绪状态**: `ready-for-review`（plan 收口后提交 tech-lead review）  
**下一跳**: `/team-execute` Story 0 → Story 1~5 并行  
**阻塞项**: 无（Story 0 本身即解锁条件，无外部阻塞）
