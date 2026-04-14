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

## 当前任务

| 任务 | 状态 | 目录 |
|------|------|------|
| experience-repo-pki | closed | docs/artifacts/2026-04-14-experience-repo-pki/ |
| experience-repo-phase2 | released | docs/artifacts/2026-04-14-experience-repo-phase2/ |
| project-review-0414 | completed | docs/artifacts/2026-04-14-project-review/ |
| claude-code-evolution-integration | completed | docs/artifacts/2026-04-05-claude-code-evolution-integration/ |
| experience-repository | completed | docs/artifacts/2026-04-09-experience-repository/ |

### 任务摘要

**experience-repo-pki**：PKI 公钥注册表 + Ed25519 签名验证 + Rate Limiting + 公钥版本管理（25 单元测试 + 13 集成测试 = 38/38 通过，100% 完成度）

**experience-repo-phase2**：实现 Share 功能（OEN Envelope + Ed25519 签名验证 + Key Service），预计工期 10 天

**experience-repository**：构建 Oris 经验仓库的 HTTP API 服务（第一期 MVP 已完成 Fetch 只读查询）

## 关键依赖

| 依赖 | 版本 | 用途 |
|------|------|------|
| oris-evolution | 0.4.1 | 核心进化类型和 Pipeline |
| oris-evokernel | 0.14.1 | 编排层 |
| oris-genestore | 0.2.0 | SQLite Gene 存储 |
| oris-sandbox | 0.3.0 | 沙箱执行 |
| oris-mutation-evaluator | 0.3.0 | 两阶段评估 |

## 活跃风险

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| 外部 Agent 身份凭证体系未定义 | P0 阻断 Share 功能 | 第一期 MVP 仅实现 Fetch 只读 |
| API Key 安全模型待设计 | 安全风险 | 二期引入 Key Service |
| keyword matching 非语义搜索 | 查询质量限制 | 标注限制，二期引入向量搜索 |

## 最后更新

2026-04-14
