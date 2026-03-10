# 实战：构建自进化开发助手

> 把所有东西组合起来——构建一个真正能"自己修 Bug"的开发助手。

## 0. 最终目标

```
┌─────────────────────────────────────────────────────────────┐
│                   自进化开发助手                              │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  1. 监听代码变化                                           │
│  2. 运行测试                                               │
│  3. 检测失败 → 生成信号                                     │
│  4. Intake 分类 + 优先级                                    │
│  5. Evolution 选择基因 + 变异                              │
│  6. 沙箱验证                                               │
│  7. 成功 → 固化到基因池                                     │
│  8. 失败 → 人工介入                                         │
│                                                             │
│  结果：Bug 自动修复！                                       │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

## 1. 系统架构

```
┌─────────────────────────────────────────────────────────────┐
│                  自进化开发助手架构                           │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌─────────────────────────────────────────────────────┐   │
│  │                    Event Source                      │   │
│  │    (Git hooks / CI webhook / File watcher)          │   │
│  └────────────────────────┬────────────────────────────┘   │
│                           ▼                                  │
│  ┌─────────────────────────────────────────────────────┐   │
│  │                   Evokernel                          │   │
│  │     (Signal Extraction)                            │   │
│  └────────────────────────┬────────────────────────────┘   │
│                           ▼                                  │
│  ┌─────────────────────────────────────────────────────┐   │
│  │                    Intake                            │   │
│  │    (Dedup / Classify / Prioritize / Route)         │   │
│  └────────────────────────┬────────────────────────────┘   │
│                           ▼                                  │
│  ┌─────────────────────────────────────────────────────┐   │
│  │               Evolution Pipeline                     │   │
│  │  Select → Mutate → Execute → Validate → Evaluate   │   │
│  └────────────────────────┬────────────────────────────┘   │
│                           ▼                                  │
│  ┌─────────────────────────────────────────────────────┐   │
│  │                   Gene Pool                          │   │
│  │         (Storage + Retrieval)                       │   │
│  └─────────────────────────────────────────────────────┘   │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

## 2. 初始化

### 2.1 创建项目

```bash
cargo new evo-dev-assistant
cd evo-dev-assistant
```

### 2.2 添加依赖

```toml
# Cargo.toml
[dependencies]
oris-runtime = { version = "0.22", features = [
    "full-evolution-experimental",
    "sqlite-persistence",
] }
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
```

### 2.3 配置文件

```yaml
# config.yaml
evolution:
  enabled: true
  max_candidates: 5
  max_iterations: 3
  timeout_seconds: 300

gene_pool:
  storage: sqlite
  path: ./gene_pool.db

intake:
  deduplicate: true
  similarity_threshold: 0.8

sandbox:
  timeout_seconds: 60
  memory_limit_mb: 512

monitoring:
  webhook_url: "http://localhost:9000/webhook"
```

## 3. 核心实现

### 3.1 主程序

```rust
use oris_runtime::{
    evokernel::Evokernel,
    intake::IssueIntake,
    evolution::{EvolutionPipeline, GenePool},
    graph::{StateGraph, SqliteCheckpointer},
};

struct EvoDevAssistant {
    evokernel: Evokernel,
    intake: IssueIntake,
    pipeline: EvolutionPipeline,
    gene_pool: GenePool,
}

impl EvoDevAssistant {
    async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        // 1. 初始化基因池
        let gene_pool = GenePool::new("sqlite:gene_pool.db")?;
        
        // 2. 初始化 Evokernel
        let evokernel = Evokernel::builder()
            .compilers(vec![Box::new(RustCompiler::new())])
            .testers(vec![Box::new(RustTester::new())])
            .runtime(RuntimeMonitor::new().capture_panics(true).build()?)
            .build()?;
        
        // 3. 初始化 Intake
        let intake = IssueIntake::builder()
            .config(IntakeConfig::default()
                .deduplicate(true)
                .auto_classify(true)
                .auto_prioritize(true)
            )
            .build()?;
        
        // 4. 初始化进化管道
        let pipeline = EvolutionPipeline::builder()
            .name("dev_assistant")
            .max_candidates(5)
            .max_iterations(3)
            .build()?;
        
        Ok(Self {
            evokernel,
            intake,
            pipeline,
            gene_pool,
        })
    }
}
```

### 3.2 运行测试并捕获信号

```rust
impl EvoDevAssistant {
    async fn run_tests_and_capture_signals(&self) 
        -> Result<Vec<Signal>, Box<dyn std::error::Error>> {
        
        let mut signals = Vec::new();
        
        // 1. 运行编译
        let build_output = Command::new("cargo")
            .args(&["build", "--message-format=json"])
            .output()?;
        
        if !build_output.status.success() {
            let compiler_signals = self.evokernel.extract_from_compiler(
                &String::from_utf8_lossy(&build_output.stderr)
            );
            signals.extend(compiler_signals);
        }
        
        // 2. 运行测试
        let test_output = Command::new("cargo")
            .args(&["test", "--no-fail-fast", "--message-format=json"])
            .output()?;
        
        if !test_output.status.success() {
            let test_signals = self.evokernel.extract_from_tests(
                &String::from_utf8_lossy(&test_output.stdout)
            );
            signals.extend(test_signals);
        }
        
        Ok(signals)
    }
}
```

### 3.3 处理问题

```rust
impl EvoDevAssistant {
    async fn process_signals(&self, signals: Vec<Signal>) 
        -> Result<(), Box<dyn std::error::Error>> {
        
        if signals.is_empty() {
            println!("No issues detected!");
            return Ok(());
        }
        
        println!("Detected {} signals", signals.len());
        
        // 1. Intake 处理
        let issue = self.intake.ingest(signals).await?;
        
        println!("Created issue: {} (Priority: {:?})", 
            issue.title, issue.priority);
        
        // 2. 根据优先级决策
        match issue.priority {
            Priority::Critical | Priority::High => {
                println!("Attempting auto-fix...");
                self.auto_fix(issue).await?;
            }
            _ => {
                println!("Added to backlog");
                self.intake.add_to_backlog(&issue).await?;
            }
        }
        
        Ok(())
    }
}
```

### 3.4 自动修复

```rust
impl EvoDevAssistant {
    async fn auto_fix(&self, issue: Issue) 
        -> Result<(), Box<dyn std::error::Error>> {
        
        println!("Starting evolution for issue: {}", issue.id);
        
        // 运行进化
        let result = self.pipeline.evolve(issue.clone(), &self.gene_pool).await?;
        
        match result {
            EvolutionResult::Success(capsule) => {
                println!("Evolution successful!");
                println!("Applied gene: {}", capsule.gene().id());
                
                // 验证修复
                println!("Verifying fix...");
                let verified = self.verify_fix(&issue).await?;
                
                if verified {
                    println!("Fix verified! Solidifying to gene pool...");
                    
                    // 固化到基因池
                    self.gene_pool.solidify(capsule).await?;
                    
                    // 创建 PR
                    self.create_pr(&issue, &capsule).await?;
                } else {
                    println!("Fix verification failed");
                }
            }
            EvolutionResult::Failed(reasons) => {
                println!("Evolution failed: {:?}", reasons);
                
                // 人工介入
                self.request_human_review(&issue, reasons).await?;
            }
        }
        
        Ok(())
    }
}
```

### 3.5 验证修复

```rust
impl EvoDevAssistant {
    async fn verify_fix(&self, issue: &Issue) 
        -> Result<bool, Box<dyn std::error::Error>> {
        
        // 重新运行编译和测试
        let build = Command::new("cargo").arg("build").output()?;
        if !build.status.success() {
            return Ok(false);
        }
        
        let test = Command::new("cargo").arg("test").output()?;
        if !test.status.success() {
            return Ok(false);
        }
        
        Ok(true)
    }
}
```

### 3.6 创建 PR

```rust
impl EvoDevAssistant {
    async fn create_pr(&self, issue: &Issue, capsule: &Capsule) 
        -> Result<(), Box<dyn std::error::Error>> {
        
        // 创建分支
        let branch = format!("fix/issue-{}", issue.id);
        Command::new("git")
            .args(&["checkout", "-b", &branch])
            .output()?;
        
        // 应用修复
        // ... (应用代码变更)
        
        // 提交
        Command::new("git")
            .args(&["add", "-A"])
            .output()?;
        
        Command::new("git")
            .args(&["commit", "-m", &format!(
                "fix: {}\n\nAuto-generated by Oris EvoDevAssistant\nIssue: {}", 
                issue.title, issue.id
            )])
            .output()?;
        
        // 推送
        Command::new("git")
            .args(&["push", "origin", &branch])
            .output()?;
        
        println!("Created branch: {}", branch);
        
        Ok(())
    }
}
```

## 4. 运行模式

### 4.1 文件监控模式

```rust
use notify::{Watcher, RecursiveMode, recommended_watcher};

async fn watch(_modeassistant: &EvoDevAssistant) {
    let (tx, mut rx) = tokio::sync::mpsc::channel(100);
    
    // 启动文件监控
    std::thread::spawn(move || {
        let mut watcher = recommended_watcher(move |res| {
            tx.blocking_send(res).unwrap();
        }).unwrap();
        
        watcher.watch("./src", RecursiveMode::Recursive).unwrap();
        
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    });
    
    // 监听变化
    loop {
        if let Some(event) = rx.recv().await {
            println!("File changed: {:?}", event);
            
            // 延迟一下，等文件稳定
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            
            // 运行测试并捕获信号
            let signals = assistant.run_tests_and_capture_signals().await?;
            
            // 处理
            assistant.process_signals(signals).await?;
        }
    }
}
```

### 4.2 CI 模式

```rust
async fn ci_mode(assistant: &EvoDevAssistant) {
    // CI 触发
    let signals = assistant.run_tests_and_capture_signals().await?;
    assistant.process_signals(signals).await?;
}
```

### 4.3 Webhook 模式

```rust
use actix_web::{web, App, HttpServer, Responder};

async fn webhook(
    assistant: web::Data<EvoDevAssistant>,
    payload: web::Json<WebhookPayload>,
) -> impl Responder {
    match payload.event {
        "push" => {
            let signals = assistant.run_tests_and_capture_signals().await?;
            assistant.process_signals(signals).await?;
            "OK"
        }
        _ => "Ignored",
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let assistant = EvoDevAssistant::new().await?;
    
    HttpServer::new(move || {
        App::new()
            .app_data(assistant.clone())
            .route("/webhook", web::post().to(webhook))
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}
```

## 5. Git Hooks 集成

### 5.1 pre-commit Hook

```bash
#!/bin/bash
# .git/hooks/pre-commit

echo "Running pre-commit checks..."

# 运行测试
cargo test

if [ $? -ne 0 ]; then
    echo "Tests failed! Running EvoDevAssistant..."
    
    # 捕获信号
    signals=$(cargo build 2>&1 | grep "error" | ...)
    
    # 处理
    # ...
    
    exit 1
fi
```

### 5.2 post-merge Hook

```bash
#!/bin/bash
# .git/hooks/post-merge

echo "Running after merge..."

# 检查新引入的问题
cargo test
```

## 6. 监控与告警

### 6.1 Dashboard

```rust
use oris_runtime::monitoring::Dashboard;

let dashboard = Dashboard::new()
    .port(3000)
    .build()?;

dashboard.start().await?;
```

### 6.2 指标

```
┌─────────────────────────────────────────────────────────────┐
│               EvoDevAssistant Dashboard                      │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  Statistics (24h):                                          │
│  ─────────────────                                          │
│  Signals detected: 156                                      │
│  Issues created: 89                                         │
│  Auto-fixed: 67 (75%)                                       │
│  Human review: 22 (25%)                                     │
│                                                             │
│  Evolution:                                                 │
│  ──────────                                                │
│  Success rate: 75%                                          │
│  Avg fix time: 45s                                          │
│  Genes in pool: 234                                         │
│                                                             │
│  Top Issues:                                               │
│  ───────────                                               │
│  1. Compilation error (42)                                 │
│  2. Test failure (38)                                      │
│  3. Runtime panic (15)                                      │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

## 7. 完整启动脚本

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting EvoDevAssistant...");
    
    // 1. 初始化
    let assistant = EvoDevAssistant::new().await?;
    println!("Initialized");
    
    // 2. 加载基因
    let genes_loaded = assistant.gene_pool.count().await?;
    println!("Loaded {} genes", genes_loaded);
    
    // 3. 运行模式
    let mode = std::env::var("MODE").unwrap_or_else(|_| "watch".to_string());
    
    match mode.as_str() {
        "watch" => {
            println!("Running in watch mode...");
            watch_mode(&assistant).await;
        }
        "ci" => {
            println!("Running in CI mode...");
            ci_mode(&assistant).await;
        }
        "webhook" => {
            println!("Running webhook server...");
            // 启动 webhook 服务器
        }
        _ => {
            println!("Unknown mode: {}", mode);
        }
    }
    
    Ok(())
}
```

## 8. 运行示例

### 8.1 本地运行

```bash
# 启动
MODE=watch cargo run

# 输出
Starting EvoDevAssistant...
Initialized
Loaded 234 genes
Running in watch mode...

# 修改代码触发测试失败
File changed: src/main.rs
Detected 2 signals
Created issue: cannot find value `user` (Priority: High)
Attempting auto-fix...
Evolution successful!
Applied gene: fix_null_pointer
Verifying fix...
Fix verified! Solidifying to gene pool...
Created branch: fix/issue-xxx
```

### 8.2 GitHub Actions

```yaml
# .github/workflows/evo-assistant.yml
name: EvoDevAssistant

on:
  push:
    branches: [main]

jobs:
  evolve:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      
      - name: Setup Rust
        uses: actions-rs/toolchain@v1
        
      - name: Run EvoDevAssistant
        env:
          MODE: ci
          OPENAI_API_KEY: ${{ secrets.OPENAI_API_KEY }}
        run: cargo run
```

## 9. 小结

自进化开发助手 = 完整的闭环系统：

1. **Evokernel** — 捕获问题信号（编译、测试、运行时）
2. **Intake** — 去重、分类、优先级
3. **Evolution Pipeline** — 选择、变异、执行、验证
4. **Gene Pool** — 存储 + 复用
5. **验证** — 确保修复有效
6. **PR 创建** — 自动提交

**这就是未来：Bug 自动修复，代码自己变好。**

---

*10 篇 Oris 技术博客全部完成！*

## 博客索引

| # | 标题 | 核心内容 |
|---|------|----------|
| 1 | Oris 介绍 | 自进化执行运行时理念 |
| 2 | 状态图与执行引擎 | StateGraph、CompiledGraph、Checkpoint |
| 3 | Agent 架构 | Deep Agent、工具集成 |
| 4 | 工具系统 | Tool trait、内置工具 |
| 5 | 记忆系统 | Working/Conversational/Long-term Memory |
| 6 | RAG 与向量存储 | 知识检索 |
| 7 | 自进化系统 | Gene、Capsule、Pipeline |
| 8 | 诊断信号提取 | Evokernel |
| 9 | 问题接入 | Intake、优先级 |
| 10 | 实战 | 完整自进化开发助手 |
