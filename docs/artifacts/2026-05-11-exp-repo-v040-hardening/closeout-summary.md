# Closeout Summary — exp-repo-v040-hardening

**Role**: tech-lead  
**Status**: closed  
**Date**: 2026-05-11  
**Sprint**: exp-repo-v040-hardening (v0.4.0 hardening)

---

## 收口对象

| 字段 | 值 |
|------|---|
| 关联任务 | exp-repo-v040-hardening |
| 发布目标 | oris-evolution-network + oris-evokernel + oris-experience-repo |
| 收口角色 | tech-lead |
| 观察窗口 | 本地 CI（cargo test 259/259），无生产环境 |

---

## 结果判断

**当前状态：closed**

所有 4 个修复项（F1–F4）已全部实现并通过测试：

| 项 | 描述 | 状态 |
|----|------|------|
| F1 | Ed25519 signing 链路打通（3 个结构体 + builder） | ✅ 完成 |
| F2 | sender_id 使用节点 ID；node_id=None 时跳过推送 + warn! | ✅ 完成 |
| F3 | bootstrap/trusted-local 促销路径触发网络推送（F1/F2 已覆盖） | ✅ 完成 |
| F4 | GET / 首页 handler（HTML，含版本、状态、API 表格） | ✅ 完成 |

测试结果：259 passed, 1 ignored，0 regressions。

---

## 残余事项

| 事项 | 类型 | 处置 |
|------|------|------|
| maybe_push_to_network 无 tracing 断言测试 | 技术债 | 接受，后续 sprint 补 |
| ORT/fastembed build 错误 | 预存在问题 | 与本 sprint 无关，保持现状 |
| v040-backlog-fixes 旧团队成员无法终止 | 平台限制 | 僵尸进程将自然超时，不影响功能 |

---

## 知识沉淀

1. **F3 已隐式覆盖**：`ensure_builtin_experience_assets` 和 `record_reported_experience` 在循环内已调用 `maybe_push_to_network`，修复 F1/F2 即自动修复 F3，无需额外改动 `_in_store` 函数。
2. **NodeKeypair Clone**：`SigningKey` 无 derive(Clone)，需手动实现通过 `to_bytes()` 深拷贝。
3. **Arc<NodeKeypair>** 跨结构体共享签名密钥，避免重复持有字节副本。

---

## Backlog 回填

以下内容已同步到 `docs/memory/backlog.md`：
- [ ] maybe_push_to_network 补 tracing-subscriber 捕获断言测试
- [ ] ORT/fastembed build 错误根因调查（--all-features build 时）

---

## 任务关闭结论

**任务状态：closed**  
exp-repo-v040-hardening sprint 全部目标达成，无阻塞遗留，可归档。
