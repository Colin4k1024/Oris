---
artifact: prd
task: claude-code-evolution-integration
date: 2026-04-05
role: tech-lead
status: plan-complete
---

# PRD — 将 Oris 自我进化能力集成到 Claude Code

## 1. 背景

### 业务问题
Claude Code 当前是一个静态的代码生成和修改工具，缺乏自我进化的能力——即从错误中学习、固化成功模式、避免重复犯错的能力。Oris 提供了完整的自我进化运行时，包括：检测(Signal) → 选择(Select) → 突变(Mutate) → 执行(Execute) → 验证(Validate) → 评估(Evaluate) → 固化(Solidify) → 重用(Reuse) 的完整生命周期。

### 触发原因
用户希望 Claude Code 能够：
- 自动从编译错误、测试失败、panic 中学习
- 将成功的修复模式固化为可重用的基因(Gene)
- 在后续任务中复用学到的知识，减少重复推理

### 当前约束
- 必须采用 Oris 当前的核心代码，不做大规模重构
- 集成目标是 Claude Code 的 agent harness（`~/.claude/`）

---

## 2. 目标与成功标准

### 业务目标
让 Claude Code 具备自我进化能力，实现"一次学习，永久受益"的开发体验。

### 用户价值
- 重复出现的同类错误能被自动避免
- 团队最佳实践可跨会话共享
- 减少 LLM token 消耗（复用而非重推理）

### 成功指标
| 指标 | 目标 |
|------|------|
| 错误模式识别率 | ≥ 80%（相同根因的错误不出现第二次） |
| Gene 复用率 | ≥ 30%（30% 的修复来自基因库） |
| Token 节省 | ≥ 20%（相比无进化基线） |
| 进化延迟 | < 500ms（不影响正常任务流） |

---

## 3. 用户故事

### 用户故事 1：自动错误学习
> 作为用户，我希望 Claude Code 能自动从编译错误中学习，这样下次遇到类似问题时能直接给出正确解决方案。

**验收标准：**
- 编译错误发生 → 3秒内生成 Mutation Proposal
- 评估通过 → 自动写入 Gene Pool
- 相同错误再次出现 → 直接复用，不触发完整推理

### 用户故事 2：跨会话知识共享
> 作为用户，我希望在当前会话学到的修复模式能在后续新会话中使用。

**验收标准：**
- Gene Pool 持久化到本地 SQLite
- 新会话启动时加载历史 Gene
- 通过语义相似度匹配触发复用

### 用户故事 3：安全隔离执行
> 作为用户，我希望突变代码在沙箱中执行验证，避免污染主代码库。

**验收标准：**
- `oris-sandbox` 提供 OS 级资源隔离
- 验证失败不触发副作用
- 验证通过才写入主分支

---

## 4. 范围

### In Scope
- 集成 `oris-evolution` 核心 pipeline（Detect → Solidify → Reuse）
- 集成 `oris-mutation-evaluator` 两阶段评估
- 集成 `oris-genestore` SQLite 基因存储
- 集成 `oris-sandbox` 沙箱执行
- Claude Code harness 插件接口设计
- 本地 Gene Pool 管理（CRUD + 语义检索）

### Out of Scope
- `oris-evolution-network`（多节点 gossip 同步，v1.0+）
- `oris-governor`（复杂 promotion 策略，v1.0+）
- 云端 Gene Pool 服务
- 前端 UI 可视化基因库
- Claude Code 外部 API 改造

---

## 5. 技术架构

### 集成架构

```
Claude Code (Harness)
       │
       ▼
┌─────────────────────────────────────────────────────────┐
│            Evolution Integration Layer                   │
│  ┌─────────────┐  ┌─────────────┐  ┌───────────────┐  │
│  │SignalDetect │→│MutationEval │→│  GenePoolMgr  │  │
│  └─────────────┘  └─────────────┘  └───────────────┘  │
│         │                │                  │          │
│         ▼                ▼                  ▼          │
│  ┌─────────────┐  ┌─────────────┐  ┌───────────────┐  │
│  │ oris-evo    │  │oris-mutation│  │ oris-genestore│  │
│  │ -kernel     │  │ -evaluator  │  │  (SQLite)     │  │
│  └─────────────┘  └─────────────┘  └───────────────┘  │
│                        │                               │
│                        ▼                               │
│                 ┌─────────────┐                        │
│                 │oris-sandbox│ (隔离执行)              │
│                 └─────────────┘                        │
└─────────────────────────────────────────────────────────┘
```

### 关键模块映射

| Oris Crate | 功能 | 集成点 |
|------------|------|--------|
| `oris-evokernel` | 进化编排 | 核心 pipeline 驱动 |
| `oris-evolution` | Gene/Capsule/Pipeline | 类型定义和核心 trait |
| `oris-mutation-evaluator` | 两阶段评估 | Validate → Evaluate |
| `oris-genestore` | SQLite 持久化 | Gene Pool 存储 |
| `oris-sandbox` | 资源隔离 | Mutation 验证执行 |

### 安全治理架构

```
┌─────────────────────────────────────────────────────────────┐
│                    Gene Security Layer                        │
│                                                              │
│  ┌─────────────┐   ┌─────────────┐   ┌───────────────┐   │
│  │Ed25519 Sign │→  │ Source Tag  │→  │ Auto Revert   │   │
│  │(必须签名)   │   │(必须来源)   │   │(自动回滚)    │   │
│  └─────────────┘   └─────────────┘   └───────────────┘   │
│         │                  │                  │              │
│         ▼                  ▼                  ▼              │
│  ┌─────────────────────────────────────────────────────┐   │
│  │              Gene Pool (SQLite + 签名)               │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

| 安全组件 | 要求 | 实现 |
|----------|------|------|
| **签名验证** | 必须 | Ed25519 签名（oris-evolution-network 已支持） |
| **来源标签** | 必须 | 记录：error_type, user_id, session_id, timestamp |
| **自动 revert** | 必须 | 验证失败或置信度骤降 > 20% 时触发 |

---

## 6. 关键约束

### 技术约束
1. **Rust-first**：Oris 是纯 Rust，Claude Code harness 也支持 Rust 插件
2. **向后兼容**：不破坏 Claude Code 现有工作流
3. **零外部依赖**：进化能力不引入额外运行时依赖（除 SQLite）
4. **性能基线**：进化开销 < 500ms，不阻塞主任务流

### 设计约束
1. **采用当前核心代码**：不重写，利用现有 `full-evolution-experimental` feature
2. **最小侵入**：通过插件接口集成，不修改 Oris 核心
3. **可插拔**：Gene Pool 可选启用/禁用

---

## 7. 风险与依赖

### 已知风险
| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| 沙箱执行性能开销 | 任务延迟增加 | 异步执行，结果异步返回 |
| Gene Pool 膨胀 | 检索性能下降 | 置信度阈值 + LRU 淘汰 |
| 误进化（错误的修复被固化） | 知识污染 | 高阈值（≥0.72）才固化 |
| LLM 评估延迟 | 阻塞主流程 | 两阶段，第一阶段静态分析阻断 |

### 依赖项
- Oris `full-evolution-experimental` feature 稳定
- SQLite 可用（已有 `sqlite-vss` 依赖）
- Claude Code 插件接口支持 Rust FFI 或 WASM

---

## 8. 决策记录（需求挑战会确认）

### 功能决策（已确认）

| 议题 | 决策 | 说明 |
|------|------|------|
| 插件接口形式 | **IPC (Unix socket)** | 解耦、低开销（<5ms）、Oris 独立演进 |
| 进化触发时机 | **混合模式** | 高置信度(≥0.72)自动固化，低置信度用户确认 |
| Gene Pool 存储 | **`~/.claude/evolution/genes.db`** | 符合 Claude Code 配置习惯 |
| CLI 工具 | **需要** | 管理 Gene Pool（查看、清理、导出） |

### 性能决策（已确认）

| 指标 | 目标值 |
|------|--------|
| 进化总延迟 | < 500ms |
| 静态分析阶段 | < 50ms |
| Sandbox 执行 | < 300ms |
| Gene 检索 | < 20ms |
| Gene Pool 最大容量 | 10,000 个 Gene |
| 并发进化任务上限 | 3 个 |

### 安全决策（已确认）

| 议题 | 决策 | 说明 |
|------|------|------|
| Gene 签名验证 | **必须** | Ed25519 签名，oris-evolution-network 已支持 |
| 来源标签 | **必须** | error_type, user_id, session_id, timestamp |
| 自动 revert | **必须** | 验证失败或置信度骤降 > 20% 时触发 |

### 待确认项 → 已关闭

所有待确认项已通过需求挑战会确认为上述决策。

---

## 9. 参与角色

| 角色 | 输入缺口 | 职责 |
|------|----------|------|
| `tech-lead` | 整体架构决策 | 方案评审、技术选型拍板 |
| `architect` | 集成接口设计 | 插件接口、API 边界 |
| `backend-engineer` | Oris 核心代码 | 集成层实现 |
| `qa-engineer` | 验证方案 | 集成测试、E2E 场景 |
| `devops-engineer` | 发布配置 | 构建产物打包 |

---

## 10. 需求挑战会候选分组

### 核心讨论组（必须参加）
- tech-lead + architect + backend-engineer

### 扩展讨论组（按需）
- qa-engineer（测试策略）
- devops-engineer（构建/发布）

### 关键议题
1. **插件接口形式**：WASM vs FFI vs IPC — 各有什么限制？
2. **Signal 检测点**：在 harness 哪个位置注入检测逻辑？
3. **Sandbox 隔离级别**：进程级 vs OS 级 — 性能 vs 安全性权衡？
4. **Gene Pool 一致性**：单文件 SQLite 是否足够？

---

## 11. 下一步

1. 召开需求挑战会，确认上述待确认项
2. 确定插件接口形式后，启动 `/team-plan`
3. 输出 `delivery-plan.md` 和 `arch-design.md`
