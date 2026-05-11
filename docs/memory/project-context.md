# Project Context

## 项目信息

| 字段 | 内容 |
|------|------|
| 项目名 | Oris |
| 类型 | 自我进化执行运行时 |
| 语言 | Rust |
| 版本 | v0.61.0 |
| 仓库 | /Users/jiafan/Desktop/poc/Oris |

## Tech Stack

- **核心**: Rust (16 library crates, 6 example projects)
- **进化层**: oris-evolution, oris-evokernel, oris-mutation-evaluator
- **存储**: SQLite (oris-genestore)
- **沙箱**: oris-sandbox (OS 级隔离)
- **网络**: oris-evolution-network (Ed25519 签名)
- **经验仓库**: oris-experience-repo (HTTP API, PKI key service, OEN envelope)

## 当前任务

| 任务 | 状态 | 目录 |
|------|------|------|
| exp-repo-evokernel-wire | follow-up-required | docs/artifacts/2026-05-11-exp-repo-evokernel-wire/ |
| experience-repo-pki | closed | docs/artifacts/2026-04-14-experience-repo-pki/ |
| experience-repo-phase2 | released | docs/artifacts/2026-04-14-experience-repo-phase2/ |
| project-review-0414 | completed | docs/artifacts/2026-04-14-project-review/ |
| claude-code-evolution-integration | completed | docs/artifacts/2026-04-05-claude-code-evolution-integration/ |
| experience-repository | completed | docs/artifacts/2026-04-09-experience-repository/ |

### 任务摘要

**exp-repo-evokernel-wire**：R1–R8 集成胶水全部交付，579 tests 通过。PKI 网络推送路径受 3 个 P0 问题阻塞（空签名、sender_id 语义、Key 管理端点鉴权），不得对外开放，计划 v0.4.0 修复。

**experience-repo-pki**：PKI 公钥注册表 + Ed25519 签名验证 + Rate Limiting（38/38 通过）

**experience-repo-phase2**：OEN Envelope + Ed25519 签名验证 + Key Service

**experience-repository**：HTTP API MVP（Fetch 只读查询）

## 关键依赖

| 依赖 | 版本 | 用途 |
|------|------|------|
| oris-evolution | 0.4.1 | 核心进化类型和 Pipeline |
| oris-evokernel | 0.14.1 | 编排层 |
| oris-genestore | 0.2.0 | SQLite Gene 存储 |
| oris-sandbox | 0.3.0 | 沙箱执行 |
| oris-mutation-evaluator | 0.3.0 | 两阶段评估 |
| oris-evolution-network | 0.5.0 | OEN 网络、gossip、Ed25519 |
| oris-experience-repo | 0.3.0 | HTTP 经验仓库 |

## 活跃风险

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| P0：`publish_envelope` 产生空签名，服务端 Ed25519 验证必然失败 | PKI push 不可用 | v0.4.0 修复前不开放 `with_network_publisher()` |
| P0：`sender_id` 使用 gene.id 违反 OEN identity 语义 | 身份绑定验证被拒绝 | 依赖节点身份 feature，v0.4.0 处理 |
| P0：Key 管理端点缺少 `verify_key` 鉴权（CRITICAL） | 任意人可操作 API Key | 独立 issue，下一迭代立即处理 |
| P1：reqwest 无超时配置 | 推送阻塞 | v0.4.0 |

## 下一步

- 打开独立 issue 修复 Key 管理端点鉴权（P0，立即）
- 规划 v0.4.0 sprint（P0 签名问题 + sender_id + 3 处晋升路径补全）
- E2E 集成测试（P2，下一迭代有 HTTP 测试环境时）

## 最后更新

2026-05-11
