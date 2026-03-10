# 问题接入与优先级 (Intake)：从信号到 Issue

> Intake = 问题的"过滤器"和"路由器"。

## 0. Intake 的价值

```
问题来了：
- 100 个信号同时到达
- 有些是重复的
- 有些优先级高，有些优先级低
- 有些可以自动修复，有些需要人工

Intake = 智能分类 + 优先级排序 + 自动路由
```

## 1. Issue 结构

### 1.1 Issue 定义

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    /// Issue ID
    pub id: String,
    
    /// 标题
    pub title: String,
    
    /// 描述
    pub description: String,
    
    /// 来源信号
    pub signals: Vec<Signal>,
    
    /// 分类
    pub category: IssueCategory,
    
    /// 优先级
    pub priority: Priority,
    
    /// 状态
    pub status: IssueStatus,
    
    /// 创建时间
    pub created_at: i64,
    
    /// 元数据
    pub metadata: HashMap<String, Value>,
}
```

### 1.2 Issue 分类

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum IssueCategory {
    /// 编译错误
    Compilation,
    /// 测试失败
    TestFailure,
    /// 运行时错误
    RuntimeError,
    /// 性能问题
    Performance,
    /// 内存问题
    Memory,
    /// 安全漏洞
    Security,
    /// 逻辑错误
    Logic,
    /// 代码异味
    CodeSmell,
    /// 待办事项
    Todo,
}
```

### 1.3 优先级

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Critical = 5,  // 必须立即修复
    High = 4,      // 应该尽快修复
    Medium = 3,    // 正常优先级
    Low = 2,       // 可以稍后修复
    Trivial = 1,   // 无关紧要
}
```

## 2. Intake 流程

### 2.1 完整流程

```
Signal → 去重 → 分类 → 优先级 → 路由 → Issue
            ↓
      相似合并
```

### 2.2 Intake 配置

```rust
use oris_runtime::intake::{Intake, IntakeConfig};

let intake = Intake::builder()
    .config(IntakeConfig::default()
        .deduplicate(true)
        .similarity_threshold(0.8)
        .auto_classify(true)
        .auto_prioritize(true)
    )
    .classifier(RuleBasedClassifier::new()
        .add_rule(|s| s.message.contains("E0425"), IssueCategory::Compilation)
        .add_rule(|s| s.signal_type == SignalType::TestFailure, IssueCategory::TestFailure)
        .add_rule(|s| s.signal_type == SignalType::RuntimeError, IssueCategory::RuntimeError)
    )
    .prioritizer(MLPrioritizer::new()
        .model("issue-priority-v1")
    )
    .router(DefaultRouter::new()
        .route(Priority::Critical, RouteTarget::Immediate)
        .route(Priority::High, RouteTarget::Evolution)
        .route(Priority::Medium, RouteTarget::Backlog)
    )
    .build()?;
```

## 3. 去重与合并

### 3.1 相似度计算

```rust
use oris_runtime::intake::deduplication::Deduplicator;

let deduplicator = Deduplicator::new()
    .similarity_threshold(0.8)
    .use_embedding(true)
    .build()?;

fn compute_similarity(s1: &Signal, s2: &Signal) -> f32 {
    // 1. 文本相似度
    let text_sim = jaccard_similarity(&s1.message, &s2.message);
    
    // 2. 位置相似度
    let loc_sim = if let (Some(l1), Some(l2)) = (&s1.location, &s2.location) {
        if l1.file == l2.file {
            1.0 - (l1.line as f32 - l2.line as f32).abs() / 100.0
        } else {
            0.0
        }
    } else {
        0.5
    };
    
    // 加权组合
    text_sim * 0.7 + loc_sim * 0.3
}
```

### 3.2 合并策略

```rust
let issue = deduplicator.merge_signals(&signals)?;

println!("Merged {} signals into 1 issue", signals.len());
println!("Issue: {}", issue.title);
println!("Occurrences: {}", issue.metadata["occurrence_count"]);
```

## 3. 自动分类

### 3.1 规则分类

```rust
let classifier = RuleBasedClassifier::new()
    .add_rule(
        // 编译错误
        |s: &Signal| s.signal_type == SignalType::CompilationError,
        IssueCategory::Compilation,
    )
    .add_rule(
        // 测试失败
        |s| s.signal_type == SignalType::TestFailure,
        IssueCategory::TestFailure,
    )
    .add_rule(
        // panic
        |s| s.message.contains("panicked at"),
        IssueCategory::RuntimeError,
    )
    .add_rule(
        // 性能
        |s| s.signal_type == SignalType::PerformanceRegression,
        IssueCategory::Performance,
    )
    .add_rule(
        // 内存
        |s| s.signal_type == SignalType::MemoryLeak,
        IssueCategory::Memory,
    )
    .build()?;
```

### 3.2 ML 分类

```rust
let classifier = MLClassifier::new()
    .model("classifier-v2")
    .train_data("./training_data.json")
    .build()?;
```

## 4. 优先级计算

### 4.1 基于规则

```rust
let prioritizer = RuleBasedPrioritizer::new()
    .rule(
        |issue: &Issue| issue.category == IssueCategory::Security,
        Priority::Critical,
    )
    .rule(
        |issue| issue.category == IssueCategory::Compilation && issue.signals.len() > 5,
        Priority::High,
    )
    .rule(
        |issue| issue.category == IssueCategory::CodeSmell,
        Priority::Low,
    )
    .build()?;
```

### 4.2 基于 ML

```rust
let prioritizer = MLPrioritizer::new()
    .model("priority-v1")
    .features(vec![
        Feature::Category,
        Feature::SignalCount,
        Feature::Severity,
        Feature::FilePath,
        Feature::RecentIssues,
    ])
    .build()?;

let priority = prioritizer.predict(&issue).await?;
```

### 4.3 因素

```rust
fn calculate_priority(issue: &Issue) -> Priority {
    let mut score = 0.0;
    
    // 1. 信号严重程度
    for signal in &issue.signals {
        score += match signal.severity {
            Severity::Critical => 5.0,
            Severity::High => 3.0,
            Severity::Medium => 2.0,
            Severity::Low => 1.0,
        };
    }
    
    // 2. 信号数量
    score += issue.signals.len() as f32 * 0.5;
    
    // 3. 分类权重
    score += match issue.category {
        IssueCategory::Security => 10.0,
        IssueCategory::Compilation => 5.0,
        IssueCategory::TestFailure => 3.0,
        IssueCategory::RuntimeError => 8.0,
        IssueCategory::Performance => 2.0,
        _ => 1.0,
    };
    
    // 4. 用户指定
    if issue.metadata.contains_key("user_priority") {
        score += issue.metadata["user_priority"].as_f64().unwrap_or(0.0);
    }
    
    // 转换为优先级
    match score {
        s if s >= 15.0 => Priority::Critical,
        s if s >= 10.0 => Priority::High,
        s if s >= 5.0 => Priority::Medium,
        s if s >= 2.0 => Priority::Low,
        _ => Priority::Trivial,
    }
}
```

## 5. 路由策略

### 5.1 路由目标

```rust
#[derive(Debug, Clone)]
pub enum RouteTarget {
    /// 立即处理
    Immediate,
    /// 加入进化队列
    Evolution,
    /// 加入待办列表
    Backlog,
    /// 忽略
    Ignore,
    /// 人工审核
    ManualReview,
}
```

### 5.2 路由规则

```rust
let router = RuleRouter::new()
    // 关键错误立即处理
    .route(
        |i: &Issue| i.priority == Priority::Critical,
        RouteTarget::Immediate,
    )
    // 安全漏洞人工审核
    .route(
        |i| i.category == IssueCategory::Security,
        RouteTarget::ManualReview,
    )
    // 一般错误进入进化
    .route(
        |i| matches!(i.priority, Priority::High | Priority::Medium),
        RouteTarget::Evolution,
    )
    // 低优先级进入待办
    .route(
        |i| matches!(i.priority, Priority::Low | Priority::Trivial),
        RouteTarget::Backlog,
    )
    .build()?;
```

## 6. Issue 管理

### 6.1 创建 Issue

```rust
use oris_runtime::intake::IssueIntake;

let intake = IssueIntake::new().build()?;

let issue = intake.create_issue(signals).await?;

println!("Created issue: {}", issue.id);
```

### 6.2 更新 Issue

```rust
// 更新状态
intake.update_status(&issue_id, IssueStatus::InProgress).await?;

// 添加评论
intake.add_comment(&issue_id, "Starting investigation...").await?;

// 关联 PR
intake.link_pr(&issue_id, "https://github.com/...").await?;
```

### 6.3 查询 Issue

```rust
// 按状态查询
let open = intake.query()
    .status(IssueStatus::Open)
    .execute()
    .await?;

// 按优先级查询
let critical = intake.query()
    .priority(Priority::Critical)
    .execute()
    .await?;

// 按分类查询
let bugs = intake.query()
    .category(IssueCategory::RuntimeError)
    .execute()
    .await?;
```

## 7. 与 Evolution 集成

### 7.1 自动触发进化

```rust
let intake = Intake::builder()
    .auto_evolve(true)
    .evolve_on_signals(true)
    .build()?;

let issue = intake.ingest(signals).await?;

// 自动触发进化
if should_evolve(&issue) {
    let pipeline = EvolutionPipeline::new();
    pipeline.evolve(issue).await?;
}
```

### 7.2 Issue 反馈循环

```rust
// 进化完成后
pipeline.on_complete(|result| {
    match result {
        EvolutionResult::Success(capsule) => {
            // 更新 Issue 状态
            intake.resolve(&issue_id, format!(
                "Fixed by gene: {}",
                capsule.gene().id()
            )).await?;
            
            // 标记为已解决
            intake.update_status(&issue_id, IssueStatus::Resolved)?;
        }
        EvolutionResult::Failed(reasons) => {
            // 标记为无法自动修复
            intake.update_status(&issue_id, IssueStatus::NeedsReview)?;
            
            // 添加说明
            intake.add_comment(&issue_id, 
                &format!("Auto-fix failed: {:?}", reasons)
            ).await?;
        }
    }
});
```

## 8. 完整示例

### 8.1 一站式接入

```rust
use oris_runtime::intake::{Intake, IntakeConfig};
use oris_runtime::evolution::EvolutionPipeline;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. 创建 Intake
    let intake = Intake::builder()
        .config(IntakeConfig::defaultuplicate(true)
()
            .ded            .auto_classify(true)
            .auto_prioritize(true)
        )
        .build()?;
    
    // 2. 接入信号
    let signals = vec![
        Signal::new(SignalType::CompilationError, "cannot find value"),
        Signal::new(SignalType::CompilationError, "cannot find value"),
    ];
    
    let issue = intake.ingest(signals).await?;
    
    println!("Issue: {} (Priority: {:?})", issue.title, issue.priority);
    
    // 3. 根据优先级路由
    match issue.priority {
        Priority::Critical | Priority::High => {
            // 触发进化
            let pipeline = EvolutionPipeline::new();
            pipeline.evolve(issue.clone()).await?;
        }
        _ => {
            // 加入待办
            intake.add_to_backlog(&issue).await?;
        }
    }
    
    Ok(())
}
```

## 9. Dashboard

### 9.1 Issue 统计

```
┌─────────────────────────────────────────────────────────────┐
│                    Issue Dashboard                           │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  Open Issues: 23                                           │
│  ├── Critical: 2   ██                                       │
│  ├── High: 5       █████                                    │
│  ├── Medium: 10    ██████████                               │
│  └── Low: 6        ██████                                   │
│                                                             │
│  By Category:                                              │
│  ├── Compilation: 5                                        │
│  ├── TestFailure: 8                                        │
│  ├── RuntimeError: 3                                       │
│  ├── Performance: 4                                        │
│  └── Other: 3                                              │
│                                                             │
│  Recent:                                                   │
│  ├── [Critical] Cannot find value `user` in src/main.rs    │
│  ├── [High] Test `test_login` failed                        │
│  └── [Medium] Memory usage increased by 20%               │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

## 10. 小结

Intake = 问题的智能处理器：

1. **Signal → Issue** — 信号标准化
2. **去重** — 合并相似问题
3. **分类** — 规则 + ML
4. **优先级** — 多因素计算
5. **路由** — Immediate/Evolution/Backlog/Manual
6. **管理** — CRUD + 查询
7. **Evolution 集成** — 自动触发修复
8. **Dashboard** — 可视化

**Intake 让问题处理变得有序——从混乱的信号到结构化的 Issue。**

---

*下篇预告：实战——构建自进化开发助手*
