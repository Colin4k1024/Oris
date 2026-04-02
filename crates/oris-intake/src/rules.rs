//! Custom rule engine for intake processing

use crate::signal::ExtractedSignal;
use crate::source::IntakeEvent;
use regex_lite::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A rule for processing intake events
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IntakeRule {
    /// Unique rule ID
    pub id: String,
    /// Human-readable rule name
    pub name: String,
    /// Rule description
    pub description: String,
    /// Priority (higher = evaluated first)
    pub priority: i32,
    /// Whether the rule is enabled
    pub enabled: bool,
    /// Conditions that must match
    pub conditions: RuleConditions,
    /// Actions to apply when matched
    pub actions: Vec<RuleAction>,
}

/// Conditions for rule matching
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct RuleConditions {
    /// Match source type
    #[serde(default)]
    pub source_types: Vec<String>,
    /// Match severity (if any match)
    #[serde(default)]
    pub severities: Vec<String>,
    /// Match title pattern (regex)
    #[serde(default)]
    pub title_pattern: Option<String>,
    /// Match description pattern (regex)
    #[serde(default)]
    pub description_pattern: Option<String>,
    /// Match signals containing these patterns
    #[serde(default)]
    pub signal_patterns: Vec<String>,
    /// Minimum confidence threshold
    #[serde(default)]
    pub min_confidence: Option<f32>,
}

/// Actions to perform when rule matches
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuleAction {
    /// Set custom severity
    SetSeverity { severity: String },
    /// Add tags
    AddTags { tags: Vec<String> },
    /// Set priority boost
    SetPriorityBoost { boost: i32 },
    /// Route to specific queue
    RouteToQueue { queue: String },
    /// Skip further processing
    Skip,
    /// Require manual approval
    RequireApproval,
    /// Add custom signals
    AddSignals { signals: Vec<String> },
    /// Set target
    SetTarget { target: String },
}

/// Result of applying a rule
#[derive(Clone, Debug)]
pub struct RuleApplication {
    pub rule_id: String,
    pub matched: bool,
    pub actions_applied: Vec<RuleAction>,
    pub modified_event: Option<IntakeEvent>,
    pub should_skip: bool,
}

/// Result of applying all matching rules to a single intake event.
#[derive(Clone, Debug)]
pub struct RuleProcessingResult {
    pub event: IntakeEvent,
    pub applications: Vec<RuleApplication>,
    pub should_skip: bool,
}

/// Rule engine for evaluating and applying rules
pub struct RuleEngine {
    rules: Vec<IntakeRule>,
    compiled_patterns: HashMap<String, Regex>,
}

impl RuleEngine {
    /// Create a new rule engine with default rules
    pub fn new() -> Self {
        let mut engine = Self {
            rules: Vec::new(),
            compiled_patterns: HashMap::new(),
        };

        // Add default rules
        engine.add_default_rules();
        engine
    }

    /// Create from custom rules
    pub fn with_rules(rules: Vec<IntakeRule>) -> Self {
        let mut engine = Self {
            rules,
            compiled_patterns: HashMap::new(),
        };
        engine.compile_patterns();
        engine
    }

    /// Add default rules
    fn add_default_rules(&mut self) {
        self.rules = vec![
            IntakeRule {
                id: "rule_critical_security".to_string(),
                name: "Critical Security Issues".to_string(),
                description: "Route critical security issues for immediate attention".to_string(),
                priority: 100,
                enabled: true,
                conditions: RuleConditions {
                    severities: vec!["critical".to_string()],
                    signal_patterns: vec!["security".to_string(), "vulnerability".to_string()],
                    ..Default::default()
                },
                actions: vec![
                    RuleAction::RequireApproval,
                    RuleAction::SetPriorityBoost { boost: 50 },
                ],
            },
            IntakeRule {
                id: "rule_compiler_errors".to_string(),
                name: "Compiler Errors".to_string(),
                description: "High priority for compiler errors".to_string(),
                priority: 80,
                enabled: true,
                conditions: RuleConditions {
                    signal_patterns: vec!["compiler_error".to_string()],
                    min_confidence: Some(0.7),
                    ..Default::default()
                },
                actions: vec![RuleAction::SetPriorityBoost { boost: 30 }],
            },
            IntakeRule {
                id: "rule_test_failures".to_string(),
                name: "Test Failures".to_string(),
                description: "Handle test failures from CI".to_string(),
                priority: 60,
                enabled: true,
                conditions: RuleConditions {
                    source_types: vec!["github".to_string(), "gitlab".to_string()],
                    signal_patterns: vec!["test_failure".to_string()],
                    ..Default::default()
                },
                actions: vec![RuleAction::AddTags {
                    tags: vec!["ci".to_string(), "test".to_string()],
                }],
            },
            IntakeRule {
                id: "rule_low_confidence".to_string(),
                name: "Low Confidence Events".to_string(),
                description: "Require approval for low confidence events".to_string(),
                priority: 10,
                enabled: true,
                conditions: RuleConditions {
                    min_confidence: Some(0.3),
                    ..Default::default()
                },
                actions: vec![RuleAction::RequireApproval],
            },
        ];

        self.compile_patterns();
    }

    /// Compile regex patterns for efficiency
    fn compile_patterns(&mut self) {
        self.compiled_patterns.clear();

        for rule in &self.rules {
            if let Some(ref pattern) = rule.conditions.title_pattern {
                if let Ok(re) = Regex::new(pattern) {
                    self.compiled_patterns
                        .insert(format!("{}:title", rule.id), re);
                }
            }
            if let Some(ref pattern) = rule.conditions.description_pattern {
                if let Ok(re) = Regex::new(pattern) {
                    self.compiled_patterns
                        .insert(format!("{}:desc", rule.id), re);
                }
            }
        }
    }

    /// Evaluate rules against an event
    pub fn evaluate(
        &self,
        event: &IntakeEvent,
        signals: &[ExtractedSignal],
    ) -> Vec<RuleApplication> {
        let mut results = Vec::new();

        // Sort rules by priority (highest first)
        let mut sorted_rules = self.rules.clone();
        sorted_rules.sort_by(|a, b| b.priority.cmp(&a.priority));

        for rule in sorted_rules {
            if !rule.enabled {
                continue;
            }

            let matched = self.matches_conditions(&rule.id, &rule.conditions, event, signals);

            if matched {
                let application = RuleApplication {
                    rule_id: rule.id.clone(),
                    matched: true,
                    actions_applied: rule.actions.clone(),
                    modified_event: None,
                    should_skip: rule.actions.iter().any(|a| matches!(a, RuleAction::Skip)),
                };
                results.push(application);
            }
        }

        results
    }

    /// Apply matching rules to an event and return the processed result.
    pub fn apply(&self, event: &IntakeEvent, signals: &[ExtractedSignal]) -> RuleProcessingResult {
        let applications = self.evaluate(event, signals);
        let mut modified_event = event.clone();
        let mut should_skip = false;

        for application in &applications {
            should_skip |= application.should_skip;

            for action in &application.actions_applied {
                match action {
                    RuleAction::SetSeverity { severity } => {
                        if let Some(mapped) = parse_severity(severity) {
                            modified_event.severity = mapped;
                        }
                    }
                    RuleAction::AddSignals { signals } => {
                        for signal in signals {
                            if !modified_event.signals.contains(signal) {
                                modified_event.signals.push(signal.clone());
                            }
                        }
                    }
                    RuleAction::Skip => {
                        should_skip = true;
                    }
                    _ => {}
                }
            }
        }

        RuleProcessingResult {
            event: modified_event,
            applications,
            should_skip,
        }
    }

    /// Check if conditions match
    fn matches_conditions(
        &self,
        rule_id: &str,
        conditions: &RuleConditions,
        event: &IntakeEvent,
        signals: &[ExtractedSignal],
    ) -> bool {
        // Check source type
        if !conditions.source_types.is_empty() {
            let source_match = conditions
                .source_types
                .iter()
                .any(|st| st.eq_ignore_ascii_case(&event.source_type.to_string()));
            if !source_match {
                return false;
            }
        }

        // Check severity
        if !conditions.severities.is_empty() {
            let severity_match = conditions
                .severities
                .iter()
                .any(|s| s.eq_ignore_ascii_case(&event.severity.to_string()));
            if !severity_match {
                return false;
            }
        }

        // Check title pattern
        if conditions.title_pattern.is_some() {
            let key = format!("{}:title", rule_id);
            if let Some(re) = self.compiled_patterns.get(&key) {
                if !re.is_match(&event.title) {
                    return false;
                }
            }
        }

        // Check description pattern
        if conditions.description_pattern.is_some() {
            let key = format!("{}:desc", rule_id);
            if let Some(re) = self.compiled_patterns.get(&key) {
                if !re.is_match(&event.description) {
                    return false;
                }
            }
        }

        // Check signal patterns
        if !conditions.signal_patterns.is_empty() {
            let signal_match = signals.iter().any(|s| {
                conditions
                    .signal_patterns
                    .iter()
                    .any(|p| s.content.to_lowercase().contains(&p.to_lowercase()))
            });
            if !signal_match {
                return false;
            }
        }

        // Check min confidence
        if let Some(min_conf) = conditions.min_confidence {
            let avg_conf: f32 = if signals.is_empty() {
                0.0
            } else {
                signals.iter().map(|s| s.confidence).sum::<f32>() / signals.len() as f32
            };
            if avg_conf < min_conf {
                return false;
            }
        }

        true
    }

    /// Add a custom rule
    pub fn add_rule(&mut self, rule: IntakeRule) {
        self.rules.push(rule);
        self.compile_patterns();
    }

    /// Get all rules
    pub fn get_rules(&self) -> &[IntakeRule] {
        &self.rules
    }
}

impl Default for RuleEngine {
    fn default() -> Self {
        Self::new()
    }
}

fn parse_severity(value: &str) -> Option<crate::source::IssueSeverity> {
    match value.to_ascii_lowercase().as_str() {
        "critical" => Some(crate::source::IssueSeverity::Critical),
        "high" => Some(crate::source::IssueSeverity::High),
        "medium" => Some(crate::source::IssueSeverity::Medium),
        "low" => Some(crate::source::IssueSeverity::Low),
        "info" => Some(crate::source::IssueSeverity::Info),
        _ => None,
    }
}

/// Simple ML-based classifier for problem categorization
pub struct ProblemClassifier {
    /// Known patterns for different problem types
    patterns: HashMap<String, Vec<(String, f32)>>,
}

impl ProblemClassifier {
    /// Create a new classifier
    pub fn new() -> Self {
        let mut patterns = HashMap::new();

        // Compiler/Build issues
        patterns.insert(
            "compiler_error".to_string(),
            vec![
                ("borrow".to_string(), 0.9),
                ("type mismatch".to_string(), 0.85),
                ("cannot find".to_string(), 0.8),
                ("unresolved".to_string(), 0.8),
                ("compile".to_string(), 0.7),
            ],
        );

        // Runtime errors
        patterns.insert(
            "runtime_error".to_string(),
            vec![
                ("panic".to_string(), 0.95),
                ("timeout".to_string(), 0.8),
                ("connection".to_string(), 0.75),
                ("null".to_string(), 0.7),
                ("exception".to_string(), 0.7),
            ],
        );

        // Test failures
        patterns.insert(
            "test_failure".to_string(),
            vec![
                ("test failed".to_string(), 0.95),
                ("assertion".to_string(), 0.85),
                ("expected".to_string(), 0.7),
                ("mock".to_string(), 0.6),
            ],
        );

        // Performance issues
        patterns.insert(
            "performance".to_string(),
            vec![
                ("slow".to_string(), 0.85),
                ("latency".to_string(), 0.8),
                ("memory leak".to_string(), 0.9),
                ("cpu".to_string(), 0.7),
                ("throughput".to_string(), 0.7),
            ],
        );

        // Security issues
        patterns.insert(
            "security".to_string(),
            vec![
                ("vulnerability".to_string(), 0.95),
                ("security".to_string(), 0.9),
                ("injection".to_string(), 0.85),
                ("xss".to_string(), 0.9),
                ("sql injection".to_string(), 0.9),
            ],
        );

        // Configuration issues
        patterns.insert(
            "configuration".to_string(),
            vec![
                ("config".to_string(), 0.9),
                ("missing".to_string(), 0.7),
                ("permission".to_string(), 0.75),
                ("denied".to_string(), 0.7),
            ],
        );

        Self { patterns }
    }

    /// Classify an event into problem types
    pub fn classify(&self, event: &IntakeEvent) -> Vec<ProblemCategory> {
        let text = format!("{} {}", event.title, event.description).to_lowercase();
        let mut scores = Vec::new();

        for (category, patterns) in &self.patterns {
            let mut score = 0.0;
            for (pattern, weight) in patterns {
                if text.contains(&pattern.to_lowercase()) {
                    score += weight;
                }
            }
            if score > 0.0 {
                scores.push(ProblemCategory {
                    category: category.clone(),
                    confidence: (score / patterns.len() as f32).min(1.0),
                });
            }
        }

        // Sort by confidence
        scores.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());

        scores
    }
}

impl Default for ProblemClassifier {
    fn default() -> Self {
        Self::new()
    }
}

/// A problem category with confidence
#[derive(Clone, Debug)]
pub struct ProblemCategory {
    pub category: String,
    pub confidence: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::{IntakeSourceType, IssueSeverity};

    #[test]
    fn test_rule_engine_default_rules() {
        let engine = RuleEngine::new();
        assert!(!engine.get_rules().is_empty());
    }

    #[test]
    fn test_rule_matching() {
        let engine = RuleEngine::new();

        let event = IntakeEvent {
            event_id: "test-1".to_string(),
            source_type: IntakeSourceType::Github,
            source_event_id: None,
            title: "Build failed".to_string(),
            description: "Borrow checker error".to_string(),
            severity: IssueSeverity::High,
            signals: vec![],
            raw_payload: None,
            timestamp_ms: 0,
        };

        let signals = vec![ExtractedSignal {
            signal_id: "sig-1".to_string(),
            content: "compiler_error:borrow checker".to_string(),
            signal_type: crate::signal::SignalType::CompilerError,
            confidence: 0.8,
            source: "test".to_string(),
        }];

        let results = engine.evaluate(&event, &signals);
        assert!(!results.is_empty());
    }

    #[test]
    fn test_problem_classifier() {
        let classifier = ProblemClassifier::new();

        let event = IntakeEvent {
            event_id: "test-1".to_string(),
            source_type: IntakeSourceType::Github,
            source_event_id: None,
            title: "SQL Injection vulnerability found".to_string(),
            description: "Security issue in login".to_string(),
            severity: IssueSeverity::Critical,
            signals: vec![],
            raw_payload: None,
            timestamp_ms: 0,
        };

        let categories = classifier.classify(&event);
        assert!(!categories.is_empty());
        assert_eq!(categories[0].category, "security");
    }

    #[test]
    fn test_custom_rule() {
        let mut engine = RuleEngine::new();

        let rule = IntakeRule {
            id: "custom_rule".to_string(),
            name: "Custom Rule".to_string(),
            description: "Test custom rule".to_string(),
            priority: 50,
            enabled: true,
            conditions: RuleConditions {
                severities: vec!["critical".to_string()],
                ..Default::default()
            },
            actions: vec![RuleAction::Skip],
        };

        engine.add_rule(rule);
        assert!(engine.get_rules().len() > 4);
    }

    #[test]
    fn test_apply_returns_skip_when_matching_rule_requests_it() {
        let engine = RuleEngine::with_rules(vec![IntakeRule {
            id: "skip_high".to_string(),
            name: "Skip high severity".to_string(),
            description: "skip event".to_string(),
            priority: 100,
            enabled: true,
            conditions: RuleConditions {
                severities: vec!["high".to_string()],
                ..Default::default()
            },
            actions: vec![RuleAction::Skip],
        }]);

        let event = IntakeEvent {
            event_id: "evt-1".to_string(),
            source_type: IntakeSourceType::Github,
            source_event_id: None,
            title: "Build failed".to_string(),
            description: "compiler broke".to_string(),
            severity: IssueSeverity::High,
            signals: vec![],
            raw_payload: None,
            timestamp_ms: 0,
        };

        let result = engine.apply(&event, &[]);
        assert!(result.should_skip);
        assert_eq!(result.applications.len(), 1);
        assert_eq!(result.applications[0].rule_id, "skip_high");
    }
}
