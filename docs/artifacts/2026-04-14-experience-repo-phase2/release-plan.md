---
artifact: release-plan
task: experience-repo-phase2
date: 2026-04-14
role: devops-engineer
status: draft
---

# Release Plan — 经验仓库二期 (Experience Repository Phase 2)

## 1. 发布信息

| 字段 | 内容 |
|------|------|
| 发布版本 | oris-experience-repo v0.2.0 |
| 发布时间 | 2026-04-14 |
| 发布类型 | MVP 内部发布 |
| 发布负责人 | tech-lead |
| 观察窗口 | 48 小时 |

## 2. 变更与风险

### 变更范围
- 新增 Share API（`POST /experience`）
- 新增 Key Management API（`GET/POST /keys`, `DELETE /keys/:key_id`, `POST /keys/:key_id/rotate`）
- 新增 OEN Envelope 解析
- 新增 Key Service（SQLite 存储）
- Ed25519 签名验证暂时禁用

### 风险评估

| 风险 | 概率 | 影响 | 缓解 |
|------|------|------|------|
| Ed25519 签名验证未启用 | 高 | 中 | MVP 内部使用，API Key 提供访问控制 |
| SQLite 并发写入瓶颈 | 低 | 中 | WAL 模式已配置 |
| API Key 无 rate limiting | 中 | 中 | MVP 仅内部使用 |

### 已接受风险
- sender_id 由客户端提供（签名验证启用后解决）
- SHA-256 无 salt（API Key 高熵，彩虹表攻击不可行）

## 3. 执行步骤

### 预检清单
- [x] 15/15 单元测试通过
- [x] cargo build --release 通过
- [x] cargo clippy 无警告
- [x] test-plan.md 已完成
- [x] launch-acceptance.md 建议放行

### 构建步骤
```bash
# 1. 构建 release 版本
cargo build -p oris-experience-repo --release

# 2. 构建 CLI 工具
cargo build -p oris-exp-repo-cli --release

# 3. 运行测试
cargo test -p oris-experience-repo --release

# 4. 运行 clippy
cargo clippy -p oris-experience-repo --release -- -D warnings
```

### 部署步骤
```bash
# 1. 启动服务器
cargo run -p oris-experience-repo --example server

# 2. 验证健康检查
curl http://localhost:8080/health

# 3. 创建测试 Key
curl -X POST http://localhost:8080/keys \
  -H "Content-Type: application/json" \
  -d '{"agent_id": "test-agent", "ttl_days": 30}'

# 4. 验证 Share API
curl -X POST http://localhost:8080/experience \
  -H "Content-Type: application/json" \
  -H "X-Api-Key: <your-api-key>" \
  -d '{"envelope": {...}}'
```

## 4. 验证与监控

### 验证命令
```bash
# 健康检查
curl -s http://localhost:8080/health | jq .

# 列出 Keys
curl -s http://localhost:8080/keys -H "X-Api-Key: <key>" | jq .

# 验证 Fetch 回归
curl -s "http://localhost:8080/experience?q=timeout&min_confidence=0.5" | jq .
```

### 监控指标
- Share API 响应时间（目标 <200ms）
- Key 创建/验证成功率
- API Key 验证拒绝率（异常请求）
- SQLite 连接数

## 5. 回滚方案

### 触发条件
- Share API 错误率 >5%
- 响应时间 P99 >1s
- 数据库连接失败

### 回滚步骤
1. 停止服务进程
2. `git revert HEAD`
3. 重新构建：`cargo build -p oris-experience-repo --release`
4. 重启服务
5. 验证健康检查

### 回滚验证
```bash
curl http://localhost:8080/health
# 期望: {"status":"ok","version":"0.2.0"}
```

## 6. 放行结论

### Go/No-Go 检查

| 检查项 | 状态 | 说明 |
|--------|------|------|
| 单元测试全部通过 | Go | 15/15 通过 |
| 代码编译无错误 | Go | release + debug 编译通过 |
| Share API 基本流程可用 | Go | Envelope 接收 + Gene 存储 |
| Key Management CRUD 可用 | Go | Create/List/Revoke/Rotate |
| Fetch API 回归正常 | Go | 原有功能不受影响 |
| 已知限制已文档化 | Go | test-plan.md 已标注 |

### 结论
**✅ 建议放行 MVP**

- 实现完整度：100%（Share + Key Management + Fetch）
- 测试覆盖率：15/15 通过
- 已知限制已文档化
- 二期功能（签名验证、Feedback）已明确排除

### 后续观察项
- Share API 响应时间（目标 <200ms）
- Key 创建/验证成功率
- API Key 验证拒绝率（异常请求）

## 7. 企业内控补充

| 字段 | 内容 |
|------|------|
| 应用等级 | T4（原型探索） |
| 技术架构等级 | L4（单体应用） |
| 关键组件偏离 | 无 |
| 资产文档入口 | docs/artifacts/2026-04-14-experience-repo-phase2/ |

### 合规说明
- MVP 仅供内部使用
- API Key 提供基本访问控制
- Ed25519 签名验证需二期启用 PKI 后才能生产使用
