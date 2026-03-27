//! Semantic task matching for recurring bounded classes (#400).
//!
//! Extends the keyword-overlap matching in `task_class.rs` with bounded
//! equivalence matching that can recognise recurring problem classes even
//! when the exact signal strings differ.
//!
//! # Design
//!
//! A `SemanticTaskMatcher` wraps a `TaskClassMatcher` and adds:
//! - Normalised signal canonicalisation (strip noise, lowercase, dedup tokens)
//! - Bounded equivalence classes that group signals by structural similarity
//! - Confidence-gated matching: only returns a match when the composite score
//!   meets the configured threshold

use crate::task_class::{builtin_task_classes, TaskClass, TaskClassMatcher};
use serde::{Deserialize, Serialize};

// ─── Signal normalisation ────────────────────────────────────────────────────

/// Normalise a raw signal string into a canonical form for equivalence matching.
///
/// Steps:
/// 1. Lowercase
/// 2. Strip Rust error code brackets (e.g. `error[E0308]` → `error e0308`)
/// 3. Collapse whitespace
/// 4. Remove line/column references (e.g. `src/main.rs:10:5`)
/// 5. Deduplicate tokens
pub fn normalise_signal(raw: &str) -> String {
    let lowered = raw.to_lowercase();
    // Strip bracket notation around error codes
    let stripped = lowered.replace('[', " ").replace(']', " ");
    // Remove file:line:col references
    let mut tokens: Vec<&str> = stripped
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .filter(|t| {
            // Filter out pure numeric tokens (line numbers, column numbers)
            !t.chars().all(|c| c.is_ascii_digit()) || t.starts_with('e')
        })
        .collect();
    tokens.dedup();
    tokens.join(" ")
}

// ─── Bounded equivalence ─────────────────────────────────────────────────────

/// A bounded equivalence class groups structurally similar signals.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BoundedEquivalenceClass {
    /// The task class ID this equivalence class maps to.
    pub task_class_id: String,
    /// Canonical signal patterns that define this equivalence class.
    /// A signal matches if its normalised form contains all tokens from
    /// any one of these patterns.
    pub canonical_patterns: Vec<Vec<String>>,
}

impl BoundedEquivalenceClass {
    pub fn new(task_class_id: impl Into<String>, patterns: Vec<Vec<String>>) -> Self {
        Self {
            task_class_id: task_class_id.into(),
            canonical_patterns: patterns,
        }
    }

    /// Check if a normalised signal matches any canonical pattern in this class.
    pub fn matches(&self, normalised_signal: &str) -> bool {
        let tokens: Vec<&str> = normalised_signal.split_whitespace().collect();
        self.canonical_patterns.iter().any(|pattern| {
            pattern
                .iter()
                .all(|kw| tokens.iter().any(|t| t.contains(kw.as_str())))
        })
    }
}

/// Return the built-in bounded equivalence classes for Rust compiler errors.
pub fn builtin_equivalence_classes() -> Vec<BoundedEquivalenceClass> {
    vec![
        BoundedEquivalenceClass::new(
            "missing-import",
            vec![
                vec!["cannot".into(), "find".into()],
                vec!["unresolved".into(), "import".into()],
                vec!["undefined".into(), "symbol".into()],
                vec!["e0425".into()],
                vec!["e0433".into()],
            ],
        ),
        BoundedEquivalenceClass::new(
            "type-mismatch",
            vec![
                vec!["mismatched".into(), "type".into()],
                vec!["expected".into(), "found".into()],
                vec!["e0308".into()],
            ],
        ),
        BoundedEquivalenceClass::new(
            "borrow-conflict",
            vec![
                vec!["cannot".into(), "borrow".into()],
                vec!["moved".into(), "value".into()],
                vec!["e0502".into()],
                vec!["e0505".into()],
            ],
        ),
        BoundedEquivalenceClass::new(
            "test-failure",
            vec![
                vec!["test".into(), "failed".into()],
                vec!["assertion".into(), "failed".into()],
                vec!["panicked".into(), "at".into()],
            ],
        ),
        BoundedEquivalenceClass::new(
            "performance",
            vec![
                vec!["slow".into(), "response".into()],
                vec!["latency".into(), "exceeded".into()],
                vec!["timeout".into()],
            ],
        ),
    ]
}

// ─── SemanticTaskMatcher ─────────────────────────────────────────────────────

/// Configuration for semantic matching.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SemanticMatchConfig {
    /// Minimum composite score to accept a match (0.0–1.0).
    pub min_confidence: f32,
    /// Weight for keyword overlap score (0.0–1.0).
    pub keyword_weight: f32,
    /// Weight for equivalence class match (0.0–1.0).
    pub equivalence_weight: f32,
}

impl Default for SemanticMatchConfig {
    fn default() -> Self {
        Self {
            min_confidence: 0.4,
            keyword_weight: 0.5,
            equivalence_weight: 0.5,
        }
    }
}

/// Result of a semantic match attempt.
#[derive(Clone, Debug)]
pub struct SemanticMatchResult {
    /// The matched task class ID, or `None` if no match met the threshold.
    pub task_class_id: Option<String>,
    /// Composite confidence score.
    pub confidence: f32,
    /// Whether the match came from keyword overlap.
    pub keyword_match: bool,
    /// Whether the match came from equivalence class.
    pub equivalence_match: bool,
}

/// Semantic task matcher that combines keyword overlap with bounded equivalence.
pub struct SemanticTaskMatcher {
    keyword_matcher: TaskClassMatcher,
    equivalence_classes: Vec<BoundedEquivalenceClass>,
    config: SemanticMatchConfig,
}

impl SemanticTaskMatcher {
    pub fn new(
        classes: Vec<TaskClass>,
        equivalence_classes: Vec<BoundedEquivalenceClass>,
        config: SemanticMatchConfig,
    ) -> Self {
        Self {
            keyword_matcher: TaskClassMatcher::new(classes),
            equivalence_classes,
            config,
        }
    }

    /// Create a matcher with built-in task classes and equivalence classes.
    pub fn with_builtins() -> Self {
        Self::new(
            builtin_task_classes(),
            builtin_equivalence_classes(),
            SemanticMatchConfig::default(),
        )
    }

    /// Override the configuration.
    pub fn with_config(mut self, config: SemanticMatchConfig) -> Self {
        self.config = config;
        self
    }

    /// Match signals against task classes using combined keyword + equivalence scoring.
    pub fn match_signals(&self, signals: &[String]) -> SemanticMatchResult {
        let normalised: Vec<String> = signals.iter().map(|s| normalise_signal(s)).collect();

        // Phase 1: keyword overlap via TaskClassMatcher
        let keyword_result = self.keyword_matcher.classify(signals);
        let keyword_score = if keyword_result.is_some() { 1.0 } else { 0.0 };

        // Phase 2: bounded equivalence matching
        let mut best_equiv: Option<(&str, f32)> = None;
        for eq_class in &self.equivalence_classes {
            let match_count = normalised.iter().filter(|ns| eq_class.matches(ns)).count();
            if match_count > 0 {
                let score = match_count as f32 / normalised.len().max(1) as f32;
                match best_equiv {
                    None => best_equiv = Some((&eq_class.task_class_id, score)),
                    Some((_, prev)) if score > prev => {
                        best_equiv = Some((&eq_class.task_class_id, score))
                    }
                    _ => {}
                }
            }
        }

        let equiv_score = best_equiv.map(|(_, s)| s).unwrap_or(0.0);

        // Composite score
        let composite = keyword_score * self.config.keyword_weight
            + equiv_score * self.config.equivalence_weight;

        // Determine winning class ID
        let task_class_id = if composite >= self.config.min_confidence {
            // Prefer keyword match class, fall back to equivalence class
            keyword_result
                .map(|c| c.id.clone())
                .or_else(|| best_equiv.map(|(id, _)| id.to_string()))
        } else {
            None
        };

        SemanticMatchResult {
            task_class_id,
            confidence: composite,
            keyword_match: keyword_result.is_some(),
            equivalence_match: best_equiv.is_some(),
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalise_strips_brackets_and_line_numbers() {
        let raw = "error[E0308]: mismatched types at src/main.rs:10:5";
        let norm = normalise_signal(raw);
        assert!(norm.contains("e0308"));
        assert!(norm.contains("mismatched"));
        assert!(!norm.contains("["));
        assert!(!norm.contains("]"));
    }

    #[test]
    fn normalise_deduplicates_tokens() {
        let raw = "error error error type type";
        let norm = normalise_signal(raw);
        // After dedup, consecutive duplicates are removed
        assert_eq!(norm, "error type");
    }

    #[test]
    fn equivalence_class_matches_canonical_pattern() {
        let eq = BoundedEquivalenceClass::new(
            "type-mismatch",
            vec![vec!["mismatched".into(), "type".into()]],
        );
        assert!(eq.matches("mismatched type expected u32 found string"));
        assert!(!eq.matches("borrow checker conflict"));
    }

    #[test]
    fn semantic_matcher_keyword_match() {
        let matcher = SemanticTaskMatcher::with_builtins();
        let signals = vec!["error[E0308]: mismatched types expected u32 found String".into()];
        let result = matcher.match_signals(&signals);
        assert_eq!(result.task_class_id.as_deref(), Some("type-mismatch"));
        assert!(result.keyword_match);
        assert!(result.confidence >= 0.4);
    }

    #[test]
    fn semantic_matcher_equivalence_only_match() {
        // Use a signal that matches equivalence but has low keyword overlap
        let matcher = SemanticTaskMatcher::with_builtins().with_config(SemanticMatchConfig {
            min_confidence: 0.2,
            keyword_weight: 0.5,
            equivalence_weight: 0.5,
        });
        let signals = vec!["cannot borrow `x` as mutable".into()];
        let result = matcher.match_signals(&signals);
        assert!(result.equivalence_match);
        assert!(result.confidence > 0.0);
    }

    #[test]
    fn semantic_matcher_no_match_below_threshold() {
        let matcher = SemanticTaskMatcher::with_builtins().with_config(SemanticMatchConfig {
            min_confidence: 0.99,
            keyword_weight: 0.5,
            equivalence_weight: 0.5,
        });
        let signals = vec!["completely unrelated signal about networking".into()];
        let result = matcher.match_signals(&signals);
        assert!(result.task_class_id.is_none());
    }

    #[test]
    fn semantic_matcher_empty_signals() {
        let matcher = SemanticTaskMatcher::with_builtins();
        let result = matcher.match_signals(&[]);
        assert!(result.task_class_id.is_none());
        assert_eq!(result.confidence, 0.0);
    }

    #[test]
    fn builtin_equivalence_classes_cover_all_builtin_task_classes() {
        let eq_classes = builtin_equivalence_classes();
        let task_classes = builtin_task_classes();
        for tc in &task_classes {
            assert!(
                eq_classes.iter().any(|eq| eq.task_class_id == tc.id),
                "missing equivalence class for task class '{}'",
                tc.id
            );
        }
    }

    #[test]
    fn semantic_matcher_multiple_signals_accumulate() {
        let matcher = SemanticTaskMatcher::with_builtins();
        let signals = vec![
            "expected type u32".into(),
            "found type String — type mismatch".into(),
        ];
        let result = matcher.match_signals(&signals);
        assert_eq!(result.task_class_id.as_deref(), Some("type-mismatch"));
    }
}
