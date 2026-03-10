# 工具系统：Tool Trait 与内置工具

> 工具是 Agent 的"手"——让 Agent 能操作世界。

## 0. 为什么需要工具？

没有工具的 Agent 只能"想"，不能"做"：

```
无工具 Agent：
思考 → 思考 → 思考 → 输出文字
（只能动嘴，不能动手）

有工具 Agent：
思考 → 调用搜索 API → 读取文件 → 执行命令 → 操作数据库
（能动能做）
```

**工具 = Agent 与外部世界交互的桥梁。**

## 1. Tool Trait

### 1.1 核心定义

```rust
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub trait Tool: Send + Sync {
    /// 工具名称
    fn name(&self) -> &str;
    
    /// 工具描述（让 LLM 知道什么时候用它）
    fn description(&self) -> &str;
    
    /// 输入 JSON Schema
    fn input_schema(&self) -> &Value;
    
    /// 执行工具
    async fn execute(&self, input: ToolInput) -> Result<ToolOutput, ToolError>;
}
```

### 1.2 输入输出类型

```rust
// 工具输入
pub struct ToolInput {
    pub raw: Value,              // 原始 JSON
    pub parsed: Option<Value>,   // 解析后的结构
}

impl ToolInput {
    pub fn parse<T: DeserializeOwned>(&self) -> Result<T, ToolError> {
        serde_json::from_value(self.raw.clone())
            .map_err(|e| ToolError::ParseError(e.to_string()))
    }
}

// 工具输出
pub enum ToolOutput {
    Text(String),
    Json(Value),
    Binary(Vec<u8>),
    Error(String),
}

impl ToolOutput {
    pub fn text(&self) -> Option<&str> {
        match self { ToolOutput::Text(s) => Some(s), _ => None }
    }
    
    pub fn json(&self) -> Option<&Value> {
        match self { ToolOutput::Json(v) => Some(v), _ => None }
    }
}
```

### 1.3 错误类型

```rust
#[derive(Debug)]
pub enum ToolError {
    /// 输入解析失败
    ParseError(String),
    
    /// 验证失败
    ValidationError(String),
    
    /// 执行失败
    ExecutionError(String),
    
    /// 超时
    Timeout(Duration),
    
    /// 权限不足
    PermissionDenied,
    
    /// 工具不存在
    NotFound,
}
```

## 2. 内置工具概览

| 工具 | 功能 |
|------|------|
| **Command** | 执行 shell 命令 |
| **Search** | 搜索引擎查询 |
| **SQL** | 数据库查询 |
| **Scraper** | 网页抓取 |
| **BrowserUse** | 浏览器自动化 |
| **ReadFile** | 读取文件 |
| **WriteFile** | 写入文件 |
| **HTTP** | HTTP 请求 |

## 3. Command 工具

### 3.1 基本用法

```rust
use oris_runtime::tools::Command;

let cmd = Command::new("ls -la /tmp")
    .description("List files in tmp directory")
    .timeout(Duration::from_secs(30))
    .build()?;

let result = cmd.execute(ToolInput::empty()).await?;

println!("{}", result.text().unwrap());
```

### 3.2 带参数

```rust
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct CmdInput {
    command: String,
    cwd: Option<String>,
    env: Option<HashMap<String, String>>,
    timeout: Option<u64>,
}

let cmd = Command::new("{{command}}")
    .description("Execute a shell command")
    .input_schema(r#{
        "type": "object",
        "properties": {
            "command": {"type": "string"},
            "cwd": {"type": "string"},
            "timeout": {"type": "number"}
        },
        "required": ["command"]
    }#)
    .build()?;
```

### 3.3 安全性

```rust
let cmd = Command::new("{{command}}")
    // 允许的命令白名单
    .allowed_commands(vec!["git", "cargo", "ls", "cat"])
    // 或禁止危险命令
    .blocked_commands(vec!["rm -rf", "dd", "mkfs"])
    // 限制工作目录
    .allowed_dirs(vec!["/home/user/projects"])
    // 超时限制
    .timeout(Duration::from_secs(30))
    .build()?;
```

## 4. Search 工具

### 4.1 搜索引擎

```rust
use oris_runtime::tools::search::{Search, Engine};

let search = Search::new()
    .engine(Engine::Brave)  // Brave, Google, DuckDuckGo
    .max_results(10)
    .include_snippets(true)
    .build()?;

let result = search.execute(ToolInput::json(serde_json::json!({
    "query": "Rust async programming best practices"
}))).await?;

for item in result.json().unwrap().as_array().unwrap() {
    println!("Title: {}", item["title"]);
    println!("URL: {}", item["url"]);
    println!();
}
```

### 4.2 搜索结果结构

```rust
#[derive(Deserialize)]
struct SearchResult {
    results: Vec<SearchItem>,
    total: usize,
}

#[derive(Deserialize)]
struct SearchItem {
    title: String,
    url: String,
    snippet: String,
    score: f32,
}
```

## 5. SQL 工具

### 5.1 基本查询

```rust
use oris_runtime::tools::sql::SQL;

let sql = SQL::new(pool)
    .description("Query the database")
    .build()?;

let result = sql.execute(ToolInput::json(serde_json::json!({
    "query": "SELECT * FROM users WHERE created_at > '2024-01-01'"
}))).await?;

println!("{}", result.text().unwrap());
```

### 5.2 写操作

```rust
let sql = SQL::new(pool)
    // 允许写操作
    .allow_writes(true)
    // 或限制为只读
    .read_only(true)
    // 表白名单
    .allowed_tables(vec!["users", "orders", "products"])
    .build()?;
```

### 5.3 事务支持

```rust
let result = sql.execute(ToolInput::json(serde_json::json!({
    "query": "BEGIN; INSERT INTO orders ...; COMMIT;",
    "transaction": true
}))).await?;
```

## 6. Scraper 工具

### 6.1 基本抓取

```rust
use oris_runtime::tools::scraper::Scraper;

let scraper = Scraper::new()
    .user_agent("Oris/1.0 (Research Bot)")
    .timeout(Duration::from_secs(10))
    .build()?;

let result = scraper.execute(ToolInput::json(serde_json::json!({
    "url": "https://example.com",
    "selector": "article h1, article p"  // CSS 选择器
}))).await?;

let data = result.json().unwrap();
println!("Title: {}", data["title"]);
println!("Content: {}", data["content"]);
```

### 6.2 JavaScript 渲染

```rust
let scraper = Scraper::new()
    .enable_javascript(true)  // 渲染 JS
    .wait_for_selector(".content", Duration::from_secs(5))
    .build()?;
```

## 7. BrowserUse 工具

### 7.1 浏览器自动化

```rust
use oris_runtime::tools::browser_use::BrowserUse;

let browser = BrowserUse::new()
    .headless(true)
    .viewport(1920, 1080)
    .build()?;

let result = browser.execute(ToolInput::json(serde_json::json!({
    "actions": [
        {"type": "goto", "url": "https://example.com"},
        {"type": "click", "selector": "#login-btn"},
        {"type": "type", "selector": "#email", "text": "user@example.com"},
        {"type": "click", "selector": "#submit"},
        {"type": "wait", "duration": 2000},
        {"type": "screenshot"},
        {"type": "extract", "selector": ".result"}
    ]
}))).await?;
```

### 7.2 动作类型

| 动作 | 说明 |
|------|------|
| `goto` | 导航到 URL |
| `click` | 点击元素 |
| `type` | 输入文本 |
| `hover` | 悬停 |
| `scroll` | 滚动 |
| `screenshot` | 截图 |
| `extract` | 提取内容 |
| `wait` | 等待 |

## 8. 自定义工具

### 8.1 实现完整工具

```rust
use oris_runtime::tools::{Tool, ToolInput, ToolOutput, ToolError};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

struct WeatherTool {
    client: reqwest::Client,
    api_key: String,
}

impl WeatherTool {
    fn new(api_key: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
        }
    }
}

impl Tool for WeatherTool {
    fn name(&self) -> &str => "weather"
    
    fn description(&self) -> &str => 
        "Get current weather for a city. Input: { city: string }"
    
    fn input_schema(&self) -> &Value => serde_json::json!({
        "type": "object",
        "properties": {
            "city": {
                "type": "string",
                "description": "City name"
            }
        },
        "required": ["city"]
    })
    
    async fn execute(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let input: WeatherInput = input.parse()?;
        
        let url = format!(
            "https://api.weather.com/v3/wx/conditions?city={}&key={}",
            input.city, self.api_key
        );
        
        let response = self.client.get(&url)
            .send()
            .await
            .map_err(|e| ToolError::ExecutionError(e.to_string()))?;
        
        let data: WeatherData = response.json()
            .await
            .map_err(|e| ToolError::ExecutionError(e.to_string()))?;
        
        Ok(ToolOutput::json(serde_json::json!({
            "city": input.city,
            "temperature": data.temperature,
            "condition": data.condition,
            "humidity": data.humidity,
            "wind_speed": data.wind_speed
        })))
    }
}
```

### 8.2 工具工厂

```rust
fn create_tools() -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(Command::new("...").build().unwrap()),
        Box::new(Search::new().build().unwrap()),
        Box::new(WeatherTool::new(std::env::var("WEATHER_API_KEY").unwrap())),
    ]
}

let agent = DeepAgent::builder()
    .tools(create_tools())
    .build()?;
```

## 9. 工具注册与管理

### 9.1 注册中心

```rust
use oris_runtime::tools::registry::ToolRegistry;

let registry = ToolRegistry::new();

// 注册工具
registry.register(WeatherTool::new(api_key))?;
registry.register(Search::new().build()?);
registry.register(Command::new("...").build()?);

// 获取工具
let tool = registry.get("weather")?;

// 列出所有工具
for name in registry.list() {
    println!("{}", name);
}
```

### 9.2 工具发现

```rust
// Agent 自动发现可用工具
let agent = DeepAgent::builder()
    .auto_discover_tools(true)
    .tool_dirs(vec!["./tools", "/usr/lib/oris/tools"])
    .build()?;
```

## 10. 工具调用的执行流程

```
Agent 调用工具：
┌─────────────────────────────────────────────────────────────┐
│                                                             │
│  1. Agent 决定调用工具 "weather"                             │
│  2. 构建输入 JSON                                           │
│  3. 调用 Tool::execute(input)                              │
│                                                             │
│  ┌─────────────────────────────────────────────────────┐   │
│  │ Tool 实现                                            │   │
│  │  - 验证输入                                          │   │
│  │  - 调用外部服务                                      │   │
│  │  - 处理响应                                          │   │
│  │  - 返回结果                                          │   │
│  └─────────────────────────────────────────────────────┘   │
│                                                             │
│  4. 工具返回 ToolOutput                                     │
│  5. Agent 处理输出                                          │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

## 11. 小结

Oris 的工具系统：

1. **Tool Trait** — 统一接口
2. **内置工具** — Command、Search、SQL、Scraper、BrowserUse
3. **输入验证** — JSON Schema
4. **错误处理** — 统一的 ToolError
5. **自定义工具** — 简单实现
6. **注册管理** — 工具发现与检索

**工具是 Agent 的手——让 Agent 能操作真实世界。**

---

*下篇预告：记忆系统——Memory 实现与上下文管理*
