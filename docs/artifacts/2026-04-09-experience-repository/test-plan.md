---
artifact: test-plan
task: experience-repository
date: 2026-04-09
role: qa-engineer
status: draft
---

# Test Plan — 经验仓库 (Experience Repository)

## 1. 测试范围

### 功能范围
- GET /experience — 按 signals 查询基因
- GET /health — 健康检查
- API Key 验证逻辑

### 非功能范围
- 性能测试（SQLite 查询延迟）
- 并发连接数（10 并发请求）

### 不覆盖项
- POST /experience（Share）- MVP 排除
- POST /experience/{id}/feedback - MVP 排除
- 向量语义搜索 - MVP 排除

## 2. 测试矩阵

| 场景 | 类型 | 前置条件 | 预期结果 |
|------|------|----------|----------|
| 查询空仓库 | 功能 | 仓库无数据 | 返回空 assets，200 |
| 按 signals 查询 | 功能 | 仓库有匹配基因 | 返回匹配的基因列表 |
| 查询无匹配 | 功能 | 仓库有数据但无匹配 | 返回空 assets，200 |
| min_confidence 过滤 | 功能 | 基因 confidence=0.3 | min_confidence=0.5 时不返回 |
| limit 参数 | 功能 | limit=2 | 最多返回 2 条 |
| 缺少必需参数 | 异常 | 无 q 参数 | 400 Bad Request |
| 无效 API Key | 安全 | X-Api-Key: invalid | 401 Unauthorized |
| 缺失 API Key | 安全 | 无 X-Api-Key header | 401 Unauthorized |
| 健康检查 | 功能 | - | 200 + {"status":"ok"} |

## 3. 集成测试用例

```rust
// 场景：完整 Fetch 流程
async fn test_fetch_with_matching_genes() {
    // 1. 初始化内存数据库
    let store = SqliteGeneStore::new(":memory:").unwrap();
    // 2. 插入测试基因
    store.store_gene(gene_with_signals(&["timeout", "error"])).unwrap();
    // 3. 启动服务器
    let app = create_app(store).await;
    // 4. 发起请求
    let response = app.fetch("timeout", 0.5, 10).await;
    // 5. 验证
    assert!(response.assets.len() >= 1);
}
```

## 4. 风险评估

| 风险 | 概率 | 影响 | 缓解 |
|------|------|------|------|
| keyword matching 查不到语义相关基因 | 中 | 低 | MVP 接受，文档标注限制 |
| SQLite WAL 写入并发争用 | 低 | 中 | 监控连接数 |
| API Key 验证绕过后果 | 低 | 高 | MVP 仅内部使用 |

## 5. 放行建议

- [x] 6 个单元测试全部通过
- [x] 集成测试覆盖主路径
- [x] API Key 验证正确拒绝未授权请求
- [x] 错误响应格式一致
- [ ] 性能测试（建议 100ms 内响应）- 可选

**建议：可以放行 MVP**
