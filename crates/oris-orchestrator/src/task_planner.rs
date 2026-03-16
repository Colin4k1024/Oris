//! Autonomous task planning layer — Issue #280 (Stream B).
//!
//! Converts a `DiscoveredIssue` from intake into a bounded, risk-assessed
//! `AutonomousTaskPlan`.  High-risk or low-feasibility candidates are denied
//! fail-closed before reaching proposal generation or PR delivery.
//!
//! # Pipeline position
//!
//! ```text
//! IssueDiscoveryPort  ──→  DiscoveredIssue
//!       ↓
//!  plan_autonomous_candidate()   ← this module
//!       ↓
//!  AutonomousTaskPlan { approved: true }
//!       ↓
//!  ProposalGeneratorPort  ──→  GeneratedProposal
//! ```
//!
//! Denied plans produce an `Err(AutonomousPlanReasonCode)` and the issue is
//! dropped before any external action is taken.

use serde::{Deserialize, Serialize};

use oris_evolution::{TaskClass, TaskClassMatcher};

use crate::autonomous_loop::DiscoveredIssue;

// ── Risk tier ──────────────────────────────────────────────────────────────

/// Risk level assigned to a candidate during task planning.
///
/// Used for blast-radius gating: candidates classified `High` or `Critical`
/// are denied fail-closed.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum RiskTier {
    /// Safe, well-understood change class.
    Low,
    /// Change class with moderate blast radius or uncertainty.
    Medium,
    /// Large or poorly-scoped change — denied by default.
    High,
    /// Cross-cutting or destructive change — always denied.
    Critical,
}

// ── BoundedTaskClass ───────────────────────────────────────────────────────

/// A task class augmented with execution-safety bounds.
///
/// Production routing uses the built-in `BoundedTaskClass` registry returned
/// by [`bounded_task_classes`].  The `id` field matches the corresponding
/// `TaskClass::id` in `oris-evolution`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BoundedTaskClass {
    /// Stable identifier; matches `TaskClass::id`.
    pub id: String,
    /// Human-readable label.
    pub name: String,
    /// Maximum number of files this class is permitted to touch.
    pub max_files: usize,
    /// Highest `RiskTier` that is allowed to proceed past planning.
    /// Candidates whose computed risk exceeds this ceiling are denied.
    pub risk_ceiling: RiskTier,
    /// Minimum feasibility score (0.0–1.0) required to proceed.
    pub min_feasibility: f64,
    /// The `RiskTier` statically associated with this class.
    pub default_risk: RiskTier,
}

/// Return the canonical bounded task-class registry.
///
/// Each entry corresponds to a `builtin_task_classes()` entry in
/// `oris-evolution` and adds the safety-bound fields required by the planner.
pub fn bounded_task_classes() -> Vec<BoundedTaskClass> {
    vec![
        BoundedTaskClass {
            id: "missing-import".to_string(),
            name: "Missing import / undefined symbol".to_string(),
            max_files: 4,
            risk_ceiling: RiskTier::Medium,
            min_feasibility: 0.3,
            default_risk: RiskTier::Low,
        },
        BoundedTaskClass {
            id: "type-mismatch".to_string(),
            name: "Type mismatch".to_string(),
            max_files: 4,
            risk_ceiling: RiskTier::Medium,
            min_feasibility: 0.3,
            default_risk: RiskTier::Low,
        },
        BoundedTaskClass {
            id: "borrow-conflict".to_string(),
            name: "Borrow checker conflict".to_string(),
            max_files: 6,
            risk_ceiling: RiskTier::Medium,
            min_feasibility: 0.25,
            default_risk: RiskTier::Medium,
        },
        BoundedTaskClass {
            id: "test-failure".to_string(),
            name: "Test failure".to_string(),
            max_files: 3,
            risk_ceiling: RiskTier::Medium,
            min_feasibility: 0.4,
            default_risk: RiskTier::Low,
        },
        BoundedTaskClass {
            id: "performance".to_string(),
            name: "Performance issue".to_string(),
            max_files: 8,
            risk_ceiling: RiskTier::Medium,
            min_feasibility: 0.2,
            default_risk: RiskTier::Medium,
        },
    ]
}

// ── Feasibility score ──────────────────────────────────────────────────────

/// A normalised feasibility estimate in the range `[0.0, 1.0]`.
///
/// A score of `0.0` means completely infeasible; `1.0` means highly
/// confident the mutation can be generated and validated successfully.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct FeasibilityScore(pub f64);

impl FeasibilityScore {
    /// Clamp to `[0.0, 1.0]`.
    pub fn new(raw: f64) -> Self {
        Self(raw.clamp(0.0, 1.0))
    }

    /// `true` when the score meets or exceeds `threshold`.
    pub fn meets(&self, threshold: f64) -> bool {
        self.0 >= threshold
    }
}

// ── Blast radius ───────────────────────────────────────────────────────────

/// Estimated scope of a mutation.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlastRadius {
    /// Estimated number of files touched by the mutation.
    pub estimated_files: usize,
    /// Depth of transitive dependency change (0 = single file, higher = wider).
    pub scope_depth: u8,
}

// ── Plan reason codes ──────────────────────────────────────────────────────

/// Reason code attached to an `AutonomousTaskPlan`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum AutonomousPlanReasonCode {
    /// Plan approved — proceed to proposal generation.
    Approved,
    /// Denied because the computed risk tier exceeds the class ceiling.
    DeniedHighRisk,
    /// Denied because the feasibility score is below the class minimum.
    DeniedLowFeasibility,
    /// Denied because the estimated blast radius exceeds the class limit.
    DeniedBlastRadiusExceeded,
    /// Denied because no task class matches the candidate signals.
    DeniedUnknownClass,
}

impl AutonomousPlanReasonCode {
    /// `true` when the plan was denied on any condition.
    pub fn is_denied(&self) -> bool {
        !matches!(self, Self::Approved)
    }
}

// ── AutonomousTaskPlan ─────────────────────────────────────────────────────

/// The output of `plan_autonomous_candidate`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AutonomousTaskPlan {
    /// Issue being planned.
    pub issue_id: String,
    /// Matched bounded task class, or `None` when the class is unknown.
    pub task_class_id: Option<String>,
    /// Computed feasibility score.
    pub feasibility: FeasibilityScore,
    /// Assigned risk tier.
    pub risk_tier: RiskTier,
    /// Estimated blast radius.
    pub blast_radius: BlastRadius,
    /// Structured reason for the planning decision.
    pub reason_code: AutonomousPlanReasonCode,
}

impl AutonomousTaskPlan {
    /// `true` when the plan is approved for proposal generation.
    pub fn is_approved(&self) -> bool {
        self.reason_code == AutonomousPlanReasonCode::Approved
    }
}

// ── Core planning function ─────────────────────────────────────────────────

/// Produce an `AutonomousTaskPlan` for a discovered candidate.
///
/// Steps:
/// 1. Classify signals against the `BoundedTaskClass` registry via
///    `TaskClassMatcher`.
/// 2. Compute a feasibility score from signal quality and class match strength.
/// 3. Assign a risk tier (class default, elevated by low feasibility or wide
///    blast radius).
/// 4. Estimate blast radius from signal count and class `max_files`.
/// 5. Apply deny conditions — fail-closed on any unmet bound.
pub fn plan_autonomous_candidate(
    issue: &DiscoveredIssue,
    bounded_classes: &[BoundedTaskClass],
) -> AutonomousTaskPlan {
    // Build evolution TaskClass list from bounded registry.
    let task_classes: Vec<TaskClass> = bounded_classes
        .iter()
        .map(|bc| {
            // We reconstruct TaskClass from the bounded entry.
            // The signal_keywords are embedded in the built-in registry;
            // we use a lightweight approach: create a TaskClass whose overlap
            // score is driven by the bounded-class id matched via the evolution
            // matcher.
            oris_evolution::TaskClass::new(
                bc.id.clone(),
                bc.name.clone(),
                // keywords are sourced from builtin_task_classes in oris-evolution
                // We pull them by matching the id.
                builtin_keywords_for(&bc.id),
            )
        })
        .collect();

    let matcher = TaskClassMatcher::new(task_classes);
    let matched_class = matcher.classify(&issue.signals);

    // If no class matched → DeniedUnknownClass.
    let Some(tc) = matched_class else {
        return AutonomousTaskPlan {
            issue_id: issue.issue_id.clone(),
            task_class_id: None,
            feasibility: FeasibilityScore::new(0.0),
            risk_tier: RiskTier::High,
            blast_radius: BlastRadius {
                estimated_files: 0,
                scope_depth: 0,
            },
            reason_code: AutonomousPlanReasonCode::DeniedUnknownClass,
        };
    };

    // Find the bounded class entry.
    let bounded = bounded_classes.iter().find(|bc| bc.id == tc.id).unwrap();

    // Compute feasibility from signal count and class characteristics.
    let feasibility = compute_feasibility(&issue.signals, tc, bounded);

    // Compute blast radius.
    let blast_radius = compute_blast_radius(&issue.signals, bounded);

    // Determine the effective risk tier.
    let risk_tier = compute_risk_tier(bounded, &feasibility, &blast_radius);

    // Apply deny conditions in priority order.
    let reason_code = if risk_tier > bounded.risk_ceiling {
        AutonomousPlanReasonCode::DeniedHighRisk
    } else if blast_radius.estimated_files > bounded.max_files {
        AutonomousPlanReasonCode::DeniedBlastRadiusExceeded
    } else if !feasibility.meets(bounded.min_feasibility) {
        AutonomousPlanReasonCode::DeniedLowFeasibility
    } else {
        AutonomousPlanReasonCode::Approved
    };

    AutonomousTaskPlan {
        issue_id: issue.issue_id.clone(),
        task_class_id: Some(tc.id.clone()),
        feasibility,
        risk_tier,
        blast_radius,
        reason_code,
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────

/// Compute normalised feasibility from signal quality.
///
/// Heuristic: ratio of overlapping signal tokens to the number of unique
/// class keywords, capped at 1.0.  More overlapping tokens → higher score.
fn compute_feasibility(
    signals: &[String],
    tc: &TaskClass,
    _bounded: &BoundedTaskClass,
) -> FeasibilityScore {
    let keywords = &tc.signal_keywords;
    if keywords.is_empty() {
        return FeasibilityScore::new(0.0);
    }

    // Count distinct matched keywords across all signals.
    let matched: usize = keywords
        .iter()
        .filter(|kw| {
            signals
                .iter()
                .any(|s| s.to_lowercase().contains(kw.as_str()))
        })
        .count();

    let ratio = matched as f64 / keywords.len() as f64;
    // Apply a mild dampening so a single keyword match doesn't score 1.0 for
    // a class with many keywords — minimum signal: at least 2 matched signals.
    let signal_bonus = if signals.len() >= 2 { 1.0 } else { 0.7 };
    FeasibilityScore::new(ratio * signal_bonus)
}

/// Estimate blast radius from signal count and class bounds.
///
/// Heuristic: use signal count as a rough proxy for scope.  Wider signal
/// sets imply more code areas, up to the class `max_files`.
fn compute_blast_radius(signals: &[String], _bounded: &BoundedTaskClass) -> BlastRadius {
    // Do NOT cap at max_files here — the caller checks estimated_files > max_files.
    let estimated_files = signals.len() / 2 + 1;
    let scope_depth: u8 = if signals.len() > 4 { 2 } else { 1 };
    BlastRadius {
        estimated_files,
        scope_depth,
    }
}

/// Assign risk tier, elevating when blast radius is wide or feasibility is low.
fn compute_risk_tier(
    bounded: &BoundedTaskClass,
    feasibility: &FeasibilityScore,
    blast: &BlastRadius,
) -> RiskTier {
    let mut tier = bounded.default_risk.clone();

    // Elevate to Medium when blast radius hits the class limit.
    if blast.estimated_files >= bounded.max_files && tier < RiskTier::Medium {
        tier = RiskTier::Medium;
    }
    // Elevate to High when feasibility is very low.
    if feasibility.0 < 0.15 && tier < RiskTier::High {
        tier = RiskTier::High;
    }

    tier
}

/// Return the signal keywords for the given task-class id from the built-in
/// registry.  Falls back to an empty list for unknown ids.
fn builtin_keywords_for(id: &str) -> Vec<String> {
    match id {
        "missing-import" => vec![
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
        "type-mismatch" => vec![
            "e0308",
            "mismatched",
            "expected",
            "found",
            "type",
            "mismatch",
        ],
        "borrow-conflict" => {
            vec![
                "e0502", "e0505", "borrow", "lifetime", "moved", "cannot", "conflict",
            ]
        }
        "test-failure" => vec!["test", "failed", "panic", "assert", "assertion", "failure"],
        "performance" => vec!["slow", "latency", "timeout", "perf", "performance", "hot"],
        _ => vec![],
    }
    .into_iter()
    .map(String::from)
    .collect()
}

// ── Real ClassifierPort implementation ────────────────────────────────────

/// A `ClassifierPort` backed by `BoundedTaskClass` routing via
/// `TaskClassMatcher`.
///
/// This replaces the `FixedClassifier` test stub with a production
/// implementation that actually classifies signals against the bounded
/// task-class registry.
pub struct BoundedClassifier {
    matcher: TaskClassMatcher,
}

impl BoundedClassifier {
    /// Construct with the provided task classes.
    pub fn new(classes: Vec<TaskClass>) -> Self {
        Self {
            matcher: TaskClassMatcher::new(classes),
        }
    }

    /// Construct pre-loaded with `bounded_task_classes()` mapped to
    /// `TaskClass` entries.
    pub fn with_builtins() -> Self {
        let classes: Vec<TaskClass> = bounded_task_classes()
            .iter()
            .map(|bc| TaskClass::new(bc.id.clone(), bc.name.clone(), builtin_keywords_for(&bc.id)))
            .collect();
        Self::new(classes)
    }
}

impl crate::pipeline_orchestrator::ClassifierPort for BoundedClassifier {
    fn classify(&self, signals: &[String]) -> Option<String> {
        self.matcher.classify(signals).map(|tc| tc.id.clone())
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autonomous_loop::DiscoveredIssue;

    fn issue(id: &str, signals: Vec<&str>) -> DiscoveredIssue {
        DiscoveredIssue {
            issue_id: id.to_string(),
            title: format!("Test issue {id}"),
            signals: signals.into_iter().map(String::from).collect(),
        }
    }

    fn classes() -> Vec<BoundedTaskClass> {
        bounded_task_classes()
    }

    // ── task_planning_known_class_approved ─────────────────────────────────

    #[test]
    fn task_planning_known_class_approved() {
        let i = issue(
            "i1",
            vec!["error[E0425]: cannot find value `x`", "import missing"],
        );
        let plan = plan_autonomous_candidate(&i, &classes());
        assert_eq!(plan.task_class_id.as_deref(), Some("missing-import"));
        assert!(
            plan.is_approved(),
            "expected approved, got {:?}",
            plan.reason_code
        );
    }

    // ── task_planning_unknown_class_denied ─────────────────────────────────

    #[test]
    fn task_planning_unknown_class_denied() {
        let i = issue("i2", vec!["completely unrelated signal xyz abc"]);
        let plan = plan_autonomous_candidate(&i, &classes());
        assert_eq!(
            plan.reason_code,
            AutonomousPlanReasonCode::DeniedUnknownClass
        );
        assert!(!plan.is_approved());
    }

    // ── task_planning_denied_high_risk ────────────────────────────────────

    #[test]
    fn task_planning_denied_high_risk() {
        // Very low feasibility signals trigger High risk tier, which exceeds
        // the Medium ceiling of all built-in classes.
        // We create a class with a Low risk ceiling and High default risk so
        // the condition fires deterministically.
        let narrow_class = BoundedTaskClass {
            id: "test-failure".to_string(),
            name: "Test failure".to_string(),
            max_files: 3,
            risk_ceiling: RiskTier::Low, // ceiling is Low
            min_feasibility: 0.05,
            default_risk: RiskTier::High, // default already High → exceeds Low ceiling
        };
        let i = issue("i3", vec!["test failed", "panic"]);
        let plan = plan_autonomous_candidate(&i, &[narrow_class]);
        assert_eq!(
            plan.reason_code,
            AutonomousPlanReasonCode::DeniedHighRisk,
            "got {:?}",
            plan.reason_code
        );
    }

    // ── task_planning_high_risk_denied_before_blast_radius ─────────────────

    #[test]
    fn task_planning_high_risk_takes_priority_over_blast_radius() {
        let narrow = BoundedTaskClass {
            id: "borrow-conflict".to_string(),
            name: "Borrow checker conflict".to_string(),
            max_files: 1,                // very tight blast-radius limit
            risk_ceiling: RiskTier::Low, // ceiling is Low
            min_feasibility: 0.05,
            default_risk: RiskTier::High, // default already exceeds ceiling
        };
        let i = issue(
            "i4",
            vec![
                "error[E0502]: cannot borrow",
                "borrow conflict",
                "lifetime issue",
            ],
        );
        let plan = plan_autonomous_candidate(&i, &[narrow]);
        // High risk (> Low ceiling) takes priority.
        assert_eq!(plan.reason_code, AutonomousPlanReasonCode::DeniedHighRisk);
    }

    // ── task_planning_blast_radius_exceeded ────────────────────────────────

    #[test]
    fn task_planning_blast_radius_exceeded() {
        // Build a class where default risk ≤ ceiling but max_files is exceeded.
        // We pass many signals to push estimated_files over max_files.
        let tight_class = BoundedTaskClass {
            id: "test-failure".to_string(),
            name: "Test failure".to_string(),
            max_files: 1,                 // very tight
            risk_ceiling: RiskTier::High, // ceiling is High — risk won't fire
            min_feasibility: 0.0,
            default_risk: RiskTier::Low, // won't be elevated to High by feasibility alone
        };
        // 6 signals → estimated_files = 6/2+1 = 4 > max_files=1
        let i = issue(
            "i5",
            vec![
                "test failed",
                "panic a",
                "panic b",
                "panic c",
                "assertion d",
                "failure e",
            ],
        );
        let plan = plan_autonomous_candidate(&i, &[tight_class]);
        assert_eq!(
            plan.reason_code,
            AutonomousPlanReasonCode::DeniedBlastRadiusExceeded
        );
    }

    // ── task_planning_test_failure_class ──────────────────────────────────

    #[test]
    fn task_planning_test_failure_class_routes_correctly() {
        let i = issue("i6", vec!["test failed: assertion `left == right`"]);
        let plan = plan_autonomous_candidate(&i, &classes());
        assert_eq!(plan.task_class_id.as_deref(), Some("test-failure"));
    }

    // ── task_planning_bounded_classifier_port ────────────────────────────

    #[test]
    fn task_planning_bounded_classifier_port_classifies() {
        use crate::pipeline_orchestrator::ClassifierPort;
        let clf = BoundedClassifier::with_builtins();
        let signals = vec!["error[E0308]: mismatched types".to_string()];
        let result = clf.classify(&signals);
        assert_eq!(result.as_deref(), Some("type-mismatch"));
    }

    #[test]
    fn task_planning_bounded_classifier_returns_none_for_unknown() {
        use crate::pipeline_orchestrator::ClassifierPort;
        let clf = BoundedClassifier::with_builtins();
        let signals = vec!["completely unrelated xyz999".to_string()];
        assert!(clf.classify(&signals).is_none());
    }
}
