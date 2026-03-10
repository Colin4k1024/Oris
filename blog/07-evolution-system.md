# 自进化系统 (Evolution)：Gene、Capsule、Pipeline

> 这是 Oris 最核心的能力——让代码自己变好。

## 0. 什么是自进化？

```
传统开发：
写代码 → 测试 → 发现 Bug → 手动修复 → 提交

Oris 自进化：
检测问题 → 生成方案 → 选择最佳 → 自动修复 → 验证 → 固化 → 复用
```

**自进化 = 自动化的问题解决循环。**

## 1. 核心概念

### 1.1 Gene（基因）

**Gene = 可复用的解决方案单元**

```rust
use oris_runtime::evolution::{Gene, GeneContent};

let gene = Gene::new(
    "fix_null_pointer",
    GeneContent::Patch {
        before: "fn handler(input: Option<String>) {
    println!(\"{}\", input.unwrap());
}".to_string(),
        after: "fn handler(input: Option<String>) {
    if let Some(s) = input {
        println!(\"{}\", s);
    }
}".to_string(),
    },
);

// 添加元数据
gene.add_tag("rust".to_string());
gene.add_tag("null-safety".to_string());
gene.set_confidence(0.85);
```

### 1.2 Capsule（胶囊）

**Capsule = 封装的进化事件**

```rust
use oris_runtime::evolution::{Capsule, EvolutionEvent, ValidationResult};

let capsule = Capsule::new(
    "issue_123",      // 关联的问题
    gene.clone(),     // 使用的基因
    EvolutionEvent::Mutation {
        original: "...".to_string(),
        mutated: "...".to_string(),
    },
);

// 执行后添加结果
capsule.add_result(ValidationResult {
    passed: true,
    test_results: vec![...],
    execution_time_ms: 150,
});
```

### 1.3 Confidence（置信度）

```rust
use oris_runtime::evolution::Confidence;

// 置信度级别
match gene.confidence() {
    Confidence::High(_) => {
        // 直接应用
    }
    Confidence::Medium(_) => {
        // 建议人工审核
    }
    Confidence::Low(_) => {
        // 需要重新验证
    }
}

// 置信度变化
gene.boost(0.1);   // 成功后提升
gene.decay(0.05);  // 失败后降低
gene.decay_by_time(days(30)); // 时间衰减
```

## 2. 进化管道

### 2.1 8 阶段管道

```
┌─────────────────────────────────────────────────────────────┐
│              Evolution Pipeline (8 stages)                  │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌──────────┐                                              │
│  │  Signal   │ ← 问题检测（编译错误、panic，测试失败）       │
│  └─────┬────┘                                              │
│        ▼                                                    │
│  ┌──────────┐                                              │
│  │  Select   │ ← 从基因池选择候选基因                       │
│  └─────┬────┘                                              │
│        ▼                                                    │
│  ┌──────────┐                                              │
│  │  Mutate   │ ← 生成变体                                  │
│  └─────┬────┘                                              │
│        ▼                                                    │
│  ┌──────────┐                                              │
│  │ Execute   │ ← 沙箱执行                                  │
│  └─────┬────┘                                              │
│        ▼                                                    │
│  ┌──────────┐                                              │
│  │ Validate  │ ← 验证正确性                                 │
│  └─────┬────┘                                              │
│        ▼                                                    │
│  ┌──────────┐                                              │
│  │ Evaluate  │ ← 评估效果                                   │
│  └─────┬────┘                                              │
│        ▼                                                    │
│  ┌──────────┐                                              │
│  │ Solidify │ ← 固化到基因池                               │
│  └─────┬────┘                                              │
│        ▼                                                    │
│  ┌──────────┐                                              │
│  │  Reuse   │ ← 复用                                       │
│  └──────────┘                                              │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### 2.2 Pipeline 实现

```rust
use oris_runtime::evolution::{EvolutionPipeline, PipelineConfig};

let pipeline = EvolutionPipeline::builder()
    .name("code_fix")
    .config(PipelineConfig::default()
        .max_candidates(5)
        .max_iterations(3)
        .timeout(Duration::from_secs(300))
    )
    .selector(GeneticSelector::new()
        .population_size(20)
        .mutation_rate(0.1)
    )
    .sandbox(Sandbox::new()
        .timeout(Duration::from_secs(60))
        .memory_limit_mb(512)
    )
    .validator(TestValidator::new())
    .build()?;
```

## 3. Signal（信号）

### 3.1 什么是 Signal？

**Signal = 从运行时提取的问题指标**

```rust
use oris_runtime::evolution::signal::{Signal, SignalType};

let signal = Signal::new(
    SignalType::CompilationError,
    "error: cannot find value `foo` in this scope",
    SourceLocation {
        file: "src/main.rs",
        line: 42,
        column: 5,
    },
);
```

### 3.2 Signal 类型

| 类型 | 说明 |
|------|------|
| **CompilationError** | 编译错误 |
| **RuntimeError** | 运行时错误（panic） |
| **TestFailure** | 测试失败 |
| **PerformanceRegression** | 性能退化 |
| **MemoryLeak** | 内存泄漏 |
| **LogicError** | 逻辑错误 |

### 3.3 Signal 提取

```rust
// 从编译输出提取
let signals = Evokernel::extract_from_compiler_output(&output);

// 从测试失败提取
let signals = Evokernel::extract_from_test_failure(&failure);

// 从运行时提取
let signals = Evokernel::extract_from_panic(&panic_info);
```

## 4. Gene Selection（选择）

### 4.1 选择器接口

```rust
use oris_runtime::evolution::selector::{Selector, Candidate};

pub trait Selector: Send + Sync {
    async fn select(
        &self,
        signal: &Signal,
        gene_pool: &GenePool,
    ) -> Result<Vec<Candidate>, EvolutionError>;
}
```

### 4.2 遗传选择器

```rust
use oris_runtime::evolution::selector::GeneticSelector;

let selector = GeneticSelector::new()
    .population_size(50)
    .generations(10)
    .mutation_rate(0.15)
    .crossover_rate(0.7)
    .fitness_function(|gene, signal| {
        // 根据基因和问题匹配度计算适应度
        compute_fitness(gene, signal)
    })
    .build()?;
```

### 4.3 基于规则的選擇

```rust
use oris_runtime::evolution::selector::RuleBasedSelector;

let selector = RuleBasedSelector::new()
    .add_rule(
        // 如果是 NullPointerException，选择 null-safety 基因
        |signal| signal.error_type.contains("NullPointer"),
        |pool| pool.get_genes_by_tag("null-safety"),
    )
    .add_rule(
        // 如果是性能问题，选择 performance 基因
        |signal| signal.error_type.contains("Performance"),
        |pool| pool.get_genes_by_tag("performance"),
    )
    .build()?;
```

## 5. Mutation（变异）

### 5.1 变异策略

```rust
use oris_runtime::evolution::mutation::{Mutator, MutationStrategy};

// 代码变异
let mutator = Mutator::code_mutator()
    .strategy(MutationStrategy::Template {
        // 模板替换
        templates: vec![
            ("{{value}}.unwrap()", "if let Some(s) = {{value}} { s }"),
            ("unwrap()", "unwrap_or_default()"),
        ],
    })
    // 或 LLM 变异
    .strategy(MutationStrategy::LLM {
        llm: llm.clone(),
        examples: vec![...],
    })
    .build()?;
```

### 5.2 生成多个候选

```rust
let candidates = mutator.mutate(
    &issue,
    &selected_gene,
    5,  // 生成 5 个候选
).await?;

for candidate in candidates {
    println!("Candidate: {}", candidate.code);
}
```

## 6. Execution（执行）

### 6.1 沙箱执行

```rust
use oris_runtime::sandbox::Sandbox;

let sandbox = Sandbox::new()
    .timeout(Duration::from_secs(60))
    .memory_limit_mb(512)
    .network_access(false)  // 禁止网络
    .filesystem_access(vec!["./test_workspace"])
    .build()?;

let result = sandbox.execute(candidate.code).await?;
```

### 6.2 执行结果

```rust
#[derive(Debug)]
pub struct ExecutionResult {
    pub output: String,
    pub exit_code: i32,
    pub execution_time_ms: u64,
    pub memory_used_mb: u64,
    pub killed: bool,          // 是否被 kill（超时/内存）
    pub error: Option<String>,
}
```

## 7. Validation（验证）

### 7.1 测试验证

```rust
use oris_runtime::evolution::validator::TestValidator;

let validator = TestValidator::new()
    .run_unit_tests(true)
    .run_integration_tests(false)
    .parallel(true)
    .build()?;
```

### 7.2 静态分析

```rust
use oris_runtime::evolution::validator::StaticAnalyzer;

let validator = StaticAnalyzer::new()
    .check_rustc_warnings(true)
    .check_clippy(true)
    .check_security(true)
    .build()?;
```

### 7.3 验证结果

```rust
let result = validator.validate(&candidate).await?;

if result.passed() {
    println!("Validation passed!");
} else {
    for error in result.errors() {
        println!("Error: {}", error);
    }
}
```

## 8. Evaluation（评估）

### 8.1 评估指标

```rust
use oris_runtime::evolution::evaluator::{Evaluator, Metrics};

let evaluator = Evaluator::new()
    .add_metric(Metrics::Correctness)
    .add_metric(Metrics::Performance)
    .add_metric(Metrics::Readability)
    .add_metric(Metrics::Security)
    .build()?;
```

### 8.2 评分计算

```rust
let score = evaluator.evaluate(
    &candidate,
    &baseline,  // 原始代码
).await?;

println!("Overall score: {}", score.overall);
println!("Correctness: {}", score.correctness);
println!("Performance: {}", score.performance);
```

## 9. Solidify（固化）

### 9.1 存入基因池

```rust
use oris_runtime::evolution::gene_pool::GenePool;

// 创建胶囊并固化
let capsule = Capsule::new(issue_id, gene.clone())
    .add_result(ValidationResult { passed: true, ... })
    .add_evaluation(score);

gene_pool.solidify(capsule).await?;
```

### 9.2 置信度初始化

```rust
// 基于验证结果设置初始置信度
match (validation_result.passed, evaluator.score()) {
    (true, s) if s > 0.9 => gene.set_confidence(0.9),
    (true, s) if s > 0.7 => gene.set_confidence(0.7),
    (true, _) => gene.set_confidence(0.5),
    (false, _) => gene.set_confidence(0.1),
}
```

## 10. Reuse（复用）

### 10.1 从基因池检索

```rust
use oris_runtime::evolution::gene_pool::GenePool;

let similar = gene_pool.find_similar(
    &current_issue,
    5,  // 返回 5 个最相似的
).await?;

for gene in similar {
    println!("Gene: {}", gene.id());
    println!("Similarity: {}", gene.similarity());
    println!("Confidence: {}", gene.confidence());
}
```

### 10.2 置信度更新

```rust
// 成功复用后提升置信度
gene_pool.on_reuse_success(gene_id, context).await?;

// 失败后降低
gene_pool.on_reuse_failure(gene_id, error).await?;
```

## 11. 完整示例

### 11.1 运行进化

```rust
use oris_runtime::evolution::{EvolutionPipeline, GenePool};
use oris_runtime::evolution::signal::Signal;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. 初始化基因池
    let gene_pool = GenePool::new("postgres://...".to_string())?;
    
    // 2. 创建进化管道
    let pipeline = EvolutionPipeline::builder()
        .name("auto_fix")
        .build()?;
    
    // 3. 创建问题信号
    let signal = Signal::new(
        SignalType::CompilationError,
        "error[E0425]: cannot find value `user` in this scope",
        SourceLocation {
            file: "src/handler.rs",
            line: 15,
            column: 10,
        },
    );
    
    // 4. 运行进化
    let result = pipeline.evolve(signal, &gene_pool).await?;
    
    match result {
        EvolutionResult::Success(capsule) => {
            println!("Fixed! Gene: {}", capsule.gene().id());
            println!("Confidence: {:?}", capsule.gene().confidence());
        }
        EvolutionResult::Failed(reasons) => {
            println!("Evolution failed: {:?}", reasons);
        }
    }
    
    Ok(())
}
```

## 12. 小结

Oris 的自进化系统：

1. **Gene** — 可复用的解决方案单元
2. **Capsule** — 封装的进化事件
3. **Confidence** — 置信度追踪
4. **Signal** — 问题信号提取
5. **Selector** — 基因选择（遗传/规则）
6. **Mutator** — 代码变异
7. **Sandbox** — 安全沙箱执行
8. **Validator** — 验证（测试/静态分析）
9. **Evaluator** — 评估（多维度评分）
10. **Solidify** — 固化到基因池
11. **Reuse** — 复用 + 置信度更新

**Oris = 第一个真正能让代码"自己变好"的运行时。**

---

*下篇预告：诊断信号提取 (Evokernel)——从运行时提取问题*
