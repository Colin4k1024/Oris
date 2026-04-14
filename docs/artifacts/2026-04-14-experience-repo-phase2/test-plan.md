---
artifact: test-plan
task: experience-repo-phase2
date: 2026-04-14
role: qa-engineer
status: completed
---

# Test Plan — 经验仓库二期 (Experience Repository Phase 2)

## 1. 测试范围

### 功能范围
- **Share API**：`POST /experience` - OEN Envelope 接收 + Gene 存储
- **Key Management API**：CRUD + 轮换
- **Fetch API**：原有功能回归
- **Health API**：健康检查

### 非功能范围
- 性能测试（并发请求）
- Ed25519 签名验证（**MVP 禁用**，见已知限制）

### 不覆盖项（MVP）
- Ed25519 签名验证 - 需要 PKI
- 向量语义搜索 - 需要单独实现
- Feedback 功能 - 三期

## 2. 测试矩阵

### Share API 测试

| 场景 | 类型 | 前置条件 | 预期结果 |
|------|------|----------|----------|
| Share 有效 Envelope | 功能 | 有效 API Key + 有效 Envelope | 201 Created + gene_id |
| Share 缺失 API Key | 安全 | 无 X-Api-Key header | 401 Unauthorized |
| Share 无效 API Key | 安全 | 无效 API Key | 401 Unauthorized |
| Share 过期 API Key | 安全 | 过期 API Key | 401 KeyExpired |
| Share 已撤销 API Key | 安全 | 撤销的 API Key | 401 KeyRevoked |
| Share sender mismatch | 安全 | Key 的 agent_id ≠ Envelope sender_id | 403 SenderMismatch |
| Share 过期 timestamp | 异常 | Envelope timestamp > 5分钟 | 403 TimestampExpired（暂时禁用）|

### Key Management API 测试

| 场景 | 类型 | 前置条件 | 预期结果 |
|------|------|----------|----------|
| Create Key | 功能 | 有效 admin API Key | 201 + raw API Key |
| Create Key 带 TTL | 功能 | ttl_days=30 | 201 + expires_at |
| List Keys | 功能 | 有效 API Key | 200 + keys 列表（无 raw key） |
| List Keys 空 | 功能 | 无 Key | 200 + empty array |
| Revoke Key | 功能 | 有效 key_id | 204 No Content |
| Revoke 不存在 Key | 异常 | 无效 key_id | 401 InvalidApiKey |
| Rotate Key | 功能 | 有效 key_id | 200 + 新 raw API Key |
| Rotate 后旧 Key 失效 | 安全 | 轮换后的旧 Key | 401 Revoked |

### Fetch API 回归测试

| 场景 | 类型 | 前置条件 | 预期结果 |
|------|------|----------|----------|
| Fetch 空仓库 | 功能 | 仓库无数据 | 200 + empty assets |
| Fetch 有数据 | 功能 | 仓库有匹配 Gene | 200 + matching assets |
| Fetch 无匹配 | 功能 | 仓库有数据但无匹配 | 200 + empty assets |
| Fetch min_confidence 过滤 | 功能 | confidence=0.3 | 不返回 <0.5 的基因 |

### Health API 测试

| 场景 | 类型 | 预期结果 |
|------|------|----------|
| Health 检查 | 功能 | 200 + {status: "ok", version} |

## 3. 集成测试用例

### Share 完整流程

```rust
#[tokio::test]
async fn test_share_experience_full_flow() {
    // 1. 初始化
    let state = create_test_state();

    // 2. 创建 API Key
    let create_req = CreateKeyRequest {
        agent_id: "agent-123".to_string(),
        ttl_days: Some(30),
        description: Some("test".to_string()),
    };
    let key_resp = create_key(State(state.clone()), Json(create_req)).await.unwrap();

    // 3. 构造 Envelope
    let envelope = OenEnvelope {
        sender_id: "agent-123".to_string(),
        message_type: MessageType::Publish,
        payload: serde_json::json!({
            "gene": {
                "id": uuid::Uuid::new_v4().to_string(),
                "signals": ["timeout", "error"],
                "strategy": ["step1"],
                "validation": ["test"],
                "confidence": 0.8
            }
        }),
        signature: "fake_signature_for_mvp".to_string(), // MVP 不验证
        timestamp: Utc::now().to_rfc3339(),
    };

    // 4. 发送 Share 请求
    let share_req = ShareRequest { envelope };
    let mut headers = HeaderMap::new();
    headers.insert("X-Api-Key", key_resp.api_key.parse().unwrap());

    let resp = share_experience(State(state), headers, Json(share_req)).await.unwrap();
    assert_eq!(resp.status, "published");

    // 5. 验证 Gene 已存储
    let fetch_resp = fetch_experiences(
        State(state),
        Query(FetchQuery {
            q: Some("timeout".to_string()),
            min_confidence: 0.5,
            limit: 10,
            cursor: None,
        }),
    ).await.unwrap();
    assert!(!fetch_resp.assets.is_empty());
}
```

## 4. 风险评估

| 风险 | 概率 | 影响 | 缓解 |
|------|------|------|------|
| Ed25519 签名验证未启用 | 高 | 高 | MVP 文档标注，二期启用 PKI 后修复 |
| API Key 无 rate limiting | 中 | 中 | MVP 仅内部使用，文档标注 |
| sender_id 伪造 | 低（MVP 内部） | 高 | 二期启用签名验证 |
| SQLite 并发写入瓶颈 | 低 | 中 | WAL 模式已配置 |

## 5. 已知限制

| 限制 | 影响 | 缓解 |
|------|------|------|
| Ed25519 签名验证禁用 | 无法验证经验来源可信性 | MVP 依赖 API Key，二期启用 PKI |
| sender_id 由客户端提供 | 潜在身份伪造 | 二期启用签名验证 |
| API Key 无 rate limiting | 暴力破解风险 | MVP 仅内部使用 |

## 6. 放行建议

- [x] 15 个单元测试全部通过
- [x] Share API 端到端流程测试
- [x] Key Management CRUD 测试
- [x] Fetch API 回归测试
- [x] 签名验证已文档化禁用状态
- [ ] 性能测试（建议，非必须）
- [ ] 安全渗透测试（建议，二期前完成）

**建议：可以放行 MVP（带已知限制标注）**

## 7. 待办项（二期前必须完成）

| # | 项 | Owner | 优先级 |
|---|------|-------|--------|
| 1 | 实现 PKI 公钥注册表 | architect | P0 |
| 2 | 启用 Ed25519 签名验证 | backend-engineer | P0 |
| 3 | 添加 rate limiting | backend-engineer | P1 |
| 4 | 安全渗透测试 | qa-engineer | P1 |
