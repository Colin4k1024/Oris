//! Runtime Signal Extraction Module
//!
//! This module provides runtime signal extraction capabilities for the evolution loop:
//! - Compiler diagnostics parsing (rustc, clang, etc.)
//! - Stack trace parsing
//! - Execution log analysis
//! - Failure pattern detection
//!
//! These signals are used to drive the Detect phase of the evolution loop.

use regex_lite::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// A signal extracted from runtime execution
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RuntimeSignal {
    /// Unique signal ID
    pub signal_id: String,
    /// Signal type
    pub signal_type: RuntimeSignalType,
    /// Signal content/description
    pub content: String,
    /// Confidence score (0.0 - 1.0)
    pub confidence: f32,
    /// Source location (file:line)
    pub location: Option<String>,
    /// Additional metadata
    pub metadata: serde_json::Value,
}

/// Types of runtime signals
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSignalType {
    /// Compiler error or warning
    CompilerDiagnostic,
    /// Runtime panic or crash
    RuntimePanic,
    /// Timeout
    Timeout,
    /// Test failure
    TestFailure,
    /// Performance issue
    PerformanceIssue,
    /// Resource exhaustion (memory, disk, etc)
    ResourceExhaustion,
    /// Configuration error
    ConfigError,
    /// Security issue
    SecurityIssue,
    /// Generic error
    GenericError,
}

impl RuntimeSignal {
    /// Create a new runtime signal
    pub fn new(signal_type: RuntimeSignalType, content: String, confidence: f32) -> Self {
        Self {
            signal_id: uuid::Uuid::new_v4().to_string(),
            signal_type,
            content,
            confidence,
            location: None,
            metadata: serde_json::json!({}),
        }
    }

    /// Create with location
    pub fn with_location(mut self, location: String) -> Self {
        self.location = Some(location);
        self
    }

    /// Create with metadata
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = metadata;
        self
    }
}

/// Compiler diagnostics parser
pub struct CompilerDiagnosticsParser {
    /// Patterns for common compiler errors
    rustc_error_patterns: Vec<(Regex, RuntimeSignalType)>,
    rustc_warning_patterns: Vec<(Regex, RuntimeSignalType)>,
}

impl CompilerDiagnosticsParser {
    /// Create a new parser with default patterns
    pub fn new() -> Self {
        let rustc_error_patterns = vec![
            // Rust errors
            (
                Regex::new(r"error\[E\d+\]:\s*(.+)").unwrap(),
                RuntimeSignalType::CompilerDiagnostic,
            ),
            (
                Regex::new(r"^(?i)borrow checker error").unwrap(),
                RuntimeSignalType::CompilerDiagnostic,
            ),
            (
                Regex::new(r"^(?i)cannot find (function|struct|module|trait|macro)").unwrap(),
                RuntimeSignalType::CompilerDiagnostic,
            ),
            (
                Regex::new(r"^(?i)unresolved (import|item)").unwrap(),
                RuntimeSignalType::CompilerDiagnostic,
            ),
            (
                Regex::new(r"^(?i)type mismatch").unwrap(),
                RuntimeSignalType::CompilerDiagnostic,
            ),
            (
                Regex::new(r"^(?i)mismatched types").unwrap(),
                RuntimeSignalType::CompilerDiagnostic,
            ),
            (
                Regex::new(r"^(?i)expected (struct|enum|tuple|array)").unwrap(),
                RuntimeSignalType::CompilerDiagnostic,
            ),
            (
                Regex::new(r"^(?i)trait bound.*not satisfied").unwrap(),
                RuntimeSignalType::CompilerDiagnostic,
            ),
            (
                Regex::new(r"^(?i)the trait.*is not implemented").unwrap(),
                RuntimeSignalType::CompilerDiagnostic,
            ),
            (
                Regex::new(r"^(?i)method.*not found").unwrap(),
                RuntimeSignalType::CompilerDiagnostic,
            ),
            (
                Regex::new(r"^(?i)no method named.*found").unwrap(),
                RuntimeSignalType::CompilerDiagnostic,
            ),
            (
                Regex::new(r"^(?i)cannot borrow.*as.*mutable").unwrap(),
                RuntimeSignalType::CompilerDiagnostic,
            ),
            (
                Regex::new(r"^(?i)cannot borrow.*as.*immutable").unwrap(),
                RuntimeSignalType::CompilerDiagnostic,
            ),
            (
                Regex::new(r"^(?i)lifetime.*required").unwrap(),
                RuntimeSignalType::CompilerDiagnostic,
            ),
        ];

        let rustc_warning_patterns = vec![
            (
                Regex::new(r"warning\[W\d+\]:\s*(.+)").unwrap(),
                RuntimeSignalType::CompilerDiagnostic,
            ),
            (
                Regex::new(r"^(?i)unused (import|variable|function|struct)").unwrap(),
                RuntimeSignalType::CompilerDiagnostic,
            ),
            (
                Regex::new(r"^(?i)dead code").unwrap(),
                RuntimeSignalType::CompilerDiagnostic,
            ),
            (
                Regex::new(r"^(?i)field is never read").unwrap(),
                RuntimeSignalType::CompilerDiagnostic,
            ),
            (
                Regex::new(r"^(?i)deprecated").unwrap(),
                RuntimeSignalType::CompilerDiagnostic,
            ),
        ];

        Self {
            rustc_error_patterns,
            rustc_warning_patterns,
        }
    }

    /// Parse compiler output and extract signals
    pub fn parse(&self, output: &str) -> Vec<RuntimeSignal> {
        let mut signals = Vec::new();

        for line in output.lines() {
            // Check error patterns
            for (pattern, signal_type) in &self.rustc_error_patterns {
                if let Some(caps) = pattern.captures(line) {
                    let content = caps.get(1).map(|m| m.as_str()).unwrap_or(line);
                    signals.push(RuntimeSignal::new(
                        signal_type.clone(),
                        format!("compiler_error: {}", content),
                        0.9,
                    ));
                    break;
                }
            }

            // Check warning patterns (lower confidence)
            for (pattern, signal_type) in &self.rustc_warning_patterns {
                if let Some(caps) = pattern.captures(line) {
                    let content = caps.get(1).map(|m| m.as_str()).unwrap_or(line);
                    signals.push(RuntimeSignal::new(
                        signal_type.clone(),
                        format!("compiler_warning: {}", content),
                        0.6,
                    ));
                    break;
                }
            }
        }

        signals
    }
}

impl Default for CompilerDiagnosticsParser {
    fn default() -> Self {
        Self::new()
    }
}

/// Stack trace parser
pub struct StackTraceParser {
    /// Pattern for stack trace lines
    stack_trace_pattern: Regex,
    /// Pattern for panic messages
    panic_pattern: Regex,
}

impl StackTraceParser {
    /// Create a new parser
    pub fn new() -> Self {
        Self {
            stack_trace_pattern: Regex::new(r"(?m)^\s+at\s+(.+?)(?:\s+in\s+(.+?))?(?:\s+\(.*\))?$")
                .unwrap(),
            panic_pattern: Regex::new(r"(?i)(thread\s+.*\s+panicked|panicked\s+at)").unwrap(),
        }
    }

    /// Parse stack trace and extract signals
    pub fn parse(&self, output: &str) -> Vec<RuntimeSignal> {
        let mut signals = Vec::new();

        // Check for panic
        for line in output.lines() {
            if self.panic_pattern.is_match(line) {
                signals.push(RuntimeSignal::new(
                    RuntimeSignalType::RuntimePanic,
                    format!("panic: {}", line.trim()),
                    0.95,
                ));
            }
        }

        // Extract stack frames
        for cap in self.stack_trace_pattern.captures_iter(output) {
            if let Some(location) = cap.get(1) {
                let location_str = location.as_str().to_string();
                let confidence = if location_str.contains("main") || location_str.contains("bin") {
                    0.9
                } else {
                    0.7
                };

                signals.push(RuntimeSignal::new(
                    RuntimeSignalType::RuntimePanic,
                    format!("stack_frame: {}", location_str),
                    confidence,
                ));
            }
        }

        signals
    }
}

impl Default for StackTraceParser {
    fn default() -> Self {
        Self::new()
    }
}

/// Execution log analyzer
pub struct LogAnalyzer {
    /// Patterns for timeout errors
    timeout_patterns: Vec<Regex>,
    /// Patterns for resource errors
    resource_patterns: Vec<Regex>,
    /// Patterns for test failures
    test_failure_patterns: Vec<Regex>,
}

impl LogAnalyzer {
    /// Create a new analyzer
    pub fn new() -> Self {
        Self {
            timeout_patterns: vec![
                Regex::new(r"(?i)timeout").unwrap(),
                Regex::new(r"(?i)timed out").unwrap(),
                Regex::new(r"(?i)deadline exceeded").unwrap(),
            ],
            resource_patterns: vec![
                Regex::new(r"(?i)(out of memory|oom)").unwrap(),
                Regex::new(r"(?i)memory allocation failed").unwrap(),
                Regex::new(r"(?i)disk (full|space)").unwrap(),
                Regex::new(r"(?i)too many open files").unwrap(),
                Regex::new(r"(?i)resource temporarily unavailable").unwrap(),
            ],
            test_failure_patterns: vec![
                Regex::new(r"(?i)test failed").unwrap(),
                Regex::new(r"(?i)assertion failed").unwrap(),
                Regex::new(r"(?i)expected .* but got").unwrap(),
                Regex::new(r"(?i)panicked at").unwrap(),
            ],
        }
    }

    /// Analyze logs and extract signals
    pub fn analyze(&self, logs: &str) -> Vec<RuntimeSignal> {
        let mut signals = Vec::new();

        for line in logs.lines() {
            // Check timeouts
            for pattern in &self.timeout_patterns {
                if pattern.is_match(line) {
                    signals.push(RuntimeSignal::new(
                        RuntimeSignalType::Timeout,
                        format!("timeout: {}", line.trim()),
                        0.85,
                    ));
                    break;
                }
            }

            // Check resource issues
            for pattern in &self.resource_patterns {
                if pattern.is_match(line) {
                    signals.push(RuntimeSignal::new(
                        RuntimeSignalType::ResourceExhaustion,
                        format!("resource: {}", line.trim()),
                        0.9,
                    ));
                    break;
                }
            }

            // Check test failures
            for pattern in &self.test_failure_patterns {
                if pattern.is_match(line) {
                    signals.push(RuntimeSignal::new(
                        RuntimeSignalType::TestFailure,
                        format!("test_failure: {}", line.trim()),
                        0.9,
                    ));
                    break;
                }
            }
        }

        signals.into_iter().collect()
    }
}

impl Default for LogAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

/// Runtime signal extractor - combines all parsers
pub struct RuntimeSignalExtractor {
    compiler_parser: CompilerDiagnosticsParser,
    stack_trace_parser: StackTraceParser,
    log_analyzer: LogAnalyzer,
}

impl RuntimeSignalExtractor {
    /// Create a new extractor
    pub fn new() -> Self {
        Self {
            compiler_parser: CompilerDiagnosticsParser::new(),
            stack_trace_parser: StackTraceParser::new(),
            log_analyzer: LogAnalyzer::new(),
        }
    }

    /// Extract signals from compiler output
    pub fn extract_from_compiler(&self, output: &str) -> Vec<RuntimeSignal> {
        self.compiler_parser.parse(output)
    }

    /// Extract signals from stack trace
    pub fn extract_from_stack_trace(&self, output: &str) -> Vec<RuntimeSignal> {
        self.stack_trace_parser.parse(output)
    }

    /// Extract signals from execution logs
    pub fn extract_from_logs(&self, logs: &str) -> Vec<RuntimeSignal> {
        self.log_analyzer.analyze(logs)
    }

    /// Extract signals from all sources
    pub fn extract_all(
        &self,
        compiler_output: Option<&str>,
        stack_trace: Option<&str>,
        logs: Option<&str>,
    ) -> Vec<RuntimeSignal> {
        let mut all_signals = Vec::new();

        if let Some(output) = compiler_output {
            all_signals.extend(self.extract_from_compiler(output));
        }

        if let Some(trace) = stack_trace {
            all_signals.extend(self.extract_from_stack_trace(trace));
        }

        if let Some(log) = logs {
            all_signals.extend(self.extract_from_logs(log));
        }

        // Deduplicate by content
        let mut seen = HashSet::new();
        all_signals.retain(|s| seen.insert(s.content.clone()));

        all_signals
    }
}

impl Default for RuntimeSignalExtractor {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert runtime signals to evolution signals
impl From<&RuntimeSignal> for oris_evolution::EvolutionSignal {
    fn from(runtime: &RuntimeSignal) -> Self {
        let signal_type = match runtime.signal_type {
            RuntimeSignalType::CompilerDiagnostic => oris_evolution::SignalType::Performance {
                metric: "compiler_diagnostic".to_string(),
                improvement_potential: runtime.confidence,
            },
            RuntimeSignalType::RuntimePanic => oris_evolution::SignalType::ErrorPattern {
                error_type: "panic".to_string(),
                frequency: 1,
            },
            RuntimeSignalType::Timeout => oris_evolution::SignalType::ErrorPattern {
                error_type: "timeout".to_string(),
                frequency: 1,
            },
            RuntimeSignalType::TestFailure => oris_evolution::SignalType::ErrorPattern {
                error_type: "test_failure".to_string(),
                frequency: 1,
            },
            RuntimeSignalType::PerformanceIssue => oris_evolution::SignalType::Performance {
                metric: runtime.content.clone(),
                improvement_potential: runtime.confidence,
            },
            RuntimeSignalType::ResourceExhaustion => {
                oris_evolution::SignalType::ResourceOptimization {
                    resource_type: "memory_or_cpu".to_string(),
                    current_usage: 1.0 - runtime.confidence,
                }
            }
            _ => oris_evolution::SignalType::QualityIssue {
                issue_type: "runtime_error".to_string(),
                severity: runtime.confidence,
            },
        };

        oris_evolution::EvolutionSignal {
            signal_id: runtime.signal_id.clone(),
            signal_type,
            source_task_id: "runtime".to_string(),
            confidence: runtime.confidence,
            description: runtime.content.clone(),
            metadata: runtime.metadata.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compiler_error_parsing() {
        let parser = CompilerDiagnosticsParser::new();

        let output = r#"
error[E0425]: cannot find function `foo` in this scope
  --> src/main.rs:10:5
   |
10 |     foo();
   |     ^^^ not found in this scope

error[E0308]: mismatched types
"#;

        let signals = parser.parse(output);
        assert!(!signals.is_empty());
        assert!(signals
            .iter()
            .any(|s| s.content.contains("cannot find function")));
    }

    #[test]
    fn test_stack_trace_parsing() {
        let parser = StackTraceParser::new();

        let trace = r#"
thread 'main' panicked at 'something failed', src/main.rs:10:5
stack backtrace:
   0: <core::panic::unwind_safe>::{{closure}}
   1: std::panicking::{{closure}}
   2: main::foo
"#;

        let signals = parser.parse(trace);
        assert!(!signals.is_empty());
    }

    #[test]
    fn test_log_analysis() {
        let analyzer = LogAnalyzer::new();

        let logs = r#"
[INFO] Starting application
[ERROR] Connection timeout after 30s
[ERROR] Test case 'test_foo' failed: assertion failed
"#;

        let signals = analyzer.analyze(logs);
        assert!(!signals.is_empty());
    }

    #[test]
    fn test_runtime_signal_extractor() {
        let extractor = RuntimeSignalExtractor::new();

        let signals = extractor.extract_all(
            Some("error[E0425]: cannot find function"),
            Some("thread 'main' panicked"),
            Some("connection timeout"),
        );

        assert!(signals.len() >= 3);
    }
}
