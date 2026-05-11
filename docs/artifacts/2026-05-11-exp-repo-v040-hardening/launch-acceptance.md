# Launch Acceptance — exp-repo-v040-hardening

**Role**: qa-engineer  
**Status**: accepted  
**Date**: 2026-05-11  
**Sprint**: exp-repo-v040-hardening

---

## 验收概览

| 字段 | 值 |
|------|---|
| 验收对象 | exp-repo-v040-hardening sprint（F1–F4） |
| 验收时间 | 2026-05-11 |
| 验收角色 | qa-engineer |
| 验收方式 | cargo test 全量 + code review |

---

## 验收范围

**In Scope**
- F1: 三个结构体（StoreReplayExecutor、EvolutionNetworkNode、EvoKernel<S>）的 signing_keypair 字段 + with_signing_keypair() builder
- F2: maybe_push_to_network 中 node_id=None 时跳过推送并打 warn!
- F3: bootstrap 和 trusted-local 促销路径已通过 F1/F2 修复自动覆盖
- F4: GET / 首页 handler，返回 HTML 服务状态页

**Out of Scope**
- ORT/fastembed 预存在 build 错误
- 真实网络节点端到端集成

---

## 验收证据

| 证据 | 结果 |
|------|------|
| cargo test -p oris-evolution-network -p oris-evokernel -p oris-experience-repo | 259 passed, 1 ignored |
| cargo build -p（3 crates） | 0 errors |
| NodeKeypair Clone impl | 单元测试覆盖 |
| sign_and_verify_round_trip | 通过 |
| test_homepage_contains_version_and_status | 通过 |
| F2 node_id skip 路径 | 代码 review 确认 |

---

## 风险判断

| 项目 | 状态 |
|------|------|
| 259 tests pass | ✅ 满足 |
| 无新增 CRITICAL/HIGH | ✅ 满足 |
| F1 signing 链路 | ✅ 满足 |
| F2 sender_id 语义修正 | ✅ 满足 |
| F4 homepage | ✅ 满足 |
| ORT build 错误 | ⚠️ 可接受风险（pre-existing，与本 sprint 无关） |

**阻塞项**: 无

---

## 上线结论

**允许上线**。

所有 F1–F4 目标已达成，测试全绿，无阻塞项。可推进 release。

**前提条件**: 无  
**观察重点**: 生产环境首次运行时确认 signing_keypair 配置正确；node_id=None 的 warn! 日志不应出现在正常配置的节点上
