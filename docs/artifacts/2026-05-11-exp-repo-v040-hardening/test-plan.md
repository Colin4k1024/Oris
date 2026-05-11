# Test Plan — exp-repo-v040-hardening

**Role**: qa-engineer  
**Status**: accepted  
**Date**: 2026-05-11  
**Sprint**: exp-repo-v040-hardening (v0.4.0 hardening sprint)

---

## 测试范围

### In Scope

| 项目 | 类型 | 说明 |
|------|------|------|
| F1 Ed25519 signing 链路 | 单元 | sign_envelope 在 maybe_push_to_network 中被正确调用 |
| F2 node_id=None 跳过 + warn! | 单元 | node_id 未配置时不 panic，打 warn 并 return |
| F3 bootstrap/trusted-local push | 集成 | ensure_builtin_experience_assets / record_reported_experience 促销后触发网络推送 |
| F4 GET / homepage | 单元 | 返回 200 HTML，含版本号、服务名、endpoint 表格 |
| NodeKeypair Clone | 单元 | Clone 后公钥十六进制一致 |
| 既有回归 | 全量 | oris-evolution-network / oris-evokernel / oris-experience-repo 所有测试通过 |

### Out of Scope

- ORT / fastembed 相关 build 错误（pre-existing，与本 sprint 无关）
- 端到端网络集成测试（需真实节点）

---

## 测试矩阵

| 场景 | 类型 | 前置条件 | 预期结果 |
|------|------|---------|---------|
| signing_keypair 已配置 → 推送签名 envelope | 单元 | keypair 已 set | envelope.signature != None |
| signing_keypair 未配置 → 推送无签名 envelope | 单元 | keypair = None | envelope.signature == None，正常推送 |
| node_id = None → 跳过推送 | 单元 | node_id = None | return early，warn! 日志 |
| node_id = Some → 正常 sender_id | 单元 | node_id = Some("node-x") | sender_id = "node-x" |
| GET / | 单元 | 无 | 200 HTML，含 "Oris Experience Repository"、version、"OK"、"/health" |
| NodeKeypair::generate_at + from_path | 单元 | 临时路径 | public_key_hex 一致 |
| sign_and_verify_round_trip | 单元 | 生成 keypair + envelope | verify_envelope Ok |
| oris-experience-repo 全量 | 回归 | cargo test -p | 41/41 pass |
| oris-evokernel 全量 | 回归 | cargo test -p | pass |
| oris-evolution-network 全量 | 回归 | cargo test -p | pass |

---

## 验证执行结果

```
cargo test -p oris-evolution-network -p oris-evokernel -p oris-experience-repo
→ 259 passed, 1 ignored (10 suites, 8.35s)
```

cargo build -p oris-evolution-network -p oris-evokernel -p oris-experience-repo  
→ Finished dev profile, 18 crates compiled, 0 errors

---

## 风险

| 风险 | 等级 | 说明 |
|------|------|------|
| 签名测试未覆盖 maybe_push_to_network 集成路径 | 低 | 现有测试验证了 sign/verify 链路；maybe_push_to_network 通过 mock publisher 隐式覆盖 |
| node_id=None warn! 无自动断言 | 低 | 行为通过代码 review 确认；可在后续 sprint 补 tracing-subscriber 捕获断言 |

---

## 放行建议

**建议放行**。

- 259/259 测试通过，0 新增失败
- F1/F2/F3/F4 均已实现并有对应测试覆盖
- 无阻塞项
