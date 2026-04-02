//! Signal extraction from intake events

use regex_lite::Regex;
use serde::{Deserialize, Serialize};

/// A extracted signal from an intake event
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExtractedSignal {
    /// Signal ID
    pub signal_id: String,

    /// Signal content/description
    pub content: String,

    /// Signal type
    pub signal_type: SignalType,

    /// Confidence score (0.0 - 1.0)
    pub confidence: f32,

    /// Source of the signal
    pub source: String,
}

/// Types of signals that can be extracted
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SignalType {
    /// Compiler diagnostic signal
    CompilerError,
    /// Runtime error signal
    RuntimeError,
    /// Test failure signal
    TestFailure,
    /// Performance issue signal
    Performance,
    /// Security issue signal
    Security,
    /// Configuration issue signal
    ConfigError,
    /// Generic error signal
    GenericError,
}

impl std::fmt::Display for SignalType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SignalType::CompilerError => write!(f, "compiler_error"),
            SignalType::RuntimeError => write!(f, "runtime_error"),
            SignalType::TestFailure => write!(f, "test_failure"),
            SignalType::Performance => write!(f, "performance"),
            SignalType::Security => write!(f, "security"),
            SignalType::ConfigError => write!(f, "config_error"),
            SignalType::GenericError => write!(f, "generic_error"),
        }
    }
}

/// Signal extractor for converting intake events to evolution signals
pub struct SignalExtractor {
    /// Minimum confidence threshold
    min_confidence: f32,

    /// Compiler error patterns
    compiler_patterns: Vec<(&'static str, Regex)>,

    /// Runtime error patterns
    runtime_patterns: Vec<(&'static str, Regex)>,

    /// Test failure patterns
    test_patterns: Vec<(&'static str, Regex)>,

    /// Performance issue patterns
    performance_patterns: Vec<(&'static str, Regex)>,

    /// Security issue patterns
    security_patterns: Vec<(&'static str, Regex)>,
}

impl SignalExtractor {
    /// Create a new signal extractor with default patterns
    pub fn new(min_confidence: f32) -> Self {
        Self {
            min_confidence,
            compiler_patterns: vec![
                (
                    "borrow checker",
                    Regex::new(r"(?i)borrow.*(error|checker)").unwrap(),
                ),
                ("type mismatch", Regex::new(r"(?i)type.*mismatch").unwrap()),
                (
                    "missing import",
                    Regex::new(r"(?i)(cannot find|missing).*(import|struct|function)").unwrap(),
                ),
                (
                    "unresolved import",
                    Regex::new(r"(?i)unresolved.*import").unwrap(),
                ),
                (
                    "unused",
                    Regex::new(r"(?i)unused.*(import|variable|function)").unwrap(),
                ),
            ],
            runtime_patterns: vec![
                ("timeout", Regex::new(r"(?i)timeout").unwrap()),
                (
                    "connection refused",
                    Regex::new(r"(?i)(connection|connect).*(refused|failed)").unwrap(),
                ),
                (
                    "out of memory",
                    Regex::new(r"(?i)(out of memory|oom)").unwrap(),
                ),
                ("panic", Regex::new(r"(?i)panic").unwrap()),
                (
                    "null pointer",
                    Regex::new(r"(?i)(null|nil).*pointer").unwrap(),
                ),
            ],
            test_patterns: vec![
                ("test failed", Regex::new(r"(?i)test.*failed").unwrap()),
                (
                    "assertion failed",
                    Regex::new(r"(?i)assertion.*failed").unwrap(),
                ),
                (
                    "expected.*actual",
                    Regex::new(r"(?i)expected.*actual").unwrap(),
                ),
            ],
            performance_patterns: vec![
                (
                    "slow",
                    Regex::new(r"(?i)(slow|latency).*(than|exceed)").unwrap(),
                ),
                ("memory leak", Regex::new(r"(?i)memory.*leak").unwrap()),
                (
                    "high cpu",
                    Regex::new(r"(?i)(high|cpu).*(usage|load)").unwrap(),
                ),
            ],
            security_patterns: vec![
                ("vulnerability", Regex::new(r"(?i)vulnerability").unwrap()),
                ("injection", Regex::new(r"(?i)(sql|xss|injection)").unwrap()),
                (
                    "auth failed",
                    Regex::new(r"(?i)(auth|permission).*(failed|denied)").unwrap(),
                ),
            ],
        }
    }

    /// Extract signals from an intake event
    pub fn extract(&self, event: &crate::source::IntakeEvent) -> Vec<ExtractedSignal> {
        let mut signals = Vec::new();

        // Process the title and description
        let text = format!("{}\n{}", event.title, event.description);

        // Check compiler patterns
        for (name, pattern) in &self.compiler_patterns {
            if pattern.is_match(&text) {
                signals.push(ExtractedSignal {
                    signal_id: uuid::Uuid::new_v4().to_string(),
                    content: format!("compiler_error:{}", name),
                    signal_type: SignalType::CompilerError,
                    confidence: 0.8,
                    source: event.source_type.to_string(),
                });
            }
        }

        // Check runtime patterns
        for (name, pattern) in &self.runtime_patterns {
            if pattern.is_match(&text) {
                signals.push(ExtractedSignal {
                    signal_id: uuid::Uuid::new_v4().to_string(),
                    content: format!("runtime_error:{}", name),
                    signal_type: SignalType::RuntimeError,
                    confidence: 0.75,
                    source: event.source_type.to_string(),
                });
            }
        }

        // Check test patterns
        for (name, pattern) in &self.test_patterns {
            if pattern.is_match(&text) {
                signals.push(ExtractedSignal {
                    signal_id: uuid::Uuid::new_v4().to_string(),
                    content: format!("test_failure:{}", name),
                    signal_type: SignalType::TestFailure,
                    confidence: 0.85,
                    source: event.source_type.to_string(),
                });
            }
        }

        // Check performance patterns
        for (name, pattern) in &self.performance_patterns {
            if pattern.is_match(&text) {
                signals.push(ExtractedSignal {
                    signal_id: uuid::Uuid::new_v4().to_string(),
                    content: format!("performance:{}", name),
                    signal_type: SignalType::Performance,
                    confidence: 0.7,
                    source: event.source_type.to_string(),
                });
            }
        }

        // Check security patterns
        for (name, pattern) in &self.security_patterns {
            if pattern.is_match(&text) {
                signals.push(ExtractedSignal {
                    signal_id: uuid::Uuid::new_v4().to_string(),
                    content: format!("security:{}", name),
                    signal_type: SignalType::Security,
                    confidence: 0.9,
                    source: event.source_type.to_string(),
                });
            }
        }

        // If no specific pattern matched, add a generic signal based on severity
        if signals.is_empty() {
            let confidence = match event.severity {
                crate::source::IssueSeverity::Critical => 0.9,
                crate::source::IssueSeverity::High => 0.75,
                crate::source::IssueSeverity::Medium => 0.5,
                crate::source::IssueSeverity::Low => 0.35,
                crate::source::IssueSeverity::Info => 0.2,
            };

            signals.push(ExtractedSignal {
                signal_id: uuid::Uuid::new_v4().to_string(),
                content: format!("issue:{}", event.title),
                signal_type: SignalType::GenericError,
                confidence,
                source: event.source_type.to_string(),
            });
        }

        // Filter by minimum confidence
        signals.retain(|s| s.confidence >= self.min_confidence);

        signals
    }
}

impl Default for SignalExtractor {
    fn default() -> Self {
        Self::new(0.5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::{IntakeEvent, IntakeSourceType, IssueSeverity};

    #[test]
    fn test_extract_compiler_error() {
        let extractor = SignalExtractor::default();

        let event = IntakeEvent {
            event_id: "test-1".to_string(),
            source_type: IntakeSourceType::Github,
            source_event_id: None,
            title: "Build failed".to_string(),
            description: "error: borrow checker error in src/main.rs".to_string(),
            severity: IssueSeverity::High,
            signals: vec![],
            raw_payload: None,
            timestamp_ms: 0,
        };

        let signals = extractor.extract(&event);
        assert!(!signals.is_empty());
        assert!(signals
            .iter()
            .any(|s| s.signal_type == SignalType::CompilerError));
    }

    #[test]
    fn test_extract_runtime_error() {
        let extractor = SignalExtractor::default();

        let event = IntakeEvent {
            event_id: "test-2".to_string(),
            source_type: IntakeSourceType::Gitlab,
            source_event_id: None,
            title: "Deployment failed".to_string(),
            description: "Error: connection timeout to database".to_string(),
            severity: IssueSeverity::High,
            signals: vec![],
            raw_payload: None,
            timestamp_ms: 0,
        };

        let signals = extractor.extract(&event);
        assert!(signals
            .iter()
            .any(|s| s.signal_type == SignalType::RuntimeError));
    }

    #[test]
    fn test_min_confidence_filter() {
        let extractor = SignalExtractor::new(0.8); // High threshold

        let event = IntakeEvent {
            event_id: "test-3".to_string(),
            source_type: IntakeSourceType::Http,
            source_event_id: None,
            title: "Minor issue".to_string(),
            description: "Some minor issue occurred".to_string(),
            severity: IssueSeverity::Low,
            signals: vec![],
            raw_payload: None,
            timestamp_ms: 0,
        };

        let signals = extractor.extract(&event);
        // With high threshold and low severity, should filter out
        for s in &signals {
            assert!(s.confidence >= 0.8);
        }
    }
}
