---
artifact: release-plan
task: experience-repo-pki
date: 2026-04-14
role: devops-engineer
status: draft
---

# Release Plan — Experience Repository PKI Completion

## 1. 发布信息

| 字段 | 内容 |
|------|------|
| **任务** | experience-repo-pki |
| **发布对象** | oris-experience-repo v0.2.1 |
| **发布时间** | 2026-04-14 |
| **发布方式** | crates.io 发布 |
| **主责角色** | backend-engineer |

## 2. 变更与风险

### 变更范围

| 组件 | 变更类型 | 说明 |
|------|----------|------|
| PKI 公钥管理 API | 新增 | POST/GET/DELETE /public-keys |
| Ed25519 签名验证 | 启用 | share_experience 中完整验证 |
| Rate Limiting | 新增基础设施 | 30/min for POST /experience |

### 风险评估

| 风险 | 概率 | 影响 | 缓解 |
|------|------|------|------|
| 签名缓存允许短时间重放攻击 | 低 | 中 | 5分钟窗口，API Key 额外保护 |
| Rate Limiting 仅部分端点 | 中 | 低 | 基础设施已就绪可扩展 |
| 公钥撤销后缓存未立即失效 | 低 | 低 | 5分钟 TTL 自动失效 |

### 已接受风险

| 风险 | 影响 | 接受理由 |
|------|------|----------|
| 签名缓存允许 5 分钟重放 | 低 | 攻击窗口有限 |
| Rate Limiting 仅 POST /experience | 中 | 基础设施已就绪 |
| 无公钥版本管理 | 低 | MVP 阶段足够 |

## 3. 执行步骤

### Phase 1: 发布前验证
- [x] 18/18 单元测试通过
- [x] cargo build --release 通过
- [x] cargo fmt --check 通过
- [x] cargo clippy -D warnings 通过

### Phase 2: crates.io 发布
```bash
# 1. 确认 Cargo.toml 版本
grep 'version = ' crates/oris-experience-repo/Cargo.toml

# 2. Dry run
cargo publish -p oris-experience-repo --dry-run

# 3. 正式发布
cargo publish -p oris-experience-repo
```

### Phase 3: 依赖方更新（可选）
```toml
# 依赖方 Cargo.toml
oris-experience-repo = "0.2.1"
```

## 4. 验证与监控

### 启动后验证
```bash
# 启动服务
cargo run -p oris-experience-repo --example server

# 健康检查
curl http://localhost:8080/health

# 公钥注册 (需要有效 API Key)
curl -X POST http://localhost:8080/public-keys \
  -H "X-Api-Key: sk_live_xxx" \
  -H "Content-Type: application/json" \
  -d '{"sender_id": "agent-123", "public_key_hex": "..."}'
```

### 监控指标
- Ed25519 签名验证失败率
- Rate Limiting 429 响应率
- PKI 公钥注册成功率

## 5. 回滚方案

### 回滚触发条件
- 签名验证失败率异常升高 (>10%)
- 服务无法正常启动
- 关键 API 返回 5xx 错误

### 回滚步骤
1. 停止当前服务
2. 在 Cargo.toml 中锁定旧版本:
   ```toml
   oris-experience-repo = "=0.2.0"
   ```
3. 重新构建并部署

### 无法回滚场景
crates.io 不支持删除已发布版本。如需修复，发布新补丁版本即可。

## 6. 放行结论

**建议: 放行**

| 检查项 | 状态 |
|--------|------|
| 所有测试通过 | ✅ 18/18 |
| 构建无错误 | ✅ |
| PKI 端点已认证 | ✅ |
| Ed25519 验证已启用 | ✅ |
| Rate Limiting 基础设施就绪 | ✅ |
| 已知限制已文档化 | ✅ |

### 前提条件
- 生产环境使用需配置有效 API Key
- 建议监控签名验证失败率

### 观察重点
- PKI 公钥注册成功率
- 签名验证失败率
- Rate Limiting 触发情况

## 7. 后续改进项

| 优先级 | 项 | Owner |
|--------|-----|-------|
| P1 | 完整 Rate Limiting middleware 集成 | backend-engineer |
| P2 | 公钥版本管理 | architect |
| P2 | 集成测试完善 | qa-engineer |
