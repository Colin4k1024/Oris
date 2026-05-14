# Arch Design — Oris 多语言 SDK

**状态**: draft  
**日期**: 2026-05-14  
**主责**: architect  
**关联**: `delivery-plan.md`

---

## 系统边界

```
外部调用者（Go / Python / TypeScript 应用）
        │
        ▼
┌──────────────────────────────────────┐
│        Oris SDK（三语言）             │
│                                      │
│  ┌──────────┐  ┌───────────────┐    │
│  │HubClient │  │ExecutionClient│    │
│  └────┬─────┘  └──────┬────────┘    │
│       │               │             │
│  ┌────┴───────────────┴────────┐    │
│  │   ExperienceRepoClient      │    │
│  └─────────────────────────────┘    │
│                                     │
│  共享层：Auth / OEN / Errors / HTTP  │
└──────────────────────────────────────┘
        │
        ▼ HTTP REST
┌───────────────┐  ┌──────────────────────┐  ┌───────────────────────┐
│  Oris Hub     │  │ Oris Execution Server │  │ Oris Experience Repo  │
│  :8100        │  │ :8080                 │  │ :3000                 │
└───────────────┘  └──────────────────────┘  └───────────────────────┘
```

**边界内**: HTTP 请求构造、认证签名、响应解析、错误分类  
**边界外**: 服务端逻辑、持久化、调度器、Worker 协议、gRPC/WebSocket 传输  
**集成点**: 三个 HTTP 服务（各自独立端口和认证协议）

---

## 组件拆分

### `spec/`（单一事实源，语言无关）

| 文件 | 内容 |
|------|------|
| `openapi.yaml` | Hub API（手工从 routes.rs 提取，8 个核心端点） |
| `execution-openapi.yaml` | Execution Runtime API（从 runtime-api-contract.json 转换） |
| `oen-envelope-spec.md` | OEN 信封字段定义、签名 payload 覆盖范围、header 格式 |
| `signing-spec.md` | Hub（Base64）vs ExperienceRepo（hex）编码；Ed25519 golden vectors |
| `golden/` | 每个端点的 request + response fixture JSON |

### SDK 每语言结构（以 Go 为范本）

```
sdks/go/oris/
├── hub/
│   ├── client.go        # HubClient：register, heartbeat, discover, search, subscriptions
│   ├── models.go        # HubClientConfig, NodeRecord, DiscoveryResult, ...
│   └── signing.go       # Ed25519 签名（Base64 编码）
├── execution/
│   ├── client.go        # ExecutionClient：run_job, get/list/cancel/resume/replay, history, timeline
│   └── models.go        # RunJobRequest, ApiEnvelope[T], JobStateResponse, ...
├── exprepo/
│   ├── client.go        # ExperienceRepoClient：share, fetch
│   ├── oen.go           # OEN 信封构造 + canonical JSON + hex Ed25519 签名
│   └── models.go        # ShareRequest, ShareResponse, FetchResponse, NetworkAsset
├── auth/
│   ├── bearer.go        # BearerAuth（API Key）
│   ├── ed25519.go       # Ed25519Auth（Base64/hex 两种模式）
│   └── composite.go     # CompositeAuth（Hub 写 = 签名，Hub 读 = Bearer）
└── errors.go            # OrisError 层次（见下方错误模型）
```

Python 和 TypeScript 保持同构结构（目录对应，命名遵循各语言惯例）。

---

## 关键数据流

### Hub 节点注册（写操作 = Ed25519 签名）

```
HubClient.register(config)
    │
    ├─ 构造 RegisterRequest { node_id, endpoint, public_key(Base64), capabilities, region, version }
    ├─ body = JSON serialize(RegisterRequest)
    ├─ signature = Ed25519.sign(seed, body) → Base64 encode
    ├─ HTTP POST /hub/nodes
    │   Headers: X-OEN-Signature: <Base64_sig>, Content-Type: application/json
    │   Body: <JSON>
    └─ 解析 RegisterResponse → success / HubError
```

### Hub 节点发现（读操作 = Bearer API Key）

```
HubClient.discover_nodes(query)
    │
    ├─ HTTP GET /hub/nodes
    │   Headers: Authorization: Bearer <api_key>
    │   Body: JSON(DiscoveryQuery)
    └─ 解析 DiscoveryResult
```

### Experience Repo share()（OEN 信封 + hex 签名）

```
ExperienceRepoClient.share(payload)
    │
    ├─ 构造 OEN 信封 envelope { version, timestamp, node_id, message_type, payload }
    ├─ canonical_bytes = JSON(sort_keys=True, no_whitespace)(envelope.payload)
    ├─ signature = Ed25519.sign(seed, canonical_bytes) → hex encode
    ├─ HTTP POST /experience/share
    │   Headers: X-Api-Key: <api_key>, X-OEN-Signature: <hex_sig>
    │   Body: JSON(envelope)
    └─ 解析 ShareResponse
```

### Execution run_job()（Bearer API Key 认证）

```
ExecutionClient.run_job(request)
    │
    ├─ HTTP POST /jobs
    │   Headers: Authorization: Bearer <api_key>
    │   Body: JSON(RunJobRequest { thread_id, input, idempotency_key, timeout_policy, priority })
    └─ 解析 ApiEnvelope<RunJobResponse> { meta, request_id, data }
```

---

## 接口约定

### 认证分层（三服务各自独立）

| 服务 | 写操作认证 | 读操作认证 | 注意 |
|------|-----------|-----------|------|
| Hub | `X-OEN-Signature: <base64_sig>` | `Authorization: Bearer <api_key>` | 同一客户端需同时持有 seed 和 api_key |
| Experience Repo | `X-Api-Key: <api_key>` + `X-OEN-Signature: <hex_sig>` | `X-Api-Key: <api_key>` | 注意：非 Bearer，是 X-Api-Key |
| Execution Server | `Authorization: Bearer <api_key>` | `Authorization: Bearer <api_key>` | 无签名要求 |

### 错误模型（三语言统一分类）

```
OrisError
├── AuthError
│   ├── InvalidKey       # 密钥格式错误
│   ├── SignatureRejected # 服务端拒绝签名（403）
│   └── TokenExpired     # Bearer token 过期（401）
├── NetworkError
│   ├── Timeout
│   ├── ConnectionRefused
│   └── TlsError
├── ApiError             # 服务端 4xx/5xx
│   ├── NotFound         # 404
│   ├── Conflict         # 409（幂等键冲突）
│   ├── RateLimited      # 429（含 retry_after）
│   └── ServerError      # 5xx
├── SerializationError   # JSON 序列化/反序列化失败
└── ValidationError      # 客户端参数校验失败
```

`ApiError` 必须携带 `request_id`（来自 `ApiEnvelope.meta.request_id`），方便关联服务端日志。

### 响应包装解包

Execution Server 所有响应使用 `ApiEnvelope<T>`:

```json
{
  "meta": { "status": "ok", "api_version": "v1" },
  "request_id": "req-123",
  "data": { ... }
}
```

SDK 在解析后自动解包，调用方直接获取 `T`（`RunJobResponse` 等），不暴露 envelope 结构。`request_id` 附加到成功响应结果上，方便日志关联。

---

## 技术选型

| 维度 | Go | Python | TypeScript |
|------|----|--------|------------|
| HTTP Client | `net/http` 标准库 | `httpx`（async + sync 均支持） | `fetch` (Node 18+ built-in) |
| Ed25519 | `crypto/ed25519` 标准库 | `cryptography` (hazmat.primitives) | `node:crypto` built-in |
| JSON | `encoding/json` 标准库 | `json` 标准库 + `json.dumps(sort_keys=True)` | `JSON.stringify` + 自定义 `canonicalize()` |
| 数据模型 | struct + json tag | `pydantic v2` dataclass | TypeScript interfaces + discriminated union |
| 测试 | `testing` 标准库 | `pytest` | `vitest` |
| 最低版本 | Go 1.21（泛型支持） | Python 3.10+ | Node.js 18+ |

**选型原则**: 优先使用语言标准库（特别是 Ed25519 和 HTTP），避免引入不必要的第三方依赖，降低供应链风险。

---

## Canonical JSON 规范

OEN 信封签名 payload 使用统一 canonical JSON 序列化（三语言一致）：

- key 按 lexicographic 升序排列
- 无多余 whitespace（无空格、换行）
- 字符串 UTF-8 编码
- 浮点数不做特殊处理（当前 payload 中无浮点字段）

**验证方式**: golden-file 测试中包含已知 seed、payload → 期望 canonical_bytes → 期望 signature（hex），三语言各自运行并比对。

---

## monorepo 目录结构

```
sdks/
├── README.md                        # 总入口：安装 + quickstart 三语言
├── spec/
│   ├── openapi.yaml                 # Hub API spec
│   ├── execution-openapi.yaml       # Execution Runtime API spec
│   ├── oen-envelope-spec.md         # OEN 信封规范（硬依赖）
│   ├── signing-spec.md              # 签名编码规范 + golden vectors
│   └── golden/                      # 每个端点的 request/response fixture
│       ├── hub_register_request.json
│       ├── hub_register_response.json
│       ├── execution_run_job_request.json
│       ├── execution_run_job_response.json
│       ├── exprepo_share_request.json
│       └── exprepo_oen_signing_vector.json
├── go/
│   ├── go.mod                       # module oris.io/sdk, go 1.21
│   ├── go.sum
│   ├── oris/                        # 包结构见上方
│   ├── tests/
│   └── examples/
├── python/
│   ├── pyproject.toml               # package: oris-sdk, version: 0.1.0
│   ├── oris/
│   ├── tests/
│   └── examples/
└── typescript/
    ├── package.json                 # name: @oris/sdk, version: 0.1.0
    ├── tsconfig.json                # strict: true
    ├── src/
    ├── tests/
    └── examples/
```

---

## 风险与约束

| 风险 | 类型 | 缓解 |
|------|------|------|
| OEN 信封 schema 变更（服务端）导致 SDK 破坏性变更 | 高影响 | `oen-envelope-spec.md` 版本化，变更触发 SDK major/minor bump |
| Go cgo 会破坏纯静态链接（若未来引入 Rust core FFI） | 架构约束 | v0.1.0 严格使用纯 Go，不引入 cgo；FFI 路径标记为 v1+ 可选 |
| TypeScript `node:crypto` Ed25519 在旧版 Node 中不可用 | 兼容约束 | 最低版本声明 Node 18+，文档显式标注 |
| Hub API 认证头名称变更（X-OEN-Signature） | 协议约束 | Hub openapi.yaml 中标注为 stable contract，变更需服务端同步 |

---

## ADR 输入

| 决策点 | 结论 | 记录位置 |
|--------|------|---------|
| SDK 版本独立于 oris-runtime（v0.1.0 起步）| 已决策 | delivery-plan.md |
| 三语言纯 native 实现（方案 A），不用 Rust FFI | 已决策 | 本文档 |
| v1 订阅仅 HTTP CRUD，不做长连接 | 已决策 | delivery-plan.md |
| Ed25519 安全降级可接受，文档化而非阻断 | 已决策 | 本文档 + README security 说明 |

> 以上决策影响面较小，均在 SDK 层内部，不触及 Oris 服务端架构，无需单独 ADR；若未来引入 Rust FFI 绑定，需补 ADR。
