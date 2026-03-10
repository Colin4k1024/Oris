# 状态图与执行引擎：StateGraph、CompiledGraph、Checkpoint

> Oris 的执行核心——如何构建可持久化、可回放的状态机。

## 0. 为什么需要状态图？

在 Oris 中，Agent 的执行不是简单的线性流程，而是**状态机**：

```
┌─────────────────────────────────────────────────────────────┐
│                     Agent 执行流                              │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│   ┌─────┐    ┌─────┐    ┌─────┐    ┌─────┐              │
│   │Start│───▶│Step1│───▶│Step2│───▶│ End │              │
│   └─────┘    └─────┘    └─────┘    └─────┘              │
│                           │                                │
│                           ▼                                │
│                      ┌─────┐                               │
│                      │Step3│                               │
│                      └─────┘                               │
│                           │                                │
│                     ┌─────┴─────┐                          │
│                     │           │                          │
│                     ▼           ▼                          │
│                 ┌─────┐     ┌─────┐                       │
│                 │Step4│     │Step5│                       │
│                 └─────┘     └─────┘                       │
│                                                             │
│   每一步都可能：                                             │
│   - 修改状态                                                 │
│   - 调用工具                                                 │
│   - 等待外部输入                                             │
│   - 分支跳转                                                 │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

这就是 **StateGraph** 要解决的问题。

## 1. StateGraph 核心概念

### 1.1 什么是 StateGraph？

**StateGraph = 可构建的状态机**

```rust
use oris_runtime::graph::{StateGraph, Checkpointer};

let mut graph = StateGraph::new(checkpointer);

// 添加节点
graph.add_node("analyze", |state| {
    let input = state.get("input").unwrap();
    // 分析输入
    state["analysis"] = analyze(input);
    Ok(())
})?;

graph.add_node("execute", |state| {
    let analysis = state.get("analysis").unwrap();
    // 执行操作
    let result = do_something(analysis);
    state["result"] = result;
    Ok(())
})?;

graph.add_node("respond", |state| {
    let result = state.get("result").unwrap();
    // 返回结果
    Ok(result.clone())
})?;

// 添加边
graph.add_edge("analyze", "execute")?;
graph.add_edge("execute", "respond")?;

// 设置入口
graph.set_entry("analyze")?;
```

### 1.2 节点类型

```rust
// 1. 普通函数节点
graph.add_node("process", |state| {
    // 处理逻辑
    Ok(())
})?;

// 2. 条件分支节点
graph.add_conditional_edge("decide", |state| -> &'static str {
    if state.get::<bool>("approved").unwrap_or(false) {
        "approve_path"
    } else {
        "reject_path"
    }
})?;

// 3. 工具节点
graph.add_tool_node("call_api", tool_api)?;

// 4. LLM 节点
graph.add_llm_node("generate", llm, prompt_template)?;
```

### 1.3 边的类型

```rust
// 1. 简单边
graph.add_edge("step1", "step2")?;

// 2. 条件边
graph.add_conditional_edge("step1", |state| -> &'static str {
    match state.get::<i32>("status_code") {
        200 => "success",
        _ => "error",
    }
})?;

// 3. 并行边（分支后合并）
graph.add_parallel_edges(
    ["branch_a", "branch_b", "branch_c"],
    "merge_point"
)?;
```

## 2. CompiledGraph：可执行图

### 2.1 编译过程

```rust
// StateGraph → CompiledGraph
let compiled = graph.compile()?;

// CompiledGraph 是可执行的
pub struct CompiledGraph {
    nodes: HashMap<String, Box<dyn Node>>,
    edges: Vec<Edge>,
    entry: String,
    checkpointer: Box<dyn Checkpointer>,
}
```

### 2.2 执行方式

```rust
// 方式 1：完整执行
let result = compiled.invoke(initial_state).await?;

// 方式 2：流式执行（逐步返回）
let mut stream = compiled.stream(initial_state).await?;
while let Some(partial) = stream.next().await {
    println!("Progress: {:?}", partial);
}

// 方式 3：单步执行（用于调试）
let mut executor = compiled.step_once(initial_state).await?;
let (state, done) = executor.execute().await?;
```

### 2.3 中断与恢复

```rust
// 单步执行时可以中断
let mut executor = compiled.step_once(state).await?;

loop {
    match executor.execute().await? {
        (state, true) => break, // 完成
        (state, false) => {
            // 可以暂停
            if should_pause(&state) {
                let checkpoint = executor.checkpoint()?;
                save_checkpoint(checkpoint);
                break;
            }
        }
    }
}
```

## 3. Checkpoint 机制

### 3.1 Checkpoint 是什么？

**Checkpoint = 状态快照**

```rust
#[derive(Serialize, Deserialize)]
pub struct Checkpoint {
    pub job_id: String,
    pub step_id: String,
    pub state: Value,          // 状态数据
    pub cursor: u64,            // 执行位置
    pub node_states: HashMap<String, NodeState>,
    pub metadata: HashMap<String, Value>,
}
```

### 3.2 Checkpointer Trait

```rust
use oris_runtime::graph::persistence::Checkpointer;

pub trait Checkpointer: Send + Sync {
    // 保存检查点
    fn save(&self, checkpoint: &Checkpoint) -> Result<(), Error>;
    
    // 加载检查点
    fn load(&self, job_id: &str) -> Result<Option<Checkpoint>, Error>;
    
    // 删除检查点
    fn delete(&self, job_id: &str) -> Result<(), Error>;
    
    // 列出检查点
    fn list(&self, filter: CheckpointFilter) -> Result<Vec<String>, Error>;
}
```

### 3.3 内置实现

#### InMemoryCheckpointer

```rust
// 内存检查点（测试用）
let checkpointer = InMemoryCheckpointer::new();
```

#### SqliteCheckpointer

```rust
// SQLite 检查点（生产用）
let checkpointer = SqliteCheckpointer::new(
    "path/to/checkpoints.db"
)?;
```

#### PostgresCheckpointer

```rust
// PostgreSQL 检查点（分布式用）
let checkpointer = PostgresCheckpointer::new(
    "postgres://user:pass@host/db"
)?;
```

### 3.4 自定义 Checkpointer

```rust
struct S3Checkpointer {
    client: S3Client,
    bucket: String,
}

impl Checkpointer for S3Checkpointer {
    fn save(&self, checkpoint: &Checkpoint) -> Result<(), Error> {
        let key = format!("checkpoints/{}.json", checkpoint.job_id);
        let data = serde_json::to_vec(checkpoint)?;
        self.client.put_object(&self.bucket, &key, data)?;
        Ok(())
    }
    
    fn load(&self, job_id: &str) -> Result<Option<Checkpoint>, Error> {
        let key = format!("checkpoints/{}.json", job_id);
        match self.client.get_object(&self.bucket, &key) {
            Ok(data) => Ok(Some(serde_json::from_slice(&data)?)),
            Err(e) if e.is_not_found() => Ok(None),
            Err(e) => Err(e),
        }
    }
}
```

## 4. 状态管理

### 4.1 状态结构

```rust
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Default)]
struct AgentState {
    // 输入
    input: Option<String>,
    
    // 中间结果
    analysis: Option<Analysis>,
    plan: Option<Plan>,
    
    // 输出
    result: Option<String>,
    
    // 元数据
    step_count: u32,
    errors: Vec<String>,
}
```

### 4.2 状态访问

```rust
graph.add_node("process", |state| {
    // 读取
    let input = state.get::<String>("input")?;
    
    // 写入
    state.set("processed", true);
    
    // 修改
    let count = state.get::<u32>("count").unwrap_or(0);
    state.set("count", count + 1);
    
    Ok(())
})?;
```

### 4.3 状态验证

```rust
graph.add_node("validate", |state| -> Result<(), Box<dyn Error>> {
    let required = ["input", "user_id"];
    
    for key in required {
        if !state.contains(key) {
            return Err(format!("Missing required field: {}", key).into());
        }
    }
    
    // 类型验证
    let user_id = state.get::<String>("user_id")?;
    if user_id.is_empty() {
        return Err("user_id cannot be empty".into());
    }
    
    Ok(())
})?;
```

## 5. 中断机制

### 5.1 可中断节点

```rust
use oris_runtime::graph::interrupt::Interrupt;

graph.add_interruptible_node("wait_approval", |state, interrupt: Interrupt| {
    // 检查是否被中断
    match interrupt {
        Interrupt::None => {
            // 正常执行
        }
        Interrupt::Signal(payload) => {
            // 处理信号
            state.set("approval", payload);
        }
        Interrupt::Cancel => {
            return Err("Task cancelled".into());
        }
    }
    Ok(())
})?;
```

### 5.2 信号处理

```rust
// 发送信号
compiled.send_signal(job_id, "wait_approval", Signal {
    payload: serde_json::json!({"approved": true}),
}).await?;

// 查询状态
let status = compiled.get_status(job_id).await?;
```

## 6. 流式输出

### 6.1 生成器模式

```rust
// 流式执行
let mut stream = compiled.stream(initial_state).await?;

while let Some(event) = stream.next().await {
    match event {
        StreamEvent::NodeStarted { node, .. } => {
            println!("Starting: {}", node);
        }
        StreamEvent::NodeCompleted { node, output, .. } => {
            println!("Completed: {} -> {:?}", node, output);
        }
        StreamEvent::Error { node, error } => {
            eprintln!("Error in {}: {}", node, error);
        }
    }
}
```

### 6.2 LLM 流式输出

```rust
// LLM 流式响应
graph.add_llm_stream_node("chat", llm, |state| {
    format!("Respond as {}", state.get::<String>("persona").unwrap_or("assistant"))
})?;

let mut stream = compiled.stream(state).await?;

while let Some(chunk) = stream.llm_chunk("chat").await {
    print!("{}", chunk);
}
```

## 7. 完整示例

### 7.1 构建一个简单的 Agent 图

```rust
use oris_runtime::graph::{StateGraph, SqliteCheckpointer};
use oris_runtime::tools::Tool;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. 创建检查点存储
    let checkpointer = SqliteCheckpointer::new("agent.db")?;
    
    // 2. 构建状态图
    let mut graph = StateGraph::new(checkpointer);
    
    // 入口：分析输入
    graph.add_node("analyze", |state| {
        let input = state.get::<String>("input").unwrap_or_default();
        state["analysis"] = format!("Analyzed: {}", input);
        Ok(())
    })?;
    
    // 决策节点
    graph.add_conditional_edge("analyze", |state| -> &'static str {
        let analysis = state.get::<String>("analysis").unwrap_or_default();
        if analysis.contains("error") {
            "error_path"
        } else {
            "success_path"
        }
    })?;
    
    // 错误处理
    graph.add_node("handle_error", |state| {
        state["result"] = "Error handled".to_string();
        Ok(())
    })?;
    
    // 成功路径
    graph.add_node("process", |state| {
        let analysis = state.get::<String>("analysis").unwrap_or_default();
        state["result"] = format!("Processed: {}", analysis);
        Ok(())
    })?;
    
    graph.add_edge("handle_error", "end")?;
    graph.add_edge("process", "end")?;
    graph.set_entry("analyze")?;
    
    // 3. 编译
    let compiled = graph.compile()?;
    
    // 4. 执行
    let result = compiled.invoke(serde_json::json!({
        "input": "Hello, world!"
    })).await?;
    
    println!("Result: {:?}", result.get("result"));
    
    Ok(())
}
```

### 7.2 持久化执行

```rust
// 带 Checkpoint 的持久执行
async fn run_with_checkpoint(
    job_id: &str,
    checkpointer: impl Checkpointer,
    graph: StateGraph,
) -> Result<Value, Error> {
    // 1. 尝试恢复
    let state = match checkpointer.load(job_id)? {
        Some(checkpoint) => checkpoint.state,
        None => initial_state(),
    };
    
    // 2. 编译图
    let compiled = graph.compile()?;
    
    // 3. 执行
    let result = compiled.invoke(state).await?;
    
    // 4. 保存检查点（每步后）
    // ...（在节点执行后自动保存）
    
    Ok(result)
}
```

## 8. 与其他系统的对比

| 特性 | Oris StateGraph | LangGraph | Temporal |
|------|-----------------|-----------|----------|
| 图构建 | Rust Builder | Python/Python | Go/Java |
| Checkpoint | 多存储后端 | 内存 | PostgreSQL |
| 流式 | 原生支持 | 有限 | 有限 |
| 中断 | Signal 机制 | 外部控制 | Signal |
| 条件边 | 闭包 | 函数 | Switch |

## 9. 小结

StateGraph 是 Oris 的执行核心：

1. **StateGraph** — 可构建的状态机
2. **CompiledGraph** — 编译后的可执行图
3. **Checkpointer** — 状态持久化（内存/SQLite/PostgreSQL）
4. **节点类型** — 函数/条件/工具/LLM
5. **边类型** — 简单/条件/并行
6. **中断机制** — Signal/Cancel
7. **流式输出** — 实时进度和 LLM 流

有了这套机制，Oris 可以构建**复杂、可持久、可回放**的工作流。

---

*下篇预告：Agent 架构——Deep Agent 实现与工具集成*
