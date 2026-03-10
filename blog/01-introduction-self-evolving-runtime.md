# Oris：自进化执行运行时——让软件自己变好

> 传统 Runtime 执行代码，Oris 进化代码。

## 0. 一个大胆的想法

想象一下：

```
你写了一个有 Bug 的程序
    ↓
程序崩溃了，Oris 自动分析错误
    ↓
Oris 生成了多个修复方案
    ↓
Oris 在沙箱中测试每个方案
    ↓
Oris 选择了最好的一个
    ↓
你的程序自动修复了 Bug
```

这听起来像科幻？但这正是 **Oris** 正在做的事情。

## 1. 传统 Runtime 的局限

### 1.1 现在的 Runtime 做什么？

```
┌─────────────────────────────────────────────────────────────┐
│                  传统 Runtime                              │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  代码 → 编译 → 执行 → 结果                                   │
│              ↑                                              │
│              │                                              │
│         只做「一次性」的事情                                 │
│         执行完了就完了                                       │
│         如果有问题 → 等待人类修复                            │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

主流 Runtime 的能力：
- **执行**：运行代码
- **持久化**：保存状态
- **恢复**：崩溃后继续
- **编排**：协调多步骤

但它们都有一个共同点：**不会让代码变得更好**。

### 1.2 问题在哪里？

```
人类开发者的工作流：

1. 写代码
2. 运行测试
3. 发现 Bug
4. 分析问题
5. 手动修复
6. 重新测试
7. 提交代码

问题：步骤 3-6 都是人工做的！
```

**自动化只到了「执行」这一步，后面的改进全是人工的。**

## 2. Oris 的回答：自进化

### 2.1 什么是自进化？

**自进化 = 自动化的问题解决循环**

```
┌─────────────────────────────────────────────────────────────┐
│                    Oris 自进化循环                          │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│   ┌─────────────┐                                          │
│   │   Detect    │ ← 检测问题（编译错误、panic、测试失败）    │
│   └──────┬──────┘                                          │
│          ↓                                                 │
│   ┌─────────────┐                                          │
│   │   Select    │ ← 选择最佳方案（Gene Selection）          │
│   └──────┬──────┘                                          │
│          ↓                                                 │
│   ┌─────────────┐                                          │
│   │  Mutate     │ ← 生成解决方案（代码变异）                │
│   └──────┬──────┘                                          │
│          ↓                                                 │
│   ┌─────────────┐                                          │
│   │  Execute    │ ← 沙箱执行（安全测试）                    │
│   └──────┬──────┘                                          │
│          ↓                                                 │
│   ┌─────────────┐                                          │
│   │ Validate    │ ← 验证正确性                              │
│   └──────┬──────┘                                          │
│          ↓                                                 │
│   ┌─────────────┐                                          │
│   │  Evaluate   │ ← 评估效果（改进 vs 回归）                │
│   └──────┬──────┘                                          │
│          ↓                                                 │
│   ┌─────────────┐                                          │
│   │  Solidify   │ ← 固化到基因池（Gene Pool）              │
│   └──────┬──────┘                                          │
│          ↓                                                 │
│   ┌─────────────┐                                          │
│   │   Reuse     │ ← 复用（带置信度追踪）                    │
│   └─────────────┘                                          │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### 2.2 核心概念

| 概念 | 含义 |
|------|------|
| **Gene（基因）** | 一个可复用的解决方案片段 |
| **Capsule（胶囊）** | 封装的进化事件，包含变异+验证结果 |
| **Signal（信号）** | 从运行时提取的问题指标 |
| **Selector（选择器）** | 从多个候选中选择最佳 Gene |
| **Pipeline（管道）** | 完整的进化流程 |
| **Confidence（置信度）** | Gene 的可信度，会自动衰减/提升 |

## 3. Oris vs 传统 Runtime

### 3.1 功能对比

| 能力 | Oris | Temporal | LangGraph |
|------|------|----------|-----------|
| 持久执行 | ✅ | ✅ | ✅ |
| 崩溃恢复 | ✅ | ✅ | ✅ |
| 确定性回放 | ✅ | ✅ | ⚠️ 有限 |
| **自进化** | ✅ 独有 | ❌ | ❌ |
| **置信度生命周期** | ✅ 独有 | ❌ | ❌ |
| Human-in-the-Loop | ✅ | ✅ | ⚠️ |

### 3.2 适用场景

```
适合 Oris：
├── 自动修复 Bug
├── 自改进 Agent
├── 进化式代码生成
├── 置信度感知缓存
├── 自主开发循环
└── 带进化的持久 Agent

适合 Temporal：
├── 业务流程编排
├── 微服务编排
├── 长时间运行的任务

适合 LangGraph：
├── 简单的 Agent 图
├── 快速原型
└── 概念验证
```

### 3.3 为什么需要自进化？

**成本问题**：

```
传统开发：
- 70% 时间在调试
- 20% 时间在修复
- 10% 时间在写新代码

Oris 愿景：
- 把 90% 的调试+修复自动化
- 人类只做创造性工作
```

**规模问题**：

```
1 个开发者：手动修复没问题
100 个微服务：手动修复忙不过来
10000 个 AI Agent：根本修不过来

需要自进化系统来处理规模的增长
```

## 4. 架构概览

### 4.1 整体架构

```
┌─────────────────────────────────────────────────────────────┐
│                       Oris 架构                              │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌─────────────────────────────────────────────────────┐   │
│  │                    User Request                      │   │
│  └────────────────────────┬────────────────────────────┘   │
│                           ▼                                  │
│  ┌─────────────────────────────────────────────────────┐   │
│  │                  Signal Extraction                   │   │
│  │                 (Evokernel - 检测问题)               │   │
│  └────────────────────────┬────────────────────────────┘   │
│                           ▼                                  │
│  ┌─────────────────────────────────────────────────────┐   │
│  │              Evolution Pipeline (8 stages)           │   │
│  │  Select → Mutate → Execute → Validate → Evaluate    │   │
│  └────────────────────────┬────────────────────────────┘   │
│                           ▼                                  │
│  ┌─────────────────────────────────────────────────────┐   │
│  │                    Gene Pool                         │   │
│  │              (可复用的解决方案库)                      │   │
│  └────────────────────────┬────────────────────────────┘   │
│                           ▼                                  │
│  ┌─────────────────────────────────────────────────────┐   │
│  │                 Execution Engine                     │   │
│  │         StateGraph + Checkpoint + Replay             │   │
│  └────────────────────────┬────────────────────────────┘   │
│                           ▼                                  │
│  ┌─────────────────────────────────────────────────────┐   │
│  │                   Storage                            │   │
│  │         SQLite / PostgreSQL / Vector DB              │   │
│  └─────────────────────────────────────────────────────┘   │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### 4.2 核心模块

| 模块 | 职责 |
|------|------|
| **graph/** | 状态图、执行引擎、持久化、检查点、流式输出 |
| **agent/** | Agent 循环、工具、中间件、多 Agent 模式 |
| **kernel/** | Kernel API - 事件优先执行、动作、回放 |
| **tools/** | 工具 trait 和内置工具 |
| **llm/** | LLM 实现（OpenAI、Claude、Ollama 等） |
| **memory/** | 记忆实现（简单、会话、长期） |
| **vectorstore/** | 向量存储（pgvector、Qdrant、SQLite） |
| **evolution/** | 自进化：Gene、Capsule、选择器、管道 |
| **evokernel/** | 从运行时诊断提取信号 |
| **intake/** | Issue 接入、去重、优先级 |

## 5. 快速开始

### 5.1 安装

```bash
cargo add oris-runtime
export OPENAI_API_KEY="your-key"
```

### 5.2 最小示例

```rust
use oris_runtime::{language_models::llm::LLM, llm::openai::OpenAI};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let llm = OpenAI::default();
    let response = llm.invoke("What is Rust?").await?;
    println!("{}", response);
    Ok(())
}
```

### 5.3 启用进化功能

```bash
cargo add oris-runtime --features full-evolution-experimental
```

### 5.4 运行进化示例

```bash
cargo run -p evo_oris_repo
```

## 6. 核心特性详解

### 6.1 持久执行

```rust
use oris_runtime::graph::{StateGraph, Checkpointer};

let mut graph = StateGraph::new(checkpointer);

graph.add_node("step1", |state| {
    // 执行步骤 1
    state["result"] = "done".to_string();
    Ok(())
});

graph.add_node("step2", |state| {
    // 执行步骤 2
    println!("Result: {}", state["result"]);
    Ok(())
});

// 编译并执行
let compiled = graph.compile()?;
compiled.invoke(initial_state).await?;
```

### 6.2 自进化

```rust
use oris_runtime::evolution::{Gene, Capsule, Selector};

// 定义一个问题
let issue = Issue {
    title: "Null pointer in handler".to_string(),
    context: "When input is None, handler panics".to_string(),
};

// 选择最佳 Gene
let selector = Selector::new();
let best_gene = selector.select(&issue, &gene_pool).await?;

// 生成变异
let capsule = best_gene.mutate(&issue).await?;

// 沙箱执行
let result = capsule.execute_in_sandbox().await?;

if result.passed_validation() {
    // 验证通过，固化到基因池
    gene_pool.solidify(capsule)?;
}
```

### 6.3 置信度追踪

```rust
use oris_runtime::evolution::Confidence;

// Gene 有置信度，会自动调整
let gene = gene_pool.get("fix_null_pointer")?;

match gene.confidence {
    Confidence::High => // 直接使用
    Confidence::Medium => // 建议人工审核
    Confidence::Low => // 需要重新验证
}

// 置信度会根据使用结果自动调整
gene.boost();   // 成功后提升
gene.decay();   // 失败后降低
```

## 7. EvoMap 对齐

Oris 对齐 [EvoMap](https://evomap.ai) 协议：

| EvoMap 概念 | Oris 实现 |
|-------------|-----------|
| Worker Pool | `EvolutionPipeline` (8 阶段) |
| Task Queue | Signal 提取 → Gene 选择 |
| Bounty System | Issue intake + 优先级评分 |
| A2A Protocol | `oris-evolution-network` crate |

## 8. 与 Aetheris 的关系

### 8.1 定位差异

| 维度 | Aetheris | Oris |
|------|----------|------|
| **语言** | Go | Rust |
| **核心能力** | 可靠执行 | 自进化 |
| **目标** | 让 Agent 可靠运行 | 让代码自己变好 |
| **进化** | 无 | 核心特性 |

### 8.2 可以结合吗？

```
可以！Oris 可以构建在 Aetheris 之上：

Aetheris（执行层）
    ↑
Oris（进化层）
    ↑
业务 Agent
```

或者：

```
Oris（执行层 + 进化层）
    ↑
业务 Agent（用 Aetheris 风格编写）
```

## 9. 小结

Oris 是一个**自进化的执行运行时**：

1. **不是普通的 Runtime** — 它能让代码自己变好
2. **Detect → Select → Mutate → Execute → Validate → Evaluate → Solidify → Reuse** — 完整的进化循环
3. **Gene + Capsule + Confidence** — 核心抽象
4. **Rust 实现** — 高性能、安全
5. **EvoMap 对齐** — 标准化协议

**下一代的软件不只是执行——它会进化。**

---

*下篇预告：状态图与执行引擎——StateGraph、CompiledGraph、Checkpoint*
