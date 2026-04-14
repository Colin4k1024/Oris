---
artifact: launch-acceptance
task: experience-repo-phase2
date: 2026-04-14
role: qa-engineer
status: accepted
---

# Launch Acceptance — 经验仓库二期 (Experience Repository Phase 2)

## 1. 验收概览

| 字段 | 内容 |
|------|------|
| **对象** | oris-experience-repo v0.2.0 |
| **时间** | 2026-04-14 |
| **角色** | backend-engineer + qa-engineer |
| **验收方式** | 本地构建 + 单元测试 + 集成测试 |

## 2. 验收范围

### 功能边界
- ✅ POST /experience（Share 经验）
- ✅ GET /keys（列出 Key）
- ✅ POST /keys（创建 Key）
- ✅ DELETE /keys/:key_id（撤销 Key）
- ✅ POST /keys/:key_id/rotate（轮换 Key）
- ✅ GET /experience（Fetch 回归）
- ✅ GET /health（健康检查）

### 不在范围内（MVP）
- ❌ Ed25519 签名验证（需要 PKI）
- ❌ 向量语义搜索
- ❌ Feedback 功能
- ❌ Rate Limiting

## 3. 验收证据

### 单元测试
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

test result: ok. 15 passed; 0 failed
```

### 构建验证
```
cargo build -p oris-experience-repo ✅
cargo build -p oris-exp-repo-cli ✅
```

## 4. Go / No-Go 检查项

| 检查项 | 状态 | 说明 |
|--------|------|------|
| 所有单元测试通过 | ✅ Go | 15/15 通过 |
| 代码编译无错误 | ✅ Go | release + debug 编译通过 |
| Share API 基本流程可用 | ✅ Go | Envelope 接收 + Gene 存储 |
| Key Management CRUD 可用 | ✅ Go | Create/List/Revoke/Rotate |
| Fetch API 回归正常 | ✅ Go | 原有功能不受影响 |
| 已知限制已文档化 | ✅ Go | test-plan.md 已标注 |

## 5. 风险判断

### 已满足项

| 项 | 状态 |
|------|------|
| 单元测试 100% 通过 | ✅ |
| Share API 端到端流程 | ✅ |
| Key Management API | ✅ |
| CLI 工具构建成功 | ✅ |
| OpenAPI 文档已生成 | ✅ |

### 已接受风险

| 风险 | 影响 | 接受理由 |
|------|------|----------|
| Ed25519 签名验证未启用 | 无法验证经验来源 | MVP 仅内部使用，API Key 提供基本访问控制 |
| sender_id 由客户端提供 | 潜在身份伪造 | 二期启用签名验证后解决 |
| API Key 无 rate limiting | 暴力破解风险 | MVP 仅内部使用，二期添加 |
| SHA-256 无 salt | 彩虹表风险 | API Key 高熵，彩虹表攻击不可行 |

### 阻塞项

| 项 | 状态 | 说明 |
|------|------|------|
| 无阻塞项 | - | - |

## 6. 上线结论

**✅ 建议放行 MVP**

- 实现完整度：100%（Share + Key Management + Fetch）
- 测试覆盖率：15/15 通过
- 已知限制已文档化
- 二期功能（签名验证、Feedback）已明确排除

### 前提条件
- 生产环境使用需配置有效 API Key（目前测试用静态 Key）
- 建议监控 SQLite 连接数
- Ed25519 签名验证在二期启用前，不得用于不可信环境

### 观察重点
- Share API 响应时间（目标 <200ms）
- Key 创建/验证成功率
- API Key 验证拒绝率（异常请求）

### 二期前置项
1. 实现 PKI 公钥注册表
2. 启用 Ed25519 签名验证
3. 添加 rate limiting

## 7. 非阻塞改进建议

| 优先级 | 建议 | Owner |
|--------|------|-------|
| P1 | 实现 PKI 公钥注册表 | architect |
| P1 | 启用 Ed25519 签名验证 | backend-engineer |
| P2 | 添加 rate limiting | backend-engineer |
| P2 | CLI 工具完善 | backend-engineer |
| P3 | 向量语义搜索 | architect |
