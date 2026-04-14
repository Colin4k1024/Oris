---
artifact: deployment-context
task: experience-repo-phase2
date: 2026-04-14
role: devops-engineer
status: draft
---

# Deployment Context — 经验仓库二期 (Experience Repository Phase 2)

## 1. 环境清单

| 环境 | 用途 | 访问入口 | 部署目标 |
|------|------|----------|----------|
| 本地开发 | 开发调试 | `127.0.0.1:8080` | 本地 SQLite |
| CI/CD | GitHub Actions 构建验证 | GitHub Actions | cargo build + test |
| 生产（未来） | 正式运营 | TBD | TBD |

## 2. 部署入口

### 本地运行
```bash
# 构建
cargo build -p oris-experience-repo

# 运行示例服务器
cargo run -p oris-experience-repo --example server

# 运行 CLI 工具
cargo run -p oris-exp-repo-cli -- --help
```

### 前置条件
- Rust 1.75+
- SQLite 3.x
- 开放端口 8080（如需远程访问）

### 回滚入口
- 代码级别回滚：`git revert <commit>`
- 二进制替换：重新构建并替换

## 3. 配置与密钥

### 环境变量
| 变量 | 默认值 | 说明 |
|------|--------|------|
| `ORIS_EXP_REPO_PORT` | `8080` | HTTP 服务端口 |
| `ORIS_EXP_REPO_STORE_PATH` | `:memory:` | Gene 存储路径（开发用内存） |
| `ORIS_EXP_REPO_KEY_STORE_PATH` | `./key_store.db` | API Key 存储路径 |

### 示例配置
```bash
export ORIS_EXP_REPO_PORT=8080
export ORIS_EXP_REPO_STORE_PATH=/data/experience.db
export ORIS_EXP_REPO_KEY_STORE_PATH=/data/key_store.db
```

## 4. 运行保障

### Feature Flag
- Ed25519 签名验证：`DISABLED`（需 PKI 实现后启用）

### 监控指标
- Share API 响应时间（目标 <200ms）
- Key 创建/验证成功率
- API Key 验证拒绝率
- SQLite 连接数

### 告警阈值
- 响应时间 >500ms 持续 5 分钟
- 错误率 >1% 持续 1 分钟

### 值守安排
- MVP 阶段无 7x24 值守需求
- 问题记录至 GitHub Issues

### 观察窗口
- 上线后 48 小时内每 4 小时检查一次

## 5. 恢复能力

### 回滚触发条件
- Share API 错误率 >5%
- 响应时间 P99 >1s
- 数据库连接失败

### 回滚路径
1. 停止服务
2. `git revert` 回到上一稳定版本
3. 重新构建
4. 验证服务正常

### 验证方法
```bash
# 健康检查
curl http://localhost:8080/health

# 功能验证
curl -X POST http://localhost:8080/keys \
  -H "Content-Type: application/json" \
  -d '{"agent_id": "test", "ttl_days": 30}'
```
