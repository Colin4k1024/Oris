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
| claude-code-evolution-integration | challenge-complete | docs/artifacts/2026-04-05-claude-code-evolution-integration/ |

### 任务摘要
将 Oris 自我进化能力集成到 Claude Code harness，采用 IPC (Unix Domain Socket) 接口形式，混合触发模式（高置信度自动固化），强制签名验证 + 来源标签 + 自动 revert。

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
| IPC 延迟 | 任务变慢 | 异步执行 |
| 误进化 | 知识污染 | 高阈值 + 自动 revert |
| 签名验证 | 开发成本 | 使用现有 Ed25519 |

## 最后更新

2026-04-05
