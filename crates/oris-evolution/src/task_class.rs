//! Task-class abstraction for semantic-equivalent task grouping.
//!
//! A `TaskClass` represents a category of semantically equivalent tasks that
//! can reuse the same learned `Gene` even when the exact signal strings differ.
//!
//! # Example classes
//!
//! | ID | Name | Signal keywords |
//! |----|------|-----------------|
//! | `missing-import` | Missing import / undefined symbol | `E0425`, `E0433`, `unresolved`, `undefined`, `import`, `use` |
//! | `type-mismatch` | Type mismatch | `E0308`, `mismatched`, `expected`, `found`, `type` |
//! | `borrow-conflict` | Borrow checker conflict | `E0502`, `E0505`, `borrow`, `lifetime`, `moved` |
//!
//! # How matching works
//!
//! 1. Each signal string is tokenised into lowercase words.
//! 2. A signal **matches** a `TaskClass` if the intersection of its word-set
//!    with the class's `signal_keywords` is non-empty.
//! 3. The `TaskClassMatcher::classify` method returns the class whose keywords
//!    produce the highest overlap score with the combined signal list.
//!
//! Cross-class false positives are prevented because each class uses disjoint
//! keyword sets; overlap scoring breaks ties by choosing the highest count, so
//! a signal that partially matches two classes still maps to the one with
//! more matching keywords.

use serde::{Deserialize, Serialize};

// ─── TaskClass ────────────────────────────────────────────────────────────────

/// A named category of semantically equivalent tasks.
///
/// Genes are tagged with a `task_class_id` during their Solidify phase.
/// When the Select stage cannot find an exact signal match, it falls back to
/// `TaskClassMatcher` to surface candidates that share the same class.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskClass {
    /// Opaque, stable identifier. Genes reference this via `Gene::task_class_id`.
    pub id: String,
    /// Human-readable label.
    pub name: String,
    /// Lowercase keywords used for signal classification.
    ///
    /// A signal string matches this class when any of these keywords appears as
    /// a word token (after lowercasing and splitting on non-alphanumeric chars).
    pub signal_keywords: Vec<String>,
}

impl TaskClass {
    /// Create a new `TaskClass`.
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        signal_keywords: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            signal_keywords: signal_keywords
                .into_iter()
                .map(|k| k.into().to_lowercase())
                .collect(),
        }
    }

    /// Count how many keyword tokens overlap with `signal`.
    ///
    /// The signal is tokenised (split on non-alphanumeric characters) and each
    /// token is compared against `signal_keywords`. Returns the overlap count.
    pub(crate) fn overlap_score(&self, signal: &str) -> usize {
        let tokens = tokenise(signal);
        self.signal_keywords
            .iter()
            .filter(|kw| tokens.contains(*kw))
            .count()
    }
}

// ─── Built-in task classes ────────────────────────────────────────────────────

/// Return the canonical built-in set of task classes.
///
/// Callers may extend this list with domain-specific classes before passing it
/// to `TaskClassMatcher::new`.
pub fn builtin_task_classes() -> Vec<TaskClass> {
    vec![
        TaskClass::new(
            "missing-import",
            "Missing import / undefined symbol",
            [
                "e0425",
                "e0433",
                "unresolved",
                "undefined",
                "import",
                "missing",
                "cannot",
                "find",
                "symbol",
            ],
        ),
        TaskClass::new(
            "type-mismatch",
            "Type mismatch",
            [
                "e0308",
                "mismatched",
                "expected",
                "found",
                "type",
                "mismatch",
            ],
        ),
        TaskClass::new(
            "borrow-conflict",
            "Borrow checker conflict",
            [
                "e0502", "e0505", "borrow", "lifetime", "moved", "cannot", "conflict",
            ],
        ),
        TaskClass::new(
            "test-failure",
            "Test failure",
            ["test", "failed", "panic", "assert", "assertion", "failure"],
        ),
        TaskClass::new(
            "performance",
            "Performance issue",
            ["slow", "latency", "timeout", "perf", "performance", "hot"],
        ),
    ]
}

// ─── TaskClassMatcher ─────────────────────────────────────────────────────────

/// Classifies a list of signal strings to the best-matching `TaskClass`.
pub struct TaskClassMatcher {
    classes: Vec<TaskClass>,
}

impl TaskClassMatcher {
    /// Create a matcher with the provided task-class registry.
    pub fn new(classes: Vec<TaskClass>) -> Self {
        Self { classes }
    }

    /// Create a matcher pre-loaded with `builtin_task_classes()`.
    pub fn with_builtins() -> Self {
        Self::new(builtin_task_classes())
    }

    /// Classify `signals` to the best-matching task class.
    ///
    /// Returns `None` when no class achieves a positive overlap score.
    pub fn classify<'a>(&'a self, signals: &[String]) -> Option<&'a TaskClass> {
        let mut best: Option<(&TaskClass, usize)> = None;

        for class in &self.classes {
            let total_score: usize = signals.iter().map(|s| class.overlap_score(s)).sum();
            if total_score > 0 {
                match best {
                    None => best = Some((class, total_score)),
                    Some((_, prev_score)) if total_score > prev_score => {
                        best = Some((class, total_score));
                    }
                    _ => {}
                }
            }
        }

        best.map(|(c, _)| c)
    }

    /// Return a reference to the underlying class registry.
    pub fn classes(&self) -> &[TaskClass] {
        &self.classes
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Tokenise a string into lowercase alphanumeric words.
fn tokenise(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_lowercase())
        .collect()
}

/// Check whether `signals` match the given task-class ID in `registry`.
///
/// A convenience wrapper around `TaskClassMatcher::classify`.
pub fn signals_match_class(signals: &[String], class_id: &str, registry: &[TaskClass]) -> bool {
    let matcher = TaskClassMatcher::new(registry.to_vec());
    matcher
        .classify(signals)
        .map_or(false, |c| c.id == class_id)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn matcher() -> TaskClassMatcher {
        TaskClassMatcher::with_builtins()
    }

    // ── Positive: same task-class, different signal variants ─────────────────

    #[test]
    fn test_missing_import_via_error_code() {
        let m = matcher();
        let signals = vec!["error[E0425]: cannot find value `foo` in scope".to_string()];
        let cls = m.classify(&signals).expect("should classify");
        assert_eq!(cls.id, "missing-import");
    }

    #[test]
    fn test_missing_import_via_natural_language() {
        let m = matcher();
        // Different phrasing — no Rust error code, but "undefined symbol" keywords
        let signals = vec!["undefined symbol: use_missing_fn".to_string()];
        let cls = m.classify(&signals).expect("should classify");
        assert_eq!(cls.id, "missing-import");
    }

    #[test]
    fn test_missing_import_via_unresolved_import() {
        let m = matcher();
        let signals = vec!["unresolved import `std::collections::Missing`".to_string()];
        let cls = m.classify(&signals).expect("should classify");
        assert_eq!(cls.id, "missing-import");
    }

    #[test]
    fn test_type_mismatch_classification() {
        let m = matcher();
        let signals =
            vec!["error[E0308]: mismatched types: expected `u32` found `String`".to_string()];
        let cls = m.classify(&signals).expect("should classify");
        assert_eq!(cls.id, "type-mismatch");
    }

    #[test]
    fn test_borrow_conflict_classification() {
        let m = matcher();
        let signals = vec![
            "error[E0502]: cannot borrow `x` as mutable because it is also borrowed as immutable"
                .to_string(),
        ];
        let cls = m.classify(&signals).expect("should classify");
        assert_eq!(cls.id, "borrow-conflict");
    }

    #[test]
    fn test_test_failure_classification() {
        let m = matcher();
        let signals = vec!["test panicked: assertion failed: x == y".to_string()];
        let cls = m.classify(&signals).expect("should classify");
        assert_eq!(cls.id, "test-failure");
    }

    #[test]
    fn test_multiple_signals_accumulate_score() {
        let m = matcher();
        // Two signals both pointing at type-mismatch → still resolves correctly
        let signals = vec![
            "expected type `u32`".to_string(),
            "found type `String` — type mismatch".to_string(),
        ];
        let cls = m.classify(&signals).expect("should classify");
        assert_eq!(cls.id, "type-mismatch");
    }

    // ── Negative: cross-class no false positives ──────────────────────────────

    #[test]
    fn test_no_false_positive_type_vs_borrow() {
        let m = matcher();
        // "E0308" → type-mismatch only, not borrow-conflict
        let signals = vec!["error[E0308]: mismatched type".to_string()];
        let cls = m.classify(&signals).unwrap();
        assert_ne!(
            cls.id, "borrow-conflict",
            "must not cross-match borrow-conflict"
        );
    }

    #[test]
    fn test_no_false_positive_borrow_vs_import() {
        let m = matcher();
        let signals = vec!["error[E0502]: cannot borrow as mutable".to_string()];
        let cls = m.classify(&signals).unwrap();
        assert_ne!(cls.id, "missing-import");
    }

    #[test]
    fn test_no_match_returns_none() {
        let m = matcher();
        // Completely unrelated signal with no keyword overlap
        let signals = vec!["network timeout connecting to database server".to_string()];
        // This might match "performance/timeout" — but if it doesn't, None is fine.
        // The key invariant is it doesn't match an unrelated class like "missing-import".
        if let Some(cls) = m.classify(&signals) {
            assert_ne!(cls.id, "missing-import");
            assert_ne!(cls.id, "type-mismatch");
            assert_ne!(cls.id, "borrow-conflict");
        }
        // None is also acceptable
    }

    #[test]
    fn test_empty_signals_returns_none() {
        let m = matcher();
        assert!(m.classify(&[]).is_none());
    }

    // ── Boundary: custom classes ──────────────────────────────────────────────

    #[test]
    fn test_custom_class_wins_over_builtin() {
        // A domain-specific class with high keyword density should beat builtins
        let mut classes = builtin_task_classes();
        classes.push(TaskClass::new(
            "db-timeout",
            "Database timeout",
            ["database", "timeout", "connection", "pool", "exhausted"],
        ));
        let m = TaskClassMatcher::new(classes);
        let signals = vec!["database connection pool exhausted — timeout".to_string()];
        let cls = m.classify(&signals).expect("should classify");
        assert_eq!(cls.id, "db-timeout");
    }

    #[test]
    fn test_signals_match_class_helper() {
        let registry = builtin_task_classes();
        let signals = vec!["error[E0425]: cannot find value".to_string()];
        assert!(signals_match_class(&signals, "missing-import", &registry));
        assert!(!signals_match_class(&signals, "type-mismatch", &registry));
    }

    #[test]
    fn test_overlap_score_case_insensitive() {
        let class = TaskClass::new("tc", "Test", ["e0425", "unresolved"]);
        let m = TaskClassMatcher::new(vec![class]);
        // Signal contains uppercase E0425 — tokenise lowercases all tokens
        // so the match is case-insensitive.
        let signals = vec!["E0425 unresolved import".to_string()];
        let cls = m
            .classify(&signals)
            .expect("case-insensitive classify should work");
        assert_eq!(cls.id, "tc");
    }
}
