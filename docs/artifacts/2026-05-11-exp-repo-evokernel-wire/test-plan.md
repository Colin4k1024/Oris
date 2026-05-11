---
artifact: test-plan
task: exp-repo-evokernel-wire
date: 2026-05-11
role: qa-engineer
status: completed
state: review
---

# Test Plan — 经验仓库 × EvoKernel 集成胶水修复

## 1. 测试范围

### 功能范围（In Scope）

| 能力 | 覆盖方式 | 状态 |
|------|----------|------|
| `ExperienceRepoClient::share_experience()` 成功路径 | mockito 单测 | ✅ 已覆盖 |
| `share_experience()` HTTP 4xx 映射为 `ClientError::HttpError` | mockito 单测 | ✅ 已覆盖 |
| `NetworkPublisher` trait 定义（编译验证） | `cargo check` + 单测引用 | ✅ 已覆盖 |
| `EvoKernel::with_network_publisher()` 构造注入 | 编译验证 | ✅ 已覆盖 |
| GeneStore `contributor_id` 字段持久化（upsert/get/search/stale） | 现有 store 测试回归 | ✅ 已覆盖（`None` 值路径）|
| `contributor_id` serde 向后兼容（JSON 不含字段时反序列化） | `#[serde(default)]` 设计保证 | ✅ 隐式覆盖 |
| `handlers.rs` fetch_experiences contributor_id 填充 | handler 单测通过 | ✅ 已覆盖 |
| `ALTER TABLE` 幂等迁移（已存在列不报错） | 所有 `SqliteGeneStore::open()` 测试 | ✅ 已覆盖 |

### 非功能范围（In Scope）

- 三个受影响 crate 的全量回归测试（247 passed, 0 failed）
- 无新增 clippy 警告（cargo fmt 通过）

### 不覆盖项（Out of Scope）

- E2E 集成测试（EvoKernel 晋升 → HTTP push → `GET /experience` 可查）— 无可用 HTTP 测试环境，待 v0.4.0
- `NetworkPublisher::publish_envelope` 端到端 mock 测试（见风险 R1）
- `contributor_id = Some(...)` 的 GeneStore 持久化正向验证（见风险 R2）

---

## 2. 测试矩阵

### T1 — share_experience()

| 场景 | 前置条件 | 预期结果 | 测试位置 |
|------|----------|----------|----------|
| 成功写入 | 服务端返回 200 + JSON body | `ShareResponse { gene_id, status }` 正确解析 | `client.rs` `share_experience_returns_share_response_on_success` |
| HTTP 401 | 服务端返回 401 | `ClientError::HttpError` | `client.rs` `share_experience_maps_http_error_to_client_error` |
| URL 构造错误 | `base_url` 无效 | `ClientError::UrlError` | 未显式覆盖（`url::ParseError` 自动传播）|

### T2 — NetworkPublisher / EvoKernel

| 场景 | 前置条件 | 预期结果 | 测试位置 |
|------|----------|----------|----------|
| 未注入 publisher，晋升正常完成 | `network_publisher: None` | 晋升不受影响，无 panic | 所有现有 evokernel 晋升测试（92 通过）|
| 注入 publisher，晋升失败时 push 也静默 | publisher 返回 `NetworkPublishError::Http` | tracing warn，晋升主路径不受影响 | 暂未覆盖（见缺口 C1）|
| `publish_envelope` 映射正确性 | 传入 `EvolutionEnvelope{assets, ...}` | OenEnvelope.payload = serde_json(assets) | 暂未覆盖（见缺口 C2）|

### T3 — GeneStore contributor_id

| 场景 | 前置条件 | 预期结果 | 测试位置 |
|------|----------|----------|----------|
| 旧数据库无 contributor_id 列 | 旧格式数据库 | ALTER TABLE 自动补列，旧行读取为 None | 所有 `:memory:` open() 测试隐式覆盖 |
| contributor_id = None 存储与读取 | 正常 upsert | 读取结果 contributor_id = None | 现有 store 单测（全覆盖）|
| contributor_id = Some(...) 存储与读取 | upsert gene + contributor_id | 读取结果与写入值一致 | **缺失（见缺口 C3）** |

---

## 3. 测试缺口与风险

| ID | 风险 | 严重度 | 影响路径 | 处理建议 |
|----|------|--------|----------|----------|
| R1 | `publish_envelope` 中 `signature.unwrap_or_default()` 在 `EvolutionEnvelope::publish()` 始终产生 `None`，导致空签名被发送到 OEN 服务端，Ed25519 验证必然失败，push 路径在生产中功能性损坏 | **HIGH（新引入）** | T2 NetworkPublisher → HTTP push 全路径 | 应在签名完成后再调用 `publish_envelope`，或 `publish_envelope` 接收预签名 envelope |
| R2 | `sender_id` 使用 `gene.id`（基因 UUID）而非节点/Agent 身份 ID，与 OEN 协议语义不符，服务端身份绑定验证会拒绝 | **HIGH（新引入）** | `maybe_push_to_network` → `OenEnvelope.sender_id` | EvoKernel 应持有节点 ID，传入 `EvolutionEnvelope::publish()` |
| R3 | 三个额外 `GenePromoted` 事件发射路径（replay、built-in bootstrap、trusted local report）未调用 `maybe_push_to_network`，实际只覆盖热路径晋升 | **MEDIUM（合同与实现不符）** | doc comment "after every GenePromoted event" 失真 | 修正注释或补全三处调用 |
| R4 | `let _ = conn.execute(ALTER TABLE...)` 静默丢弃所有错误（含 SQLITE_READONLY, SQLITE_LOCKED, SQLITE_CORRUPT），真实失败变成隐蔽的"列不存在"错误 | **MEDIUM（新引入）** | GeneStore 迁移健壮性 | 仅忽略 duplicate column error code，其余 propagate |
| R5 | `contributor_id = Some(...)` 的 GeneStore 持久化正向验证缺失 | **MEDIUM（测试缺口）** | T3b 列值 round-trip | 补充单测 |
| R6 | 关键管理端点（`POST/DELETE /keys`, `GET /keys`, `POST /keys/:id/rotate`）缺少 `verify_key` 鉴权 | **CRITICAL（预存在）** | 非本次引入，但评审中发现，需独立 issue 跟踪 | 创建独立 issue，下一迭代修复 |
| R7 | `ClientConfig` `#[derive(Debug)]` 暴露 `api_key` 到日志 | **HIGH（预存在）** | 调试/追踪日志 | 自定义 Debug impl，隔离 api_key |
| R8 | `reqwest::Client::new()` 无超时 | **HIGH（预存在）** | 推送时服务端无响应会阻塞 | 设置 `timeout` 和 `connect_timeout` |

---

## 4. 本次变更引入的新增风险分类

| 类型 | 新引入 | 预存在 |
|------|--------|--------|
| CRITICAL | 0 | 1（R6，Key 管理无鉴权） |
| HIGH | 2（R1 空签名，R2 sender_id 语义错误） | 2（R7 api_key 日志，R8 reqwest 无超时）|
| MEDIUM | 3（R3 调用缺口，R4 ALTER TABLE，R5 测试缺口）| 已知其他预存在风险 |

---

## 5. 放行建议

**条件放行（Conditional Go）**

前提：
- 基因晋升主路径（本次核心目标）功能完整，247 个测试全绿。
- NetworkPublisher HTTP push 路径在生产中因签名问题功能性损坏，但该路径是 best-effort 非阻断 side-effect，不影响晋升主路径。
- R1/R2 高风险项必须在 v0.4.0 前修复，不得在修复前开放 PKI 注册节点的生产推送。

**阻塞项（须在下一迭代 P0 完成）**：R1（空签名）、R2（sender_id 语义）、R6（Key 管理无鉴权）

**本迭代内可接受风险**：R3 - R5、R7 - R8（已在 backlog 登记）
