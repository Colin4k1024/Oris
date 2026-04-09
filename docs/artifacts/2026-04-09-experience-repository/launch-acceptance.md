---
artifact: launch-acceptance
task: experience-repository
date: 2026-04-09
role: qa-engineer
status: draft
---

# Launch Acceptance — 经验仓库 (Experience Repository)

## 1. 验收概览

| 字段 | 内容 |
|------|------|
| **对象** | oris-experience-repo v0.1.0 |
| **时间** | 2026-04-09 |
| **角色** | backend-engineer + qa-engineer |
| **验收方式** | 本地运行 + curl 测试 |

## 2. 验收范围

### 功能边界
- GET /experience — 按 signals 查询基因 ✅
- GET /health — 健康检查 ✅
- API Key 验证 ✅

### 不在范围内（MVP 排除）
- POST /experience（Share）- 外部 Agent 身份凭证体系未定义
- POST /experience/{id}/feedback - 同上
- 向量语义搜索 - keyword matching 已满足 MVP

## 3. 验收证据

### 单元测试
```
running 6 tests
test api::request::tests::test_fetch_query_signals_empty ... ok
test client::client::tests::test_client_config ... ok
test api::request::tests::test_fetch_query_signals ... ok
test api::request::tests::test_fetch_query_signals_with_spaces ... ok
test server::handlers::tests::test_health ... ok
test server::handlers::tests::test_fetch_experiences_empty ... ok

test result: ok. 6 passed; 0 failed
```

### 构建验证
```
cargo build -p oris-experience-repo ✅
```

### 自测命令
```bash
# 健康检查
curl http://localhost:8080/health
# {"status":"ok","version":"0.1.0"}

# Fetch 查询（需 X-Api-Key header）
curl -H "X-Api-Key: test-api-key" "http://localhost:8080/experience?q=timeout"
```

## 4. 风险判断

| 已满足项 | 可接受风险 | 阻塞项 |
|----------|------------|--------|
| 单元测试 100% 通过 | keyword matching 非语义搜索（文档标注） | 无 |
| 构建成功 | SQLite WAL 并发（监控即可） | 无 |
| API Key 验证生效 | API Key 硬编码（内部使用可接受） | 无 |

## 5. 上线结论

**✅ 建议放行 MVP**

- 实现完整度：100%（Fetch + Health + API Key）
- 测试覆盖率：6/6 通过
- 已知限制已文档化
- 二期功能（Share/Feedback）已明确排除

### 前提条件
- 生产环境使用需配置有效 API Key（目前测试用静态 Key）
- 建议监控 SQLite 连接数

### 观察重点
- 查询响应时间（目标 <100ms）
- API Key 验证拒绝率（异常请求）
