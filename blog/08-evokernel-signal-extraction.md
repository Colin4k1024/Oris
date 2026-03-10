# 诊断信号提取 (Evokernel)：从运行时提取问题

> 没有问题信号，就无法进化。Evokernel = 问题的"探测器"。

## 0. 为什么需要信号提取？

```
进化的前提是"发现问题"。

如果没有信号：
- Agent 不知道代码有 bug
- 不知道测试失败了
- 不知道性能发化了

Evokernel = 从运行时提取"问题信号"
```

## 1. Signal 类型

### 1.1 支持的信号类型

| 信号类型 | 来源 | 描述 |
|----------|------|------|
| **CompilationError** | 编译器 | 编译失败 |
| **RuntimeError** | 运行时 | Panic/崩溃 |
| **TestFailure** | 测试框架 | 测试失败 |
| **PerformanceRegression** | 性能监控 | 性能退化 |
| **MemoryLeak** | 内存分析 | 内存泄漏 |
| **LogicError** | 静态分析 | 逻辑错误 |

### 1.2 Signal 结构

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signal {
    /// 信号 ID
    pub id: String,
    
    /// 信号类型
    pub signal_type: SignalType,
    
    /// 原始错误信息
    pub message: String,
    
    /// 源代码位置
    pub location: Option<SourceLocation>,
    
    /// 相关代码上下文
    pub context: Option<CodeContext>,
    
    /// 严重程度
    pub severity: Severity,
    
    /// 元数据
    pub metadata: HashMap<String, Value>,
    
    /// 发现时间
    pub timestamp: i64,
}

#[derive(Debug, Clone)]
pub enum SignalType {
    CompilationError,
    RuntimeError,
    TestFailure,
    PerformanceRegression,
    MemoryLeak,
    LogicError,
    Custom(String),
}

#[derive(Debug, Clone)]
pub enum Severity {
    Critical,  // 必须修复
    High,      // 应该修复
    Medium,    // 建议修复
    Low,       // 可以修复
}
```

## 2. 从编译器提取信号

### 2.1 Rust 编译器

```rust
use oris_runtime::evokernel::compilers::RustCompiler;

let evokernel = RustCompiler::new();

// 捕获编译输出
let output = Command::new("cargo build")
    .output()?;

if !output.status.success() {
    let stderr = String::from_utf8_lossy(&output.stderr);
    
    // 提取信号
    let signals = evokernel.extract_signals(&stderr);
    
    for signal in signals {
        println!("Signal: {:?}", signal);
    }
}
```

### 2.2 信号提取规则

```rust
impl RustCompiler {
    fn extract_signals(&self, output: &str) -> Vec<Signal> {
        let mut signals = Vec::new();
        
        // error[E0425]: cannot find value `foo` in this scope
        let e0425 = Regex::new(r"error\[E(\d+)\]: (.+)")?;
        for cap in e0425.captures_iter(output) {
            let code = cap.get(1).unwrap().as_str();
            let message = cap.get(2).unwrap().as_str();
            
            signals.push(Signal::new(
                SignalType::CompilationError,
                message,
            ).with_error_code(code));
        }
        
        // warning: unused variable
        let warning = Regex::new(r"warning: (.+)")?;
        for cap in warning.captures_iter(output) {
            let message = cap.get(1).unwrap().as_str();
            
            signals.push(Signal::new(
                SignalType::CompilationError,  // 编译警告也归为此类
                message,
            ).with_severity(Severity::Low));
        }
        
        signals
    }
}
```

### 2.3 解析源代码位置

```rust
// src/main.rs:42:5:
let location_pattern = Regex::new(
    r"(.+?):(\d+):(\d+):"
)?;

if let Some(cap) = location_pattern.captures(&error_line) {
    let file = cap.get(1).unwrap().as_str();
    let line: u32 = cap.get(2).unwrap().as_str().parse()?;
    let column: u32 = cap.get(3).unwrap().as_str().parse()?;
    
    signal.location = Some(SourceLocation {
        file: file.to_string(),
        line,
        column,
    });
}
```

## 3. 从测试失败提取

### 3.1 测试框架集成

```rust
use oris_runtime::evokernel::testers::{Tester, TestResult};

let evokernel = RustTester::new();

// 运行测试
let output = Command::new("cargo test")
    .arg("--no-fail-fast")
    .output()?;

if !output.status.success() {
    let signals = evokernel.extract_from_test_output(&output);
    
    for signal in signals {
        println!("Test failed: {}", signal.message);
    }
}
```

### 3.2 提取失败信息

```rust
impl RustTester {
    fn extract_from_test_output(&self, output: &str) -> Vec<Signal> {
        let mut signals = Vec::new();
        
        // test tests::test_name ... FAILED
        let test_failed = Regex::new(
            r"test (.+?) \.\.\. (FAILED|ok|ignored)"
        )?;
        
        for cap in test_failed.captures_iter(output) {
            let test_name = cap.get(1).unwrap().as_str();
            let status = cap.get(2).unwrap().as_str();
            
            if status == "FAILED" {
                // 提取失败原因
                let failure_info = self.extract_failure_context(output, test_name);
                
                signals.push(Signal::new(
                    SignalType::TestFailure,
                    format!("Test '{}' failed: {}", test_name, failure_info.message),
                ).with_metadata("test_name", test_name));
            }
        }
        
        signals
    }
}
```

## 4. 从运行时错误提取

### 4.1 Panic 捕获

```rust
use oris_runtime::evokernel::runtime::RuntimeMonitor;

// 启动监控
let monitor = RuntimeMonitor::new()
    .capture_panics(true)
    .capture_errors(true)
    .build()?;

monitor.start();

// 程序崩溃后
let panic_info = monitor.get_last_panic().await?;

let signals = evokernel.extract_from_panic(panic_info);
```

### 4.2 Panic 信号

```rust
impl Evokernel {
    fn extract_from_panic(&self, panic_info: &PanicInfo) -> Vec<Signal> {
        let mut signals = Vec::new();
        
        // thread 'main' panicked at '...'
        let message = &panic_info.message;
        
        // 解析 panic 类型
        let panic_type = match message.as_str() {
            s if s.contains("index out of bounds") => "IndexOutOfBounds",
            s if s.contains("null") => "NullPointer",
            s if s.contains("overflow") => "Overflow",
            s if s.contains("attempt to divide by zero") => "DivideByZero",
            s if s.contains("task panicked") => "TaskPanic",
            _ => "Unknown",
        };
        
        signals.push(Signal::new(
            SignalType::RuntimeError,
            message.clone(),
        ).with_metadata("panic_type", panic_type)
          .with_location(panic_info.location.clone())
          .with_severity(Severity::Critical));
        
        signals
    }
}
```

## 5. 性能监控

### 5.1 性能回归检测

```rust
use oris_runtime::evokernel::performance::PerformanceMonitor;

let monitor = PerformanceMonitor::new()
    .baseline(Baseline::load("tests/baseline.json")?)
    .threshold(1.5)  // 超过 baseline 1.5 倍视为回归
    .build()?;

let metrics = monitor.measure_execution(|| {
    // 要测量的代码
    do_something()
}).await?;

if let Some(regression) = monitor.check_regression(&metrics) {
    let signal = Signal::new(
        SignalType::PerformanceRegression,
        format!("Execution time increased by {:.1}x", regression.ratio),
    ).with_metadata("baseline_ms", regression.baseline_ms)
     .with_metadata("actual_ms", regression.actual_ms)
     .with_severity(Severity::Medium);
    
    signals.push(signal);
}
```

### 5.2 内存泄漏检测

```rust
let monitor = MemoryMonitor::new()
    .sampling_interval(Duration::from_secs(1))
    .threshold_mb(100)  // 内存增长超过 100MB
    .build()?;

let leak = monitor.detect_leak(|| {
    // 多次执行，观察内存增长
    run_iterations(100)
}).await?;

if let Some(leak_info) = leak {
    let signal = Signal::new(
        SignalType::MemoryLeak,
        format!("Memory leaked: {} MB over {} iterations", 
            leak_info.leaked_mb, leak_info.iterations),
    ).with_severity(Severity::High);
}
```

## 6. 自定义信号源

### 6.1 实现 SignalExtractor

```rust
use oris_runtime::evokernel::SignalExtractor;

pub struct CustomExtractor;

impl SignalExtractor for CustomExtractor {
    fn can_extract(&self, source: &str) -> bool {
        source.starts_with("custom://")
    }
    
    fn extract(&self, source: &str) -> Result<Vec<Signal>, Error> {
        // 自定义提取逻辑
        let custom_data = source.strip_prefix("custom://").unwrap();
        parse_custom_signals(custom_data)
    }
}

// 注册
evokernel.register_extractor(Box::new(CustomExtractor));
```

### 6.2 从日志提取

```rust
struct LogExtractor {
    error_patterns: Vec<(Regex, SignalType)>,
}

impl LogExtractor {
    fn new() -> Self {
        Self {
            error_patterns: vec![
                (Regex::new(r"ERROR: (.+)").unwrap(), SignalType::LogicError),
                (Regex::new(r"FATAL: (.+)").unwrap(), SignalType::RuntimeError),
            ],
        }
    }
}

impl SignalExtractor for LogExtractor {
    fn extract(&self, log: &str) -> Result<Vec<Signal>, Error> {
        let mut signals = Vec::new();
        
        for (pattern, signal_type) in &self.error_patterns {
            for cap in pattern.captures_iter(log) {
                let message = cap.get(1).unwrap().as_str();
                signals.push(Signal::new(signal_type.clone(), message.to_string()));
            }
        }
        
        Ok(signals)
    }
}
```

## 7. 信号聚合

### 7.1 去重

```rust
use oris_runtime::evokernel::aggregator::SignalAggregator;

let aggregator = SignalAggregator::new()
    .deduplicate(true)
    .similarity_threshold(0.8)
    .build()?;

let aggregated = aggregator.aggregate(raw_signals);

for signal in aggregated {
    // 处理去重后的信号
    println!("Signal: {} (x{})", signal.message, signal.count);
}
```

### 7.2 优先级排序

```rust
let sorted = aggregator.sort_by_priority(aggregated);

fn priority(signal: &Signal) -> i32 {
    match signal.severity {
        Severity::Critical => 100,
        Severity::High => 75,
        Severity::Medium => 50,
        Severity::Low => 25,
    }
}
```

## 8. 与 Intake 集成

### 8.1 自动接入进化

```rust
use oris_runtime::intake::IssueIntake;

let intake = IssueIntake::new()
    .auto_ingest(true)
    .build()?;

let evokernel = Evokernel::new()
    .on_signal(move |signal| {
        // 自动创建 Issue
        intake.ingest(signal)
    })
    .build()?;
```

### 8.2 完整流程

```
运行代码
    ↓
捕获错误（编译/测试/运行时）
    ↓
Evokernel 提取信号
    ↓
信号聚合 + 去重
    ↓
Intake 接入（问题标准化）
    ↓
Evolution Pipeline 选择基因
    ↓
变异 + 验证 + 评估
    ↓
固化到基因池
```

## 9. 完整示例

### 9.1 集成到 CI/CD

```rust
use oris_runtime::evokernel::Evokernel;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. 创建 Evokernel
    let evokernel = Evokernel::builder()
        .compilers(vec![
            Box::new(RustCompiler::new()),
        ])
        .testers(vec![
            Box::new(RustTester::new()),
        ])
        .runtime(RuntimeMonitor::new()
            .capture_panics(true)
            .build()?)
        .build()?;
    
    // 2. 运行构建
    let build_output = Command::new("cargo build")
        .output()?;
    
    // 3. 提取信号
    let mut signals = Vec::new();
    
    if !build_output.status.success() {
        let compiler_signals = evokernel.extract_from_compiler(
            &String::from_utf8_lossy(&build_output.stderr)
        );
        signals.extend(compiler_signals);
    }
    
    // 4. 运行测试
    let test_output = Command::new("cargo test")
        .output()?;
    
    if !test_output.status.success() {
        let test_signals = evokernel.extract_from_tests(
            &String::from_utf8_lossy(&test_output.stdout)
        );
        signals.extend(test_signals);
    }
    
    // 5. 聚合
    let aggregated = evokernel.aggregate(signals);
    
    for signal in aggregated {
        println!("Signal: {:?}", signal);
    }
    
    Ok(())
}
```

## 10. 小结

Evokernel = 问题的探测器：

1. **Signal** — 问题信号结构
2. **Compiler 提取** — 从编译错误提取
3. **Tester 提取** — 从测试失败提取
4. **Runtime 提取** — 从 panic 提取
5. **性能监控** — 回归 + 泄漏检测
6. **自定义提取器** — 可扩展
7. **聚合 + 去重** — 避免重复
8. **Intake 集成** — 自动化进化

**没有信号就没有进化——Evokernel 让 Oris 能"看到"问题。**

---

*下篇预告：问题接入与优先级 (Intake)——从信号到 Issue*
