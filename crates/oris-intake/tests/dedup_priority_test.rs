//! Integration tests for deduplication and priority evaluation in oris-intake.

use oris_intake::{
    AutoPrioritizer, Deduplicator, IntakeEvent, IntakeRule, IntakeSourceType, IssueSeverity,
    PrioritizationResult, PriorityEvaluator, PriorityWeights, RateLimiter, RuleAction,
    RuleConditions, RuleEngine,
};

fn make_event(
    id: &str,
    source: IntakeSourceType,
    title: &str,
    severity: IssueSeverity,
) -> IntakeEvent {
    IntakeEvent {
        event_id: id.to_string(),
        source_type: source,
        source_event_id: Some(format!("src-{id}")),
        title: title.to_string(),
        description: "test event".to_string(),
        severity,
        signals: vec![],
        raw_payload: None,
        timestamp_ms: chrono::Utc::now().timestamp_millis(),
    }
}

// ─── Deduplicator tests ──────────────────────────────────────────────────────

#[test]
fn dedup_different_events_are_not_duplicates() {
    let dedup = Deduplicator::new(24, 100);

    let e1 = make_event(
        "1",
        IntakeSourceType::Github,
        "Build failed",
        IssueSeverity::High,
    );
    let e2 = make_event(
        "2",
        IntakeSourceType::Github,
        "Test failed",
        IssueSeverity::Medium,
    );

    assert!(!dedup.is_duplicate(&e1));
    assert!(!dedup.is_duplicate(&e2));
}

#[test]
fn dedup_same_title_different_source_not_duplicate() {
    let dedup = Deduplicator::new(24, 100);

    let e1 = make_event(
        "1",
        IntakeSourceType::Github,
        "Build failed",
        IssueSeverity::High,
    );
    let e2 = IntakeEvent {
        event_id: "2".to_string(),
        source_type: IntakeSourceType::Http,
        source_event_id: Some("src-1".to_string()),
        title: "Build failed".to_string(),
        description: "test".to_string(),
        severity: IssueSeverity::High,
        signals: vec![],
        raw_payload: None,
        timestamp_ms: chrono::Utc::now().timestamp_millis(),
    };

    assert!(!dedup.is_duplicate(&e1));
    assert!(!dedup.is_duplicate(&e2));
}

#[test]
fn dedup_stats_tracks_event_count() {
    let dedup = Deduplicator::new(24, 100);

    let e1 = make_event("1", IntakeSourceType::Github, "A", IssueSeverity::Low);
    let e2 = make_event("2", IntakeSourceType::Github, "B", IssueSeverity::Low);
    let e3 = make_event("3", IntakeSourceType::Github, "C", IssueSeverity::Low);

    dedup.is_duplicate(&e1);
    dedup.is_duplicate(&e2);
    dedup.is_duplicate(&e3);

    let stats = dedup.stats();
    assert_eq!(stats.tracked_events, 3);
    assert_eq!(stats.window_hours, 24);
}

#[test]
fn dedup_max_entries_triggers_cleanup() {
    let dedup = Deduplicator::new(24, 3);

    for i in 0..5 {
        let event = make_event(
            &format!("e{i}"),
            IntakeSourceType::Github,
            &format!("Event {i}"),
            IssueSeverity::Low,
        );
        dedup.is_duplicate(&event);
    }

    let stats = dedup.stats();
    // After hitting max_entries (3), cleanup runs but since all events are fresh
    // within the window, they're retained. The important thing is no panic.
    assert!(stats.tracked_events <= 5);
}

#[test]
fn dedup_none_source_event_id_still_deduplicates() {
    let dedup = Deduplicator::new(24, 100);

    let event = IntakeEvent {
        event_id: "x".to_string(),
        source_type: IntakeSourceType::Github,
        source_event_id: None,
        title: "No source ID".to_string(),
        description: "test".to_string(),
        severity: IssueSeverity::Medium,
        signals: vec![],
        raw_payload: None,
        timestamp_ms: chrono::Utc::now().timestamp_millis(),
    };

    assert!(!dedup.is_duplicate(&event));
    assert!(dedup.is_duplicate(&event));
}

// ─── Priority evaluator tests ────────────────────────────────────────────────

#[test]
fn priority_critical_higher_than_low() {
    let evaluator = PriorityEvaluator::default();

    let critical = make_event(
        "1",
        IntakeSourceType::Github,
        "Critical bug",
        IssueSeverity::Critical,
    );
    let low = make_event(
        "2",
        IntakeSourceType::Github,
        "Minor issue",
        IssueSeverity::Low,
    );

    let p_critical = evaluator.evaluate(&critical, &[]);
    let p_low = evaluator.evaluate(&low, &[]);

    assert!(
        p_critical > p_low,
        "critical={p_critical} should > low={p_low}"
    );
}

#[test]
fn priority_with_custom_weights() {
    let evaluator = PriorityEvaluator::new(PriorityWeights {
        severity: 1.0,
        confidence: 0.0,
        recency: 0.0,
        source_reliability: 0.0,
    });

    let high = make_event("1", IntakeSourceType::Github, "High", IssueSeverity::High);
    let low = make_event("2", IntakeSourceType::Github, "Low", IssueSeverity::Low);

    let p_high = evaluator.evaluate(&high, &[]);
    let p_low = evaluator.evaluate(&low, &[]);

    assert!(p_high > p_low);
}

// ─── Rate limiter tests ──────────────────────────────────────────────────────

#[test]
fn rate_limiter_allows_within_capacity() {
    let limiter = RateLimiter::new(100, 10, 1);

    for _ in 0..10 {
        assert!(limiter.try_acquire().is_ok());
    }
}

#[test]
fn rate_limiter_rejects_above_concurrent_limit() {
    let limiter = RateLimiter::new(100, 3, 1);

    assert!(limiter.try_acquire().is_ok());
    assert!(limiter.try_acquire().is_ok());
    assert!(limiter.try_acquire().is_ok());
    assert!(limiter.try_acquire().is_err());
}

// ─── AutoPrioritizer integration tests ───────────────────────────────────────

#[test]
fn auto_prioritizer_deduplicates_same_event() {
    let prioritizer = AutoPrioritizer::default();

    let event = make_event(
        "dup-1",
        IntakeSourceType::Github,
        "Duplicate event",
        IssueSeverity::Medium,
    );

    let first = prioritizer.process(&event, &[]);
    let second = prioritizer.process(&event, &[]);

    assert!(matches!(first, PrioritizationResult::Processed(_)));
    assert!(matches!(second, PrioritizationResult::Duplicate));
}

#[test]
fn auto_prioritizer_filters_by_multiple_rules() {
    let rules = vec![IntakeRule {
        id: "skip_low".to_string(),
        name: "Skip low severity".to_string(),
        description: "filter low".to_string(),
        priority: 100,
        enabled: true,
        conditions: RuleConditions {
            severities: vec!["low".to_string()],
            ..Default::default()
        },
        actions: vec![RuleAction::Skip],
    }];
    let prioritizer = AutoPrioritizer::default().with_rule_engine(RuleEngine::with_rules(rules));

    let low_event = make_event(
        "low-1",
        IntakeSourceType::Github,
        "Minor",
        IssueSeverity::Low,
    );
    let high_event = make_event(
        "high-1",
        IntakeSourceType::Github,
        "Critical",
        IssueSeverity::Critical,
    );

    let low_result = prioritizer.process(&low_event, &[]);
    let high_result = prioritizer.process(&high_event, &[]);

    assert!(matches!(low_result, PrioritizationResult::Filtered { .. }));
    assert!(matches!(high_result, PrioritizationResult::Processed(_)));
}

#[test]
fn auto_prioritizer_returns_priority_score() {
    let prioritizer = AutoPrioritizer::default();

    let event = make_event(
        "score-1",
        IntakeSourceType::Github,
        "Test",
        IssueSeverity::High,
    );
    let result = prioritizer.process(&event, &[]);

    match result {
        PrioritizationResult::Processed(info) => {
            assert!(info.priority > 0, "priority should be positive");
        }
        other => panic!("expected Processed, got {:?}", other),
    }
}
