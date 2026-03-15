//! Integration tests for Issue #245: CI/CD webhook intake
//!
//! Covers the acceptance criteria:
//! - GitHub check_run CI failure events flow into Intake (AC-1)
//! - Deduplication hit rate ≥ 95% (AC-2)
//! - Priority classification labels match expectations (AC-3)
//! - webhook → intake → signal chain (AC-4)

use oris_intake::IntakeSourceType;
use oris_intake::{
    Deduplicator, GithubIntakeSource, IntakeEvent, IntakeSource, IssueSeverity, PriorityEvaluator,
    SignalExtractor, SignalType,
};

// ---------------------------------------------------------------------------
// AC-1 & AC-4: GitHub check_run webhook → IntakeEvent → ExtractedSignal chain
// ---------------------------------------------------------------------------

/// Canonical check_run failure JSON as GitHub sends it.
fn check_run_failure_json() -> &'static str {
    r#"{
      "action": "completed",
      "check_run": {
        "id": 98765,
        "name": "cargo test",
        "head_sha": "deadbeef",
        "status": "completed",
        "conclusion": "failure",
        "html_url": "https://github.com/owner/repo/runs/98765",
        "output": {
          "title": "5 tests failed",
          "summary": "error[E0425]: cannot find value `foo` in this scope\ntest integration_test ... FAILED"
        }
      },
      "repository": {
        "full_name": "owner/repo",
        "html_url": "https://github.com/owner/repo"
      }
    }"#
}

#[test]
fn test_github_check_run_webhook_full_chain() {
    // Step 1: webhook bytes → IntakeEvent via GithubIntakeSource
    let source = GithubIntakeSource::new("check_run");
    let payload = check_run_failure_json().as_bytes();

    source
        .validate(payload)
        .expect("payload must be valid JSON");
    let events = source
        .process(payload)
        .expect("check_run must parse without error");

    assert_eq!(events.len(), 1, "one check_run event expected");
    let event = &events[0];

    // Title must mention check_run and conclusion
    assert!(
        event.title.contains("check_run"),
        "title must contain 'check_run'"
    );
    assert!(
        event.title.contains("failure"),
        "title must contain 'failure'"
    );

    // Severity of a failed check_run must be High
    assert_eq!(
        event.severity,
        IssueSeverity::High,
        "failed check_run must be High severity"
    );

    // Signals must contain the conclusion and commit sha
    assert!(
        event
            .signals
            .iter()
            .any(|s| s.contains("check_run_conclusion:failure")),
        "signals must carry check_run_conclusion:failure"
    );
    assert!(
        event
            .signals
            .iter()
            .any(|s| s.contains("commit_sha:deadbeef")),
        "signals must carry commit sha"
    );
    assert!(
        event.signals.iter().any(|s| s.contains("5 tests failed")),
        "signals must include output title"
    );

    // Step 2: IntakeEvent → ExtractedSignal via SignalExtractor
    let extractor = SignalExtractor::new(0.0); // accept all confidence levels
    let signals = extractor.extract(event);

    // The description contains "error[E0425]" → compiler_error + test_failure patterns
    assert!(!signals.is_empty(), "at least one signal must be extracted");
    let has_compiler = signals
        .iter()
        .any(|s| matches!(s.signal_type, SignalType::CompilerError));
    let has_test_failure = signals
        .iter()
        .any(|s| matches!(s.signal_type, SignalType::TestFailure));
    assert!(
        has_compiler || has_test_failure,
        "must extract compiler error or test failure signal"
    );
}

/// Auto-dispatch: when event_type is not provided, source detects check_run from payload shape.
#[test]
fn test_github_intake_source_auto_dispatch_check_run() {
    let source = GithubIntakeSource::auto();
    let events = source
        .process(check_run_failure_json().as_bytes())
        .expect("auto dispatch must succeed");
    assert_eq!(events.len(), 1);
    assert!(events[0].title.contains("check_run"));
}

/// workflow_run failure must still produce a High-severity event via the same source.
#[test]
fn test_github_intake_source_workflow_run_failure() {
    let payload = r#"{
      "action": "completed",
      "workflow": "ci.yml",
      "run_id": 12345,
      "repository": { "full_name": "owner/repo", "html_url": "https://github.com/owner/repo" },
      "workflow_run": {
        "head_branch": "main",
        "head_sha": "abc123",
        "html_url": "https://github.com/owner/repo/actions/runs/12345",
        "logs_url": "https://api.github.com/repos/owner/repo/actions/runs/12345/logs",
        "artifacts_url": "https://api.github.com/repos/owner/repo/actions/runs/12345/artifacts"
      },
      "conclusion": "failure"
    }"#;

    let source = GithubIntakeSource::new("workflow_run");
    let events = source
        .process(payload.as_bytes())
        .expect("workflow_run must parse");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].severity, IssueSeverity::High);
    assert_eq!(events[0].source_type, IntakeSourceType::Github);
}

// ---------------------------------------------------------------------------
// AC-2: Deduplication hit rate ≥ 95%
// ---------------------------------------------------------------------------

fn make_event(id: &str, source_event_id: &str) -> IntakeEvent {
    IntakeEvent {
        event_id: id.to_string(),
        source_type: IntakeSourceType::Github,
        source_event_id: Some(source_event_id.to_string()),
        title: "Build failed".to_string(),
        description: "Same root cause".to_string(),
        severity: IssueSeverity::High,
        signals: vec![],
        raw_payload: None,
        timestamp_ms: chrono::Utc::now().timestamp_millis(),
    }
}

/// Send the same event 20 times. First occurrence passes, remaining 19 are
/// caught as duplicates. Dedup hit rate = 19/20 = 95% — exactly meets the AC.
#[test]
fn test_deduplication_hit_rate_meets_95_percent() {
    let dedup = Deduplicator::new(24, 10_000);
    let total = 20usize;
    let mut duplicates = 0usize;

    for i in 0..total {
        let ev = make_event(&format!("evt-{}", i), "run-same-root-cause");
        if dedup.is_duplicate(&ev) {
            duplicates += 1;
        }
    }

    let hit_rate = duplicates as f64 / (total - 1) as f64; // hit / (total - first)
    assert!(
        hit_rate >= 0.95,
        "dedup hit rate must be ≥ 95%, got {:.1}%",
        hit_rate * 100.0
    );
    assert_eq!(
        duplicates,
        total - 1,
        "all events after the first must be caught as duplicates"
    );
}

/// Different source_event_ids produce distinct keys — not deduplicated.
#[test]
fn test_deduplication_different_root_causes_not_merged() {
    let dedup = Deduplicator::new(24, 10_000);
    for i in 0..10 {
        let ev = make_event(&format!("evt-{}", i), &format!("run-{}", i));
        assert!(
            !dedup.is_duplicate(&ev),
            "events with distinct source IDs must NOT be deduplicated"
        );
    }
}

// ---------------------------------------------------------------------------
// AC-3: Priority classification labels match expectations
// ---------------------------------------------------------------------------

fn event_with_severity(severity: IssueSeverity) -> IntakeEvent {
    IntakeEvent {
        event_id: uuid_str(),
        source_type: IntakeSourceType::Github,
        source_event_id: None,
        title: "test event".to_string(),
        description: "test".to_string(),
        severity,
        signals: vec![],
        raw_payload: None,
        timestamp_ms: chrono::Utc::now().timestamp_millis(),
    }
}

fn uuid_str() -> String {
    // Minimal deterministic ID for test events
    use std::sync::atomic::{AtomicU64, Ordering};
    static CTR: AtomicU64 = AtomicU64::new(0);
    format!("test-{}", CTR.fetch_add(1, Ordering::Relaxed))
}

#[test]
fn test_priority_classification_critical_is_highest() {
    let evaluator = PriorityEvaluator::default();
    let critical = evaluator.evaluate(&event_with_severity(IssueSeverity::Critical), &[]);
    let high = evaluator.evaluate(&event_with_severity(IssueSeverity::High), &[]);
    let medium = evaluator.evaluate(&event_with_severity(IssueSeverity::Medium), &[]);
    let low = evaluator.evaluate(&event_with_severity(IssueSeverity::Low), &[]);
    let info = evaluator.evaluate(&event_with_severity(IssueSeverity::Info), &[]);

    // Labels match ordering: critical > high > medium > low > info
    assert!(
        critical > high,
        "Critical ({}) must score higher than High ({})",
        critical,
        high
    );
    assert!(
        high > medium,
        "High ({}) must score higher than Medium ({})",
        high,
        medium
    );
    assert!(
        medium > low,
        "Medium ({}) must score higher than Low ({})",
        medium,
        low
    );
    assert!(
        low > info,
        "Low ({}) must score higher than Info ({})",
        low,
        info
    );
}

#[test]
fn test_priority_classification_critical_above_75() {
    let evaluator = PriorityEvaluator::default();
    let score = evaluator.evaluate(&event_with_severity(IssueSeverity::Critical), &[]);
    assert!(
        score >= 75,
        "Critical severity must yield priority ≥ 75, got {}",
        score
    );
}

#[test]
fn test_priority_classification_info_below_critical() {
    let evaluator = PriorityEvaluator::default();
    let info_score = evaluator.evaluate(&event_with_severity(IssueSeverity::Info), &[]);
    let critical_score = evaluator.evaluate(&event_with_severity(IssueSeverity::Critical), &[]);
    assert!(
        info_score < critical_score,
        "Info priority ({}) must be lower than Critical priority ({})",
        info_score,
        critical_score
    );
    // Info must score well below the critical threshold (critical is ≥ 75)
    assert!(
        info_score < 75,
        "Info severity must yield priority < 75 (Critical threshold), got {}",
        info_score
    );
}
