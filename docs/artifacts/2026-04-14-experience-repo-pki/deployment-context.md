---
artifact: deployment-context
task: experience-repo-pki
date: 2026-04-14
role: devops-engineer
status: draft
---

# Deployment Context — Experience Repository PKI Completion

## 1. 发布概述

| 字段 | 内容 |
|------|------|
| **发布对象** | oris-experience-repo v0.2.1 (PKI) |
| **发布类型** | Rust Crate 发布（crates.io） |
| **发布窗口** | 2026-04-14 |
| **主责角色** | backend-engineer + qa-engineer |

## 2. 环境信息

### 构建环境
- **Rust 版本**: 1.75+ (stable)
- **构建命令**: `cargo build -p oris-experience-repo --release`
- **测试命令**: `cargo test -p oris-experience-repo --release`
- **目标平台**: macOS (Apple Silicon) + Linux x86_64

### 依赖环境
- **SQLite**: 3.x (rusqlite)
- **运行时**: tokio 1.x

## 3. 部署入口

### 发布前检查
```bash
# 1. 格式化检查
cargo fmt --all -- --check

# 2. Lint 检查
cargo clippy -p oris-experience-repo -- -D warnings

# 3. 构建验证
cargo build -p oris-experience-repo --release

# 4. 运行测试
cargo test -p oris-experience-repo --release
```

### 发布命令
```bash
# Dry run (推荐先执行)
cargo publish -p oris-experience-repo --dry-run

# 正式发布
cargo publish -p oris-experience-repo
```

### 版本管理
- 当前版本: 0.2.1
- 后续版本遵循 semver

## 4. 配置与密钥

### 运行时配置
| 配置项 | 来源 | 说明 |
|--------|------|------|
| `store_path` | CLI 参数 | SQLite 数据库路径 |
| `key_store_path` | CLI 参数 | API Key 存储路径 |
| `server_addr` | 环境变量 / CLI | HTTP 服务地址 |

### 密钥管理
- API Key: 运行时生成，不持久化到代码
- Ed25519 公钥: 存储在 public_keys 表

## 5. 回滚入口

### 场景 1: 版本发布后发现问题
```bash
# 回滚到上一版本
cargo publish -p oris-evolution-network --dry-run  # 验证版本号
# 注意: crates.io 不支持覆盖已发布版本
# 需要发布新补丁版本修复问题
```

### 场景 2: 依赖方需要降级
- 在 Cargo.toml 中指定旧版本:
```toml
oris-experience-repo = "=0.2.0"  # 锁定到 0.2.0
```

## 6. 运行保障

### Feature Flags
| Flag | 说明 |
|------|------|
| 默认 | 全功能启用 |

### 健康检查
- `GET /health` — 返回 `{"status": "ok"}`

### 监控指标
- Ed25519 签名验证失败率
- PKI 公钥注册成功率
- Rate Limiting 触发次数

## 7. 值守与观察窗口

### 发布后观察 (24h)
- 签名验证错误率是否异常
- Rate Limiting 是否误伤正常请求
- 公钥注册是否正常

### 回滚触发条件
- 签名验证失败率 > 10%
- 服务无法启动
- 关键端点返回 5xx 错误

## 8. 企业内控补充

| 项 | 说明 |
|-----|------|
| 应用等级 | T4 (内部工具) |
| 技术架构等级 | 简单架构 (单服务) |
| 关键组件偏离 | 无 |
| 资源隔离 | 不适用 (本地运行) |
