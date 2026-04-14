---
artifact: prd
task: project-review-0414
date: 2026-04-14
role: tech-lead
status: draft
---

# 项目审查 (Project Review) — 2026-04-14

## 1. 背景与问题

Oris 是一个自进化执行运行时，当前已完成 `experience-repository` MVP 发布，需对项目整体状态进行定期审查，识别：
- 已完成工作的收口状态
- 悬空的待确认项和阻塞项
- 下一阶段的优先级

## 2. 当前任务状态总览

| 任务 | 状态 | 完成度 | 遗留项 |
|------|------|--------|--------|
| claude-code-evolution-integration | completed | 100% | 无 |
| experience-repository | completed | MVP 100% | Share/Feedback 延后二期 |

### experience-repository MVP 收口状态

| 交付物 | 状态 | 证据 |
|--------|------|------|
| PRD | ✅ | prd.md |
| Arch Design | ✅ | arch-design.md |
| Delivery Plan | ✅ | delivery-plan.md |
| Execute Log | ✅ | execute-log.md |
| Test Plan | ✅ | test-plan.md |
| Launch Acceptance | ✅ | launch-acceptance.md |

## 3. 最新代码变更（2026-04-13）

### 新增 Workflow
- `.github/workflows/publish-experience-repo.yml` — 发布 experience-repo 到 crates.io

### 版本更新
| Crate | 版本变更 |
|-------|----------|
| evolution-cli | v0.1.0（新发布） |
| oris-execution-server | v0.2.12 |
| oris-experience-repo | v0.1.0 |

### 未提交更改
| 文件 | 变更 | 目的 |
|------|------|------|
| `oris-evo-ipc-protocol/Cargo.toml` | +keywords, +categories | crates.io 发布元数据 |

## 4. 活跃风险与阻塞项

### P0 阻断项

| 阻断项 | 影响功能 | 当前状态 | 建议 |
|--------|----------|----------|------|
| 外部 Agent 身份凭证体系未定义 | Share、Feedback | 文档化，二期解决 | 二期前需完成设计 |

### 已知限制（已文档化）

| 限制 | 影响 | 缓解 |
|------|------|------|
| keyword matching 非语义搜索 | 查询质量 | MVP 接受，标注限制 |
| SQLite WAL 并发 | 高并发写入 | 监控连接数 |
| API Key 硬编码 | 安全性 | 内部使用可接受，二期引入 Key Service |

## 5. 待确认项清单

| 项目 | 状态 | Owner | 优先级 |
|------|------|-------|--------|
| API Key 格式和存储方式 | 未确认 | architect | P2 |
| Feedback 功能是否在二期纳入 | 未确认 | product-manager | P2 |
| 是否需要 Rate Limiting | 未确认 | architect | P2 |
| oris-evo-ipc-protocol 发布计划 | 待提交 | backend-engineer | P1 |

## 6. 下一阶段建议

### 二期候选功能（经验仓库）
1. **Share 功能** — 需要先解决身份凭证体系
2. **Feedback 功能** — 依赖 Share 功能后的评价机制
3. **向量语义搜索** — 提升查询质量
4. **API Key Service** — 动态 Key 管理和轮换

### 其他候选任务
1. **evolution-cli 完善** — v0.1.0 已发布，可继续增强
2. **oris-evo-ipc-protocol 发布** — 准备 crates.io 发布

## 7. 参与角色清单

| 角色 | 输入缺口 | 优先级 |
|------|----------|--------|
| tech-lead | 确认二期优先级和范围 | P0 |
| architect | API Key Service 设计 | P1 |
| backend-engineer | 提交 oris-evo-ipc-protocol 更改 | P1 |

## 8. 企业治理状态

| 维度 | 状态 | 说明 |
|------|------|------|
| 应用等级 | T4 | 原型探索 |
| 数据合规 | 无 | 不涉及敏感数据 |
| 集团组件约束 | 无 | 纯 Rust 项目 |

## 9. 需求挑战会候选分组

### 议题 1：二期范围确认
- **参与者**：tech-lead, architect, backend-engineer
- **议题**：Share 功能是否需要完整的身份凭证体系，还是可以使用简化方案

### 议题 2：API Key Service 设计
- **参与者**：architect, backend-engineer
- **议题**：Key 的生成、存储、轮换、撤销机制

## 10. 行动项

| # | 行动 | Owner | 状态 |
|---|------|-------|------|
| 1 | 提交 oris-evo-ipc-protocol 更改 | backend-engineer | pending |
| 2 | 确认二期 Share 功能方案 | tech-lead | pending |
| 3 | 设计 API Key Service | architect | pending |

## 11. 下次审查时间建议

建议在以下事件后进行下次审查：
- Share 功能方案确定
- 或 2026-05-01（一个月后定期审查）
