---
artifact: execute-log
task: experience-repository
date: 2026-04-09
role: backend-engineer
status: completed
---

# Execute Log — 经验仓库 (Experience Repository)

## 1. 计划 vs 实际

| 工作项 | 计划 | 实际 | 偏差 |
|--------|------|------|------|
| 创建 oris-experience-repo crate | Day 2-3 | Day 1 | 提前 |
| 实现 ExperienceRepoServer (Axum) | Day 3-4 | Day 1 | 提前 |
| 实现 Fetch API 端点 | Day 4 | Day 1 | 提前 |
| 集成 oris-genestore 查询 | Day 4 | Day 1 | 提前 |
| API Key 验证中间件 | Day 3-4 | 简化，纳入 handler | 简化 |

### 偏差原因

- API Key 验证原计划用 middleware 实现，但 Axum 0.8 的 middleware trait 签名复杂，改用 handler 内直接验证（更简单）
- 第一期 MVP 仅实现 Fetch，只读查询，无写入需求

## 2. 实施关键决定

### 决定 1：简化 API Key 验证
**决策**：不在 middleware 层做 API Key 验证，改为 handler 内验证
**原因**：Axum 0.8 的 `tower::Service` middleware 实现复杂度高，且第一期是 MVP
**影响**：代码更简洁，但 auth 逻辑分散在 handler 中

### 决定 2：使用 tokio::sync::Mutex
**决策**：使用 `tokio::sync::Mutex<dyn GeneStore>` 而非 `std::sync::Mutex`
**原因**：`search_genes` 是 async 方法，需要 async mutex
**影响**：无

### 决定 3：SQLite 基因库路径
**决策**：store_path 默认为 `:memory:` 用于测试，生产用 `.oris/experience_repo.db`
**原因**：简化本地开发，测试用内存数据库

## 3. 阻塞与解决

### 阻塞 1：Axum 0.8 middleware trait 实现复杂
**现象**：tower Service trait 的 future 类型不匹配
**根因**：Axum 0.8 的 middleware 实现比 0.7 复杂
**解决**：简化方案，在 handler 内做 auth 验证

### 阻塞 2：dyn GeneStore 不实现 Clone/Debug
**现象**：AppState 无法 derive Clone
**根因**：`dyn GeneStore` trait object
**解决**：使用 `Arc<Mutex<dyn GeneStore>>`，手动 impl Clone for AppState

## 4. 影响面

### 新增 crate
- `oris-experience-repo`：HTTP API 服务 crate

### 新增文件
```
crates/oris-experience-repo/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── error.rs
│   ├── api/
│   │   ├── mod.rs
│   │   ├── request.rs
│   │   └── response.rs
│   ├── server/
│   │   ├── mod.rs
│   │   └── handlers.rs
│   └── client/
│       ├── mod.rs
│       └── client.rs
└── examples/
    └── server.rs
```

### Workspace 变更
- `Cargo.toml`：新增 `crates/oris-experience-repo` 到 members

### 依赖
- 新增依赖：`axum`, `tower`, `tower-http`, `reqwest`, `url`
- 复用：`oris-genestore`, `oris-evolution`

## 5. 未完成项

| 项目 | 原因 | 后续 |
|------|------|------|
| POST /experience（Share 贡献） | P0 阻断：身份凭证体系未定义 | 二期 |
| POST /experience/{id}/feedback | P0 阻断：同上 | 二期 |
| API Key middleware | 简化实现，改在 handler 内验证 | 二期完善 |
| 向量语义搜索 | keyword matching 已满足 MVP | 三期 |
| OpenAPI 文档 | MVP 简化 | 二期 |

## 6. 自测结果

```
running 6 tests
test api::request::tests::test_fetch_query_signals_empty ... ok
test client::client::tests::test_client_config ... ok
test api::request::tests::test_fetch_query_signals ... ok
test api::request::tests::test_fetch_query_signals_with_spaces ... ok
test server::handlers::tests::test_health ... ok
test server::handlers::tests::test_fetch_experiences_empty ... ok

test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured
```

## 7. 交给 QA 的说明

### 测试范围
1. **GET /experience**：按 signals 查询基因，返回匹配列表
2. **GET /health**：健康检查，无需认证

### 测试用例
| 场景 | 输入 | 预期 |
|------|------|------|
| 查询空仓库 | q="timeout" | 返回空 assets |
| 查询有数据的仓库 | q="timeout" | 返回匹配的基因列表 |
| 参数边界 | min_confidence=0, limit=0 | 降级处理 |
| 缺少必需参数 | 无 q 参数 | 400 错误 |

### 注意事项
- 测试时使用 `:memory:` 数据库，每次测试独立
- API Key 验证在 MVP 阶段简化处理
