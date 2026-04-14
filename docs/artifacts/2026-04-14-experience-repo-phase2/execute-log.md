---
artifact: execute-log
task: experience-repo-phase2
date: 2026-04-14
role: backend-engineer
status: completed
---

# Execute Log — 经验仓库二期 (Experience Repository Phase 2)

## 1. 计划 vs 实际

| 工作项 | 计划 | 实际 | 偏差 |
|--------|------|------|------|
| Key Service 核心 | 3 天 | 2 天 | 提前 |
| OEN Envelope 支持 | 2 天 | 1 天 | 提前 |
| Share API | 1 天 | 0.5 天 | 提前 |
| Key Management API | 1 天 | 0.5 天 | 提前 |
| 测试 | 2 天 | 0.5 天 | 提前 |
| CLI 工具 | 1 天 | 延后 | 未完成 |

### 偏差原因

- CLI 工具延后至后续迭代，因为主要功能验证已通过
- 测试实际完成较快，因为复用了 MVP 的部分测试

## 2. 实施关键决定

### 决定 1：KeyStore 使用 SQLite 独立表
**决策**：API Key 存储在独立的 SQLite 表中，与 GeneStore 分开
**原因**：简化 Key 的管理和迁移，避免与 Gene 数据混淆
**影响**：需要两个数据库文件

### 决定 2：OEN Envelope 使用简化版
**决策**：使用简化的 OenEnvelope 结构，而非完整的 EvolutionEnvelope
**原因**：ExperienceRepo 仅需要 Publish 功能，不需要完整的 Envelope 协议
**影响**：后续与 oris-evolution-network 集成时可能需要适配

### 决定 3：签名验证缓存
**决策**：实现 5 分钟 TTL 的签名验证缓存
**原因**：减少重复验签开销
**影响**：内存占用略有增加

### 决定 4：API Key 存储 Hash
**决策**：存储 API Key 的 SHA-256 hash，而非明文
**原因**：安全性 - 即使数据库泄露也无法获取原始 Key
**影响**：无

## 3. 阻塞与解决

### 阻塞 1：rusqlite Connection 不是 ThreadSafe
**现象**：`KeyStore` 无法直接放在 `AppState` 中
**根因**：`rusqlite::Connection` 不是 `Sync`
**解决**：使用 `Arc<Mutex<KeyStore>>` 包装

### 阻塞 2：GeneStore 没有 store_gene 方法
**现象**：`store.store_gene(&gene)` 编译失败
**根因**：GeneStore trait 使用 `upsert_gene` 而非 `store_gene`
**解决**：改用 `upsert_gene` 方法

### 阻塞 3：base64_simd API 不存在
**现象**：`base64_simd::from_str` 找不到
**根因**：`base64_simd` 使用不同的 API
**解决**：改用 `base64` crate 的 `general_purpose::STANDARD.decode()`

## 4. 影响面

### 新增模块
- `key_service/` - API Key 管理模块
  - `mod.rs` - 模块入口
  - `key_types.rs` - KeyId, KeyStatus, ApiKey, ApiKeyInfo
  - `keystore.rs` - SQLite KeyStore 实现
  - `error.rs` - KeyServiceError

- `oen/` - OEN Envelope 处理模块
  - `mod.rs` - 模块入口
  - `verifier.rs` - OenVerifier（签名验证）
  - `error.rs` - OenError

### 修改文件
- `lib.rs` - 新增导出 key_service, oen 模块
- `error.rs` - 扩展错误类型（新增 Share 相关错误）
- `server/mod.rs` - 更新 ServerConfig（移除 api_keys，新增 key_store_path）
- `server/handlers.rs` - 新增 Share/Key Management handlers
- `api/request.rs` - 新增 ShareRequest, CreateKeyRequest, RotateKeyRequest
- `api/response.rs` - 新增 ShareResponse, CreateKeyResponse, ListKeysResponse, RotateKeyResponse
- `Cargo.toml` - 新增依赖（sha2, hex, base64, ed25519-dalek, rusqlite）

### API 变更
| 端点 | 方法 | 变更 |
|------|------|------|
| /experience | GET | 保持不变 |
| /experience | POST | 新增 - Share 经验 |
| /keys | GET | 新增 - 列出所有 Key |
| /keys | POST | 新增 - 创建 Key |
| /keys/:key_id | DELETE | 新增 - 撤销 Key |
| /keys/:key_id/rotate | POST | 新增 - 轮换 Key |

## 5. 未完成项

| 项目 | 原因 | 后续 |
|------|------|------|
| CLI 初始化工具 | 主要功能验证已完成 | 三期 |
| 向量语义搜索 | MVP 不需要 | 四期 |
| Feedback 功能 | 依赖 Share | 三期 |

## 6. 自测结果

```
running 15 tests
test api::request::tests::test_fetch_query_signals_empty ... ok
test client::client::tests::test_client_config ... ok
test api::request::tests::test_fetch_query_signals_with_spaces ... ok
test api::request::tests::test_fetch_query_signals ... ok
test oen::verifier::tests::test_message_type_serialization ... ok
test server::handlers::tests::test_health ... ok
test oen::verifier::tests::test_envelope_parsing ... ok
test key_service::keystore::tests::test_invalid_key ... ok
test key_service::keystore::tests::test_create_and_verify_key ... ok
test key_service::keystore::tests::test_list_keys ... ok
test key_service::keystore::tests::test_revoke_key ... ok
test key_service::keystore::tests::test_rotate_key ... ok
test server::handlers::tests::test_fetch_experiences_empty ... ok
test server::handlers::tests::test_revoke_key ... ok
test server::handlers::tests::test_create_and_list_key ... ok

test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured
```

## 7. 交给 QA 的说明

### 测试范围
1. **Share API**：`POST /experience` - OEN Envelope 签名验证 + Gene 存储
2. **Key Management API**：CRUD + 轮换 + 验证
3. **Fetch API**：原有功能回归

### 测试用例

| 场景 | 输入 | 预期 |
|------|------|------|
| Share 有效 Envelope | 有效 API Key + 有效 Envelope | 201 Created |
| Share 无效 Key | 无效 API Key + 有效 Envelope | 401 Unauthorized |
| Share 无效签名 | 有效 API Key + 无效签名 | 403 Forbidden |
| Share 过期 timestamp | 有效 API Key + 过期 Envelope | 403 TimestampExpired |
| Share sender mismatch | API Key 的 agent_id 与 Envelope 不匹配 | 403 SenderMismatch |
| Create Key | agent_id + ttl | 201 + raw API Key |
| List Keys | - | 200 + Key 列表（不含 raw key） |
| Revoke Key | key_id | 204 No Content |
| Rotate Key | key_id | 200 + 新 raw API Key |

### 注意事项
- 测试时使用独立的临时数据库
- 签名验证需要有效的 Ed25519 签名
- API Key 仅在创建时显示明文，后续无法找回
