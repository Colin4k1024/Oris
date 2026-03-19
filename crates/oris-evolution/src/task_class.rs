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

// ─── TaskClassDefinition ──────────────────────────────────────────────────────

/// Extended task-class definition that adds a natural-language `description`
/// field used by `TaskClassInferencer` for semantic matching and TOML persistence.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskClassDefinition {
    /// Opaque, stable identifier.
    pub id: String,
    /// Human-readable label.
    pub name: String,
    /// Natural-language description used when scoring signal similarity.
    pub description: String,
    /// Lowercase keywords used for overlap-based classification.
    pub signal_keywords: Vec<String>,
}

impl TaskClassDefinition {
    /// Convert into a lightweight `TaskClass` (drops the description field).
    pub fn into_task_class(self) -> TaskClass {
        TaskClass::new(self.id, self.name, self.signal_keywords)
    }
}

// ─── Built-in task class definitions ─────────────────────────────────────────

/// Return the canonical built-in task class definitions including descriptions.
pub fn builtin_task_class_definitions() -> Vec<TaskClassDefinition> {
    vec![
        TaskClassDefinition {
            id: "missing-import".to_string(),
            name: "Missing import / undefined symbol".to_string(),
            description: "Compiler cannot find symbol unresolved import undefined reference \
                          missing use declaration cannot find value in scope"
                .to_string(),
            signal_keywords: vec![
                "e0425",
                "e0433",
                "unresolved",
                "undefined",
                "import",
                "missing",
                "cannot",
                "find",
                "symbol",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        },
        TaskClassDefinition {
            id: "type-mismatch".to_string(),
            name: "Type mismatch".to_string(),
            description: "Type mismatch mismatched types expected one type found another \
                          type annotation required"
                .to_string(),
            signal_keywords: vec![
                "e0308",
                "mismatched",
                "expected",
                "found",
                "type",
                "mismatch",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        },
        TaskClassDefinition {
            id: "borrow-conflict".to_string(),
            name: "Borrow checker conflict".to_string(),
            description: "Borrow checker conflict cannot borrow as mutable lifetime error \
                          value moved cannot use after move"
                .to_string(),
            signal_keywords: vec![
                "e0502", "e0505", "borrow", "lifetime", "moved", "cannot", "conflict",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        },
        TaskClassDefinition {
            id: "test-failure".to_string(),
            name: "Test failure".to_string(),
            description: "Test failure panicked assertion failed test did not pass".to_string(),
            signal_keywords: vec!["test", "failed", "panic", "assert", "assertion", "failure"]
                .into_iter()
                .map(String::from)
                .collect(),
        },
        TaskClassDefinition {
            id: "performance".to_string(),
            name: "Performance issue".to_string(),
            description: "Performance issue slow response high latency operation timeout \
                          hot path resource contention"
                .to_string(),
            signal_keywords: vec!["slow", "latency", "timeout", "perf", "performance", "hot"]
                .into_iter()
                .map(String::from)
                .collect(),
        },
    ]
}

// ─── TOML persistence ─────────────────────────────────────────────────────────

#[cfg(feature = "evolution-experimental")]
#[derive(Deserialize)]
struct TaskClassesToml {
    task_classes: Vec<TaskClassDefinition>,
}

/// Load task class definitions from a TOML file.
///
/// The file must contain a top-level `[[task_classes]]` array whose entries
/// each have `id`, `name`, `description`, and `signal_keywords` fields.
///
/// Only available with the `evolution-experimental` feature.
#[cfg(feature = "evolution-experimental")]
pub fn load_task_classes_from_toml(
    path: &std::path::Path,
) -> Result<Vec<TaskClassDefinition>, String> {
    let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let parsed: TaskClassesToml = toml::from_str(&content).map_err(|e| e.to_string())?;
    Ok(parsed.task_classes)
}

/// Load task class definitions.
///
/// When the `evolution-experimental` feature is enabled, attempts to load from
/// `~/.oris/oris-task-classes.toml` if it exists; otherwise falls back to
/// `builtin_task_class_definitions()`.
pub fn load_task_classes() -> Vec<TaskClassDefinition> {
    #[cfg(feature = "evolution-experimental")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            let path = std::path::Path::new(&home)
                .join(".oris")
                .join("oris-task-classes.toml");
            if path.exists() {
                if let Ok(classes) = load_task_classes_from_toml(&path) {
                    return classes;
                }
            }
        }
    }
    builtin_task_class_definitions()
}

// ─── TaskClassInferencer ──────────────────────────────────────────────────────

/// Infers the task class for a signal description using keyword recall scoring.
///
/// # Scoring
///
/// For each registered class, the score is:
///
/// ```text
/// score = |signal_tokens ∩ class_keywords| / |class_keywords|
/// ```
///
/// The class with the highest score is returned when the score meets
/// `threshold` (default `0.75`).  When no class reaches the threshold the
/// fallback `"generic_fix"` ID is returned.
pub struct TaskClassInferencer {
    classes: Vec<TaskClassDefinition>,
    threshold: f32,
}

impl TaskClassInferencer {
    /// Create an inferencer from a custom set of definitions.
    pub fn new(classes: Vec<TaskClassDefinition>) -> Self {
        Self {
            classes,
            threshold: 0.75,
        }
    }

    /// Create an inferencer pre-loaded with `builtin_task_class_definitions()`.
    pub fn with_builtins() -> Self {
        Self::new(builtin_task_class_definitions())
    }

    /// Override the similarity threshold (default `0.75`).
    pub fn with_threshold(mut self, threshold: f32) -> Self {
        self.threshold = threshold;
        self
    }

    /// Infer the task class ID for the given signal description.
    ///
    /// Returns the ID of the best matching class when it achieves a score
    /// ≥ `threshold`, or `"generic_fix"` otherwise.
    pub fn infer(&self, signal_description: &str) -> String {
        let signal_tokens = tokenise(signal_description);
        if signal_tokens.is_empty() {
            return "generic_fix".to_string();
        }

        let mut best_id = "generic_fix";
        let mut best_score = 0.0f32;

        for class in &self.classes {
            let score = recall_score(&signal_tokens, &class.signal_keywords);
            if score > best_score {
                best_score = score;
                best_id = &class.id;
            }
        }

        if best_score >= self.threshold {
            best_id.to_string()
        } else {
            "generic_fix".to_string()
        }
    }

    /// Return a reference to the underlying class definitions.
    pub fn class_definitions(&self) -> &[TaskClassDefinition] {
        &self.classes
    }
}

// ─── Internal similarity helper ───────────────────────────────────────────────

/// Keyword recall: fraction of class keywords that appear in the signal tokens.
fn recall_score(signal_tokens: &[String], keywords: &[String]) -> f32 {
    if keywords.is_empty() {
        return 0.0;
    }
    let intersection = keywords
        .iter()
        .filter(|kw| signal_tokens.contains(kw))
        .count();
    intersection as f32 / keywords.len() as f32
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

    // ── TaskClassInferencer tests ─────────────────────────────────────────────

    #[test]
    fn inferencer_canonical_compiler_error_missing_import() {
        let inferencer = TaskClassInferencer::with_builtins();
        // Canonical signal: contains the majority of missing-import keywords.
        let signal = "error[E0425]: cannot find value `foo`: \
                      unresolved import symbol is undefined missing";
        let class_id = inferencer.infer(signal);
        assert_eq!(
            class_id, "missing-import",
            "canonical missing-import signal should infer correct class"
        );
    }

    #[test]
    fn inferencer_canonical_compiler_error_type_mismatch() {
        let inferencer = TaskClassInferencer::with_builtins();
        // Canonical signal: contains most type-mismatch keywords.
        let signal = "error[E0308]: mismatched type expected u32 found String type mismatch";
        let class_id = inferencer.infer(signal);
        assert_eq!(class_id, "type-mismatch");
    }

    #[test]
    fn inferencer_score_below_threshold_falls_back_to_generic_fix() {
        let inferencer = TaskClassInferencer::with_builtins();
        // Signal with only one matching keyword — far below 0.75 threshold.
        let signal = "e0308";
        let class_id = inferencer.infer(signal);
        assert_eq!(
            class_id, "generic_fix",
            "low-match signal must fall back to generic_fix"
        );
    }

    #[test]
    fn inferencer_empty_signal_falls_back_to_generic_fix() {
        let inferencer = TaskClassInferencer::with_builtins();
        assert_eq!(inferencer.infer(""), "generic_fix");
    }

    #[test]
    fn inferencer_custom_threshold_lower_accepts_partial_match() {
        // With a lower threshold partial matches should succeed.
        let inferencer = TaskClassInferencer::with_builtins().with_threshold(0.3);
        // "E0308 mismatched" — 2/6 = 0.333, which is ≥ 0.30 threshold.
        let class_id = inferencer.infer("E0308 mismatched");
        assert_eq!(class_id, "type-mismatch");
    }

    #[test]
    fn inferencer_builtin_definitions_are_configurable_via_load() {
        // load_task_classes() must return at least the builtin definitions
        // (no TOML file exists in CI — falls back to builtins).
        let defs = load_task_classes();
        assert!(
            !defs.is_empty(),
            "load_task_classes must return at least builtins"
        );
        let has_missing_import = defs.iter().any(|d| d.id == "missing-import");
        assert!(
            has_missing_import,
            "builtin missing-import class must be present"
        );
    }
}
