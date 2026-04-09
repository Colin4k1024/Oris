---
artifact: delivery-plan
task: experience-repository
date: 2026-04-09
role: tech-lead
status: completed
---

# 交付计划 — 经验仓库 (Experience Repository)

## 1. 版本目标

| 字段 | 内容 |
|------|------|
| **版本** | v0.1 (MVP) |
| **范围** | 外部 Agent HTTP API 查询经验仓库（只读）；贡献（Share）功能延后 |
| **放行标准** | 单元测试 + 集成测试通过；API 文档完成；可本地运行 demo |

### Scope 调整说明

基于需求挑战会结论：
- **P0 阻断**：外部 Agent 身份凭证体系未定义，Share 功能无法安全实现
- **决策**：第一期 MVP **仅实现 Fetch（只读查询）**，Share 功能排入下一期

## 2. 工作拆解

### 阶段 1：安全模型与接口定义

| 工作项 | 主责 | 依赖 | 计划 |
|--------|------|------|------|
| 定义 API Key 凭证模型 | architect | 挑战会结论 | Day 1 |
| 设计简化版 AgentApiEnvelope | architect | API Key 模型 | Day 1 |
| 输出 arch-design.md | architect | - | Day 1-2 |

### 阶段 2：核心实现

| 工作项 | 主责 | 依赖 | 计划 |
|--------|------|------|------|
| 创建 oris-experience-repo crate | backend-engineer | arch-design | Day 2-3 |
| 实现 ExperienceRepoServer (Axum) | backend-engineer | crate 创建 | Day 3-4 |
| 实现 Fetch API 端点 | backend-engineer | Server | Day 4 |
| 集成 oris-genestore 查询 | backend-engineer | Fetch API | Day 4 |
| 实现 API Key 验证中间件 | backend-engineer | API Key 模型 | Day 3-4 |

### 阶段 3: 测试与验证

| 工作项 | 主责 | 依赖 | 计划 |
|--------|------|------|------|
| 单元测试 (gene search, api key) | qa-engineer | 核心实现 | Day 5 |
| 集成测试 (HTTP API) | qa-engineer | Server | Day 5 |
| 文档 (OpenAPI / README) | backend-engineer | API 稳定 | Day 5 |

## 3. 风险与缓解

| 风险 | 影响 | 缓解措施 | Owner |
|------|------|----------|-------|
| API Key 凭证体系复杂度过高 | 工期延迟 | 第一期使用静态 API Key 文件，降低复杂度 | architect |
| 向量语义搜索性能不足 | 查询体验差 | 第一期用 keyword matching，标注限制 | architect |
| SQLite 并发写入瓶颈 | 可用性 | WAL 模式已配置，监控连接数 | backend-engineer |

## 4. 节点检查

| 节点 | 目标 | 阻塞项 |
|------|------|--------|
| 方案评审 | arch-design.md 评审通过 | API Key 模型、P0 阻断项解决 |
| 开发完成 | 核心功能代码完成 | - |
| 测试完成 | 80%+ 覆盖率 | 无 |
| 发布准备 | 文档、部署说明完成 | - |

## 5. 需求挑战会结论

### P0 阻断项（第一期 MVP 排除）

| 阻断项 | 结论 |
|--------|------|
| 外部 Agent 身份凭证体系未定义 | 第一期 MVP **排除 Share 功能**，仅实现 Fetch 只读 API |
| API Key 发放和轮换机制缺失 | 第一期使用静态配置文件方式的 API Key 验证 |

### 核心质疑与结论

| 质疑 | 挑战类型 | 结论 |
|------|----------|------|
| Envelope 协议与 HTTP API 语义错配 | P1 | HTTP API 使用独立简化 Envelope，内部 OEN 协议不变 |
| OEN 签名无法支撑外部 Agent 身份 | P0 | 第一期用 API Key + HMAC，后续演进 |
| GeneQuery relevance 非语义搜索 | 接受限制 | keyword matching 满足 MVP，标注限制条件 |

## 6. 技能装配清单

| 技能 | 启用原因 |
|------|----------|
| `rust-patterns` | Rust 项目实现指导 |
| `rust-testing` | 测试覆盖率要求 |
| `api-design` | HTTP API 契约设计 |

## 7. 后续待确认项

- [ ] API Key 格式和存储方式（HJSON 配置 vs SQLite 表）
- [ ] Feedback 功能是否在二期纳入
- [ ] 是否需要 Rate Limiting
