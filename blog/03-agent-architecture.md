# Agent 架构：Deep Agent 实现与工具集成

> Oris 的 Agent 不是简单的 prompt 包装——它是可组合、可持久、可进化的执行单元。

## 0. Oris Agent 的定位

```
传统 Agent：
┌────────────────────────────────────────────┐
│  Prompt → LLM → Parse → Tool → Result    │
│                                           │
│  问题：                                    │
│  - 不可持久化                              │
│  - 不可回放                                │
│  - 状态丢失                                │
│  - 无法自进化                              │
└────────────────────────────────────────────┘

Oris Agent：
┌────────────────────────────────────────────┐
│  StateGraph + Tools + Memory + Evolution   │
│                                           │
│  特性：                                    │
│  - 持久执行 ✓                              │
│  - 确定性回放 ✓                            │
│  - 工具集成 ✓                              │
│  - 自进化 ✓                                │
└────────────────────────────────────────────┘
```

## 1. Agent Trait

### 1.1 核心定义

```rust
use oris_runtime::agent::{Agent, Tool, Memory};
use oris_runtime::graph::StateGraph;

pub trait Agent: Send + Sync {
    /// Agent 类型
    fn name(&self) -> &str;
    
    /// 计划执行
    async fn plan(&self, goal: &str, context: &Value) -> Result<Plan, AgentError>;
    
    /// 获取可用工具
    fn get_tools(&self) -> Vec<Box<dyn Tool>>;
    
    /// 获取记忆
    fn get_memory(&self) -> Option<&dyn Memory>;
    
    /// 构建执行图
    fn build_graph(&self, plan: &Plan) -> Result<StateGraph, AgentError>;
}
```

### 1.2 Plan 结构

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    /// 步骤列表
    pub steps: Vec<Step>,
    
    /// 依赖关系
    pub dependencies: HashMap<String, Vec<String>>,
    
    /// 估计成本
    pub estimated_cost: Cost,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    pub id: String,
    pub description: String,
    pub tool: Option<String>,
    pub prompt: Option<String>,
    pub condition: Option<Condition>,
    pub retry: RetryPolicy,
}
```

## 2. Deep Agent

### 2.1 什么是 Deep Agent？

**Deep Agent = 深度思考 + 工具使用 + 长期记忆**

```rust
use oris_runtime::agent::deep::DeepAgent;

let agent = DeepAgent::builder()
    .name("researcher")
    .llm(OpenAI::gpt4())
    .tools(vec![
        search_tool,
        scrape_tool,
        sql_tool,
    ])
    .memory(vector_store.clone())
    .max_iterations(10)
    .temperature(0.7)
    .build()?;
```

### 2.2 思考循环

```rust
impl Agent for DeepAgent {
    async fn plan(&self, goal: &str, context: &Value) -> Result<Plan, AgentError> {
        // 1. 分析目标
        let analysis = self.analyze_goal(goal, context).await?;
        
        // 2. 分解任务
        let steps = self.decompose(&analysis).await?;
        
        // 3. 规划执行
        let plan = self.create_plan(steps).await?;
        
        Ok(plan)
    }
}

impl DeepAgent {
    async fn analyze_goal(&self, goal: &str, context: &Value) -> Result<Analysis, AgentError> {
        // 使用 LLM 分析目标
        let prompt = format!(
            r#"Analyze this goal and extract key information.
            
Goal: {}

Context: {}

Extract:
- Main objective
- Required tools/information
- Constraints
- Success criteria
"#,
            goal, context
        );
        
        let response = self.llm.invoke(&prompt).await?;
        let analysis = parse_analysis(&response)?;
        
        Ok(analysis)
    }
}
```

### 2.3 工具选择

```rust
impl DeepAgent {
    async fn select_tool(&self, task: &Task, available_tools: &[Box<dyn Tool>]) 
        -> Result<Option<Box<dyn Tool>>, AgentError> {
        
        // 构建工具选择 prompt
        let prompt = format!(
            r#"Given the task: {}
            
Available tools:
{}
            
Select the most appropriate tool. If no tool is needed, say "none".
"#,
            task.description,
            available_tools.iter()
                .map(|t| format!("- {}: {}", t.name(), t.description()))
                .collect::<Vec<_>>()
                .join("\n")
        );
        
        let response = self.llm.invoke(&prompt).await?;
        
        // 解析选择
        let selected = parse_tool_selection(&response)?;
        
        Ok(available_tools.into_iter()
            .find(|t| t.name() == selected)
            .map(|t| t))
    }
}
```

## 3. 工具集成

### 3.1 Tool Trait

```rust
use oris_runtime::tools::{Tool, ToolInput, ToolOutput};

pub trait Tool: Send + Sync {
    /// 工具名称
    fn name(&self) -> &str;
    
    /// 工具描述
    fn description(&self) -> &str;
    
    /// 输入模式（JSON Schema）
    fn input_schema(&self) -> &Value;
    
    /// 执行工具
    async fn execute(&self, input: ToolInput) -> Result<ToolOutput, ToolError>;
    
    /// 可选：验证输入
    fn validate(&self, input: &Value) -> Result<(), ToolError> {
        Ok(()) // 默认不验证
    }
}
```

### 3.2 内置工具

Oris 提供了丰富的内置工具：

```rust
use oris_runtime::tools::{
    Command, Search, SQL, Scraper, BrowserUse,
};

// 命令执行
let cmd = Command::new("ls -la")
    .description("List files")
    .timeout(Duration::from_secs(30))
    .build()?;

// 搜索
let search = Search::new()
    .engine(Engine::Brave)
    .max_results(10)
    .build()?;

// SQL 查询
let sql = SQL::new(pool)
    .description("Query database")
    .build()?;

// 网页抓取
let scraper = Scraper::new()
    .user_agent("Oris/1.0")
    .build()?;

// 浏览器自动化
let browser = BrowserUse::new()
    .headless(true)
    .build()?;
```

### 3.3 自定义工具

```rust
use oris_runtime::tools::{Tool, ToolInput, ToolOutput};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct WeatherInput {
    city: String,
}

#[derive(Serialize, Deserialize)]
struct WeatherOutput {
    temperature: f32,
    condition: String,
    humidity: f32,
}

struct WeatherTool {
    api_key: String,
}

impl Tool for WeatherTool {
    fn name(&self) -> &str => "weather"
    
    fn description(&self) -> &str => "Get weather information for a city"
    
    fn input_schema(&self) -> &Value => serde_json::json!({
        "type": "object",
        "properties": {
            "city": {"type": "string"}
        },
        "required": ["city"]
    })
    
    async fn execute(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let input: WeatherInput = input.parse()?;
        
        // 调用天气 API
        let url = format!(
            "https://api.weather.com/v3/wx/conditions?city={}&key={}",
            input.city, self.api_key
        );
        let response = reqwest::get(&url).await?;
        let data: WeatherData = response.json().await?;
        
        Ok(ToolOutput::json(WeatherOutput {
            temperature: data.temp,
            condition: data.condition,
            humidity: data.humidity,
        }))
    }
}
```

### 3.4 工具注册

```rust
let agent = DeepAgent::builder()
    .tools(vec![
        Box::new(WeatherTool::new(api_key)),
        Box::new(Command::new("...")),
        Box::new(Search::new()),
    ])
    .build()?;
```

## 4. Agent 中间件

### 4.1 中间件是什么？

**中间件 = Agent 执行链上的拦截器**

```rust
use oris_runtime::agent::middleware::{
    Middleware, MiddlewareChain,
    LoggingMiddleware, RetryMiddleware, MetricsMiddleware,
};

let chain = MiddlewareChain::new()
    .add(LoggingMiddleware::new())
    .add(RetryMiddleware::new().max_retries(3))
    .add(MetricsMiddleware::new());

let agent = DeepAgent::builder()
    .middleware(chain)
    .build()?;
```

### 4.2 自定义中间件

```rust
struct RateLimitMiddleware {
    limiter: RateLimiter,
}

impl Middleware for RateLimitMiddleware {
    async fn process(
        &self,
        ctx: &mut AgentContext,
        next: NextMiddleware,
    ) -> Result<Value, AgentError> {
        // 检查速率限制
        self.limiter.acquire().await?;
        
        // 继续执行
        let result = next.run(ctx).await?;
        
        Ok(result)
    }
}

struct LoggingMiddleware;

impl Middleware for LoggingMiddleware {
    async fn process(
        &self,
        ctx: &AgentContext,
        next: NextMiddleware,
    ) -> Result<Value, AgentError> {
        let start = Instant::now();
        
        println!("[Agent] Starting: {}", ctx.goal);
        
        let result = next.run(ctx).await;
        
        let duration = start.elapsed();
        
        match &result {
            Ok(v) => println!("[Agent] Completed in {:?}", duration),
            Err(e) => println!("[Agent] Failed: {}", e),
        }
        
        result
    }
}
```

## 5. 多 Agent 模式

### 5.1 Agent 协作

```rust
use oris_runtime::agent::multi::MultiAgent;

let team = MultiAgent::builder()
    .agent("researcher", researcher_agent)
    .agent("writer", writer_agent)
    .agent("reviewer", reviewer_agent)
    .coordination(Coordination::Sequential)  // 或 Parallel
    .build()?;

// 协作执行
let result = team.execute("Write a research report on AI").await?;
```

### 5.2 消息传递

```rust
// Agent 之间传递消息
team.send_message(
    from: "researcher",
    to: "writer",
    message: Message {
        content: "Here is the research data...",
        attachments: vec![...],
    }
).await?;

// 接收消息
let msg = writer.receive_message().await?;
```

### 5.3 共享状态

```rust
let team = MultiAgent::builder()
    .shared_state(|state| {
        state.insert("research_data".to_string(), Value::Null);
    })
    .build()?;
```

## 6. 执行与持久化

### 6.1 构建可执行的 Agent

```rust
let agent = DeepAgent::builder()
    .name("assistant")
    .llm(OpenAI::gpt4())
    .tools(tools)
    .build()?;

// 计划
let plan = agent.plan("帮我写一个排序算法", &Value::Null).await?;

// 构建执行图
let graph = agent.build_graph(&plan)?;

// 编译
let compiled = graph.compile()?;

// 执行
let result = compiled.invoke(initial_state).await?;
```

### 6.2 持久化执行

```rust
let checkpointer = SqliteCheckpointer::new("agent.db")?;

let agent = DeepAgent::builder()
    .checkpointer(checkpointer.clone())
    .build()?;

// 持久执行
let executor = agent.execute_persistent("task_123", goal).await?;

loop {
    match executor.step().await? {
        StepResult::Completed(result) => break,
        StepResult::Checkpoint(checkpoint) => {
            // 可以暂停
            checkpointer.save(&checkpoint)?;
        }
        StepResult::WaitingForInput(prompt) => {
            let input = get_user_input(&prompt).await?;
            executor.respond(input).await?;
        }
    }
}
```

## 7. 完整示例

### 7.1 创建一个研究 Agent

```rust
use oris_runtime::agent::deep::DeepAgent;
use oris_runtime::llm::openai::OpenAI;
use oris_runtime::tools::{Search, SQL, Command};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. 创建 LLM
    let llm = OpenAI::default();
    
    // 2. 创建工具
    let search = Search::new()
        .max_results(5)
        .build()?;
    
    let sql = SQL::new(pool)
        .description("Query internal database")
        .build()?;
    
    // 3. 构建 Agent
    let agent = DeepAgent::builder()
        .name("researcher")
        .llm(llm)
        .tools(vec![
            Box::new(search),
            Box::new(sql),
        ])
        .max_iterations(10)
        .temperature(0.7)
        .build()?;
    
    // 4. 执行
    let result = agent.run("Research the latest AI developments").await?;
    
    println!("Result: {}", result);
    
    Ok(())
}
```

### 7.2 添加记忆

```rust
use oris_runtime::memory::{Memory, VectorStore};

let vector_store = Qdrant::new("localhost:6334", "agent_memory")?;

let agent = DeepAgent::builder()
    .memory(vector_store)
    .memory_enabled(true)
    .build()?;

// Agent 自动记住重要信息
// 下次遇到类似问题会自动检索
```

## 8. 与 LangChain/AutoGen 对比

| 特性 | Oris Agent | LangChain Agent | AutoGen |
|------|-----------|-----------------|---------|
| 持久化 | ✅ 天然支持 | ❌ | ❌ |
| 回放 | ✅ | ❌ | ❌ |
| 自进化 | ✅ | ❌ | ❌ |
| Rust 实现 | ✅ | ❌ (Python) | ❌ (Python) |
| 性能 | 高 | 中 | 中 |
| 类型安全 | ✅ | ❌ | ❌ |

## 9. 小结

Oris 的 Agent 系统：

1. **Agent Trait** — 统一的 Agent 接口
2. **Deep Agent** — 深度思考 + 工具选择
3. **工具系统** — 丰富的内置工具 + 自定义
4. **中间件** — 可插拔的拦截链
5. **多 Agent** — 协作与消息传递
6. **持久化** — 天生的持久执行能力

Oris Agent 不只是"prompt + LLM"，而是**完整的可编程执行单元**。

---

*下篇预告：工具系统——Tool trait 与内置工具*
