//! Approval checkpoint definitions for sensitive work classes.
//!
//! Approval checkpoints define when human review is required, what evidence
//! must be collected, and how to escalate when policies are violated.

use serde::{Deserialize, Serialize};

// ─── HumanReviewRequirement ───────────────────────────────────────────────────

/// Defines whether and why human review is required at a checkpoint.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct HumanReviewRequirement {
    /// Whether human review is required.
    pub required: bool,
    /// Why human review is needed — used for audit trail and notifications.
    pub rationale: String,
    /// Optional SLA in hours for completing the review.
    pub sla_hours: Option<u32>,
}

impl HumanReviewRequirement {
    /// Create a new human review requirement.
    pub fn new(required: bool, rationale: impl Into<String>, sla_hours: Option<u32>) -> Self {
        Self {
            required,
            rationale: rationale.into(),
            sla_hours,
        }
    }

    /// Check if review has breached its SLA.
    ///
    /// Returns `true` if an SLA is set and `elapsed_hours` exceeds it.
    pub fn is_sla_breached(&self, elapsed_hours: u32) -> bool {
        self.sla_hours
            .map(|sla| elapsed_hours > sla)
            .unwrap_or(false)
    }
}

// ─── EvidenceCompleteness ────────────────────────────────────────────────────

/// Criteria for determining whether collected evidence is sufficient.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct EvidenceCompleteness {
    /// Minimum replay success rate (0.0–1.0).
    pub min_replay_success_rate: f32,
    /// Minimum number of successful replays required.
    pub min_successful_replays: u32,
    /// Whether environment match is required for evidence to be valid.
    pub require_exact_environment: bool,
    /// Minimum confidence threshold (0.0–1.0).
    pub min_confidence: f32,
}

impl EvidenceCompleteness {
    /// Create a new evidence completeness rule.
    pub fn new(
        min_replay_success_rate: f32,
        min_successful_replays: u32,
        require_exact_environment: bool,
        min_confidence: f32,
    ) -> Self {
        Self {
            min_replay_success_rate,
            min_successful_replays,
            require_exact_environment,
            min_confidence,
        }
    }

    /// Check whether the given evidence meets completeness requirements.
    pub fn is_complete(&self, evidence: &Evidence) -> bool {
        evidence.replay_success_rate >= self.min_replay_success_rate
            && evidence.successful_replays >= self.min_successful_replays
            && (!self.require_exact_environment || evidence.environment_match)
            && evidence.confidence >= self.min_confidence
    }
}

// ─── Evidence ─────────────────────────────────────────────────────────────────

/// Evidence collected at a checkpoint for completeness evaluation.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Evidence {
    /// Observed replay success rate (0.0–1.0).
    pub replay_success_rate: f32,
    /// Number of successful replays.
    pub successful_replays: u32,
    /// Whether the execution environment matched expectations.
    pub environment_match: bool,
    /// Confidence score (0.0–1.0).
    pub confidence: f32,
}

impl Evidence {
    /// Create new evidence.
    pub fn new(
        replay_success_rate: f32,
        successful_replays: u32,
        environment_match: bool,
        confidence: f32,
    ) -> Self {
        Self {
            replay_success_rate,
            successful_replays,
            environment_match,
            confidence,
        }
    }

    /// Evidence with zero successful replays — used as a default.
    pub fn none() -> Self {
        Self {
            replay_success_rate: 0.0,
            successful_replays: 0,
            environment_match: false,
            confidence: 0.0,
        }
    }
}

// ─── EscalationTrigger ────────────────────────────────────────────────────────

/// Condition that triggers escalation.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", content = "value")]
pub enum EscalationTrigger {
    /// Escalate after `0` failed review attempts.
    FailedReviews(u32),
    /// Escalate when SLA is breached.
    SlaBreach,
    /// Escalate when evidence is incomplete after `hours`.
    EvidenceTimeout { hours: u32 },
}

impl EscalationTrigger {
    /// Check if escalation should trigger given current state.
    pub fn should_escalate(
        &self,
        failed_reviews: u32,
        sla_breached: bool,
        evidence_age_hours: Option<u32>,
        evidence_complete: bool,
    ) -> bool {
        match self {
            EscalationTrigger::FailedReviews(threshold) => failed_reviews >= *threshold,
            EscalationTrigger::SlaBreach => sla_breached,
            EscalationTrigger::EvidenceTimeout { hours } => {
                evidence_age_hours.map(|age| age >= *hours).unwrap_or(false) && !evidence_complete
            }
        }
    }
}

// ─── EscalationPolicy ────────────────────────────────────────────────────────

/// Policy controlling how and when a checkpoint escalates.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct EscalationPolicy {
    /// Whether escalation is enabled.
    pub enabled: bool,
    /// Condition that triggers escalation.
    pub trigger: EscalationTrigger,
    /// Priority elevation (e.g. 0 = no change, 5 = significant boost).
    pub elevated_priority: u8,
}

impl EscalationPolicy {
    /// Create a new escalation policy.
    pub fn new(enabled: bool, trigger: EscalationTrigger, elevated_priority: u8) -> Self {
        Self {
            enabled,
            trigger,
            elevated_priority,
        }
    }

    /// Escalate after `n` failed reviews.
    pub fn on_failed_reviews(n: u32, elevated_priority: u8) -> Self {
        Self {
            enabled: true,
            trigger: EscalationTrigger::FailedReviews(n),
            elevated_priority,
        }
    }

    /// Escalate on SLA breach.
    pub fn on_sla_breach(elevated_priority: u8) -> Self {
        Self {
            enabled: true,
            trigger: EscalationTrigger::SlaBreach,
            elevated_priority,
        }
    }

    /// Escalate when evidence is incomplete after timeout.
    pub fn on_evidence_timeout(hours: u32, elevated_priority: u8) -> Self {
        Self {
            enabled: true,
            trigger: EscalationTrigger::EvidenceTimeout { hours },
            elevated_priority,
        }
    }

    /// Check whether escalation conditions are met.
    pub fn should_escalate(
        &self,
        failed_reviews: u32,
        sla_breached: bool,
        evidence_age_hours: Option<u32>,
        evidence_complete: bool,
    ) -> bool {
        self.enabled
            && self.trigger.should_escalate(
                failed_reviews,
                sla_breached,
                evidence_age_hours,
                evidence_complete,
            )
    }
}

// ─── ApprovalCheckpoint ───────────────────────────────────────────────────────

/// A gate that must be satisfied before a task class can progress.
///
/// Checkpoints can require human review, enforce evidence completeness,
/// and trigger escalation when policies are violated.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ApprovalCheckpoint {
    /// Unique identifier for this checkpoint.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Optional human review requirement.
    pub human_review: Option<HumanReviewRequirement>,
    /// Optional evidence completeness rule.
    pub evidence: Option<EvidenceCompleteness>,
    /// Optional escalation policy.
    pub escalation: Option<EscalationPolicy>,
}

impl ApprovalCheckpoint {
    /// Create a new checkpoint.
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        human_review: Option<HumanReviewRequirement>,
        evidence: Option<EvidenceCompleteness>,
        escalation: Option<EscalationPolicy>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            human_review,
            evidence,
            escalation,
        }
    }

    /// A checkpoint that always passes without any requirements.
    pub fn auto_approve() -> Self {
        Self {
            id: "auto".to_string(),
            name: "Auto-approve".to_string(),
            human_review: None,
            evidence: None,
            escalation: None,
        }
    }

    /// A checkpoint requiring human review.
    pub fn human_review_required(rationale: impl Into<String>) -> Self {
        Self {
            id: "human-review".to_string(),
            name: "Human review required".to_string(),
            human_review: Some(HumanReviewRequirement::new(true, rationale, None)),
            evidence: None,
            escalation: None,
        }
    }

    /// Whether this checkpoint requires human review.
    pub fn requires_human_review(&self) -> bool {
        self.human_review
            .as_ref()
            .map(|r| r.required)
            .unwrap_or(false)
    }

    /// Check whether evidence satisfies the completeness rule.
    ///
    /// Returns `true` if no evidence rule is defined, or if the rule is satisfied.
    pub fn check_evidence_completeness(&self, evidence: &Evidence) -> bool {
        self.evidence
            .as_ref()
            .map(|e| e.is_complete(evidence))
            .unwrap_or(true)
    }

    /// Add a human review requirement to this checkpoint.
    pub fn with_human_review(mut self, requirement: HumanReviewRequirement) -> Self {
        self.human_review = Some(requirement);
        self
    }

    /// Add an evidence completeness rule to this checkpoint.
    pub fn with_evidence(mut self, evidence: EvidenceCompleteness) -> Self {
        self.evidence = Some(evidence);
        self
    }

    /// Add an escalation policy to this checkpoint.
    pub fn with_escalation(mut self, policy: EscalationPolicy) -> Self {
        self.escalation = Some(policy);
        self
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── HumanReviewRequirement ────────────────────────────────────────────────

    #[test]
    fn human_review_requirement_sla_breach() {
        let req = HumanReviewRequirement::new(true, "security sensitive", Some(24));
        assert!(!req.is_sla_breached(12));
        assert!(!req.is_sla_breached(24));
        assert!(req.is_sla_breached(25));
    }

    #[test]
    fn human_review_requirement_no_sla() {
        let req = HumanReviewRequirement::new(true, "security sensitive", None);
        assert!(!req.is_sla_breached(9999));
    }

    // ── EvidenceCompleteness ──────────────────────────────────────────────────

    #[test]
    fn evidence_completeness_all_criteria_met() {
        let rule = EvidenceCompleteness::new(0.8, 3, true, 0.9);
        let evidence = Evidence::new(0.9, 5, true, 0.95);
        assert!(rule.is_complete(&evidence));
    }

    #[test]
    fn evidence_completeness_low_replay_rate() {
        let rule = EvidenceCompleteness::new(0.8, 3, true, 0.9);
        let evidence = Evidence::new(0.5, 5, true, 0.95);
        assert!(!rule.is_complete(&evidence));
    }

    #[test]
    fn evidence_completeness_insufficient_replays() {
        let rule = EvidenceCompleteness::new(0.8, 3, true, 0.9);
        let evidence = Evidence::new(0.9, 2, true, 0.95);
        assert!(!rule.is_complete(&evidence));
    }

    #[test]
    fn evidence_completeness_environment_mismatch() {
        let rule = EvidenceCompleteness::new(0.8, 3, true, 0.9);
        let evidence = Evidence::new(0.9, 5, false, 0.95);
        assert!(!rule.is_complete(&evidence));
    }

    #[test]
    fn evidence_completeness_low_confidence() {
        let rule = EvidenceCompleteness::new(0.8, 3, true, 0.9);
        let evidence = Evidence::new(0.9, 5, true, 0.8);
        assert!(!rule.is_complete(&evidence));
    }

    #[test]
    fn evidence_completeness_environment_not_required() {
        let rule = EvidenceCompleteness::new(0.8, 3, false, 0.9);
        let evidence = Evidence::new(0.9, 5, false, 0.95);
        assert!(rule.is_complete(&evidence));
    }

    // ── EscalationTrigger ───────────────────────────────────────────────────

    #[test]
    fn escalation_trigger_failed_reviews() {
        let trigger = EscalationTrigger::FailedReviews(3);
        assert!(!trigger.should_escalate(0, false, None, false));
        assert!(!trigger.should_escalate(2, false, None, false));
        assert!(trigger.should_escalate(3, false, None, false));
        assert!(trigger.should_escalate(5, false, None, false));
    }

    #[test]
    fn escalation_trigger_sla_breach() {
        let trigger = EscalationTrigger::SlaBreach;
        assert!(!trigger.should_escalate(0, false, None, false));
        assert!(trigger.should_escalate(0, true, None, false));
    }

    #[test]
    fn escalation_trigger_evidence_timeout() {
        let trigger = EscalationTrigger::EvidenceTimeout { hours: 48 };
        assert!(!trigger.should_escalate(0, false, Some(24), false));
        assert!(!trigger.should_escalate(0, false, Some(48), true)); // complete = no escalation
        assert!(trigger.should_escalate(0, false, Some(48), false));
        assert!(trigger.should_escalate(0, false, Some(100), false));
        assert!(!trigger.should_escalate(0, false, None, false)); // no timeout without age
    }

    // ── EscalationPolicy ────────────────────────────────────────────────────

    #[test]
    fn escalation_policy_disabled_never_escalates() {
        let policy = EscalationPolicy::new(false, EscalationTrigger::FailedReviews(1), 5);
        assert!(!policy.should_escalate(99, false, None, false));
    }

    #[test]
    fn escalation_policy_on_failed_reviews() {
        let policy = EscalationPolicy::on_failed_reviews(2, 3);
        assert!(!policy.should_escalate(1, false, None, false));
        assert!(policy.should_escalate(2, false, None, false));
    }

    // ── ApprovalCheckpoint ───────────────────────────────────────────────────

    #[test]
    fn checkpoint_auto_approve() {
        let cp = ApprovalCheckpoint::auto_approve();
        assert!(!cp.requires_human_review());
        assert!(cp.check_evidence_completeness(&Evidence::none()));
    }

    #[test]
    fn checkpoint_human_review_required() {
        let cp = ApprovalCheckpoint::human_review_required("critical safety check");
        assert!(cp.requires_human_review());
        assert_eq!(
            cp.human_review.as_ref().unwrap().rationale,
            "critical safety check"
        );
    }

    #[test]
    fn checkpoint_evidence_completeness_pass() {
        let cp = ApprovalCheckpoint::new(
            "test",
            "Test",
            None,
            Some(EvidenceCompleteness::new(0.8, 3, false, 0.9)),
            None,
        );
        let evidence = Evidence::new(0.9, 5, true, 0.95);
        assert!(cp.check_evidence_completeness(&evidence));
    }

    #[test]
    fn checkpoint_evidence_completeness_fail() {
        let cp = ApprovalCheckpoint::new(
            "test",
            "Test",
            None,
            Some(EvidenceCompleteness::new(0.8, 3, false, 0.9)),
            None,
        );
        let evidence = Evidence::new(0.5, 1, true, 0.5);
        assert!(!cp.check_evidence_completeness(&evidence));
    }

    #[test]
    fn checkpoint_builder_pattern() {
        let cp = ApprovalCheckpoint::new("test", "Test", None, None, None)
            .with_human_review(HumanReviewRequirement::new(true, "security", Some(24)))
            .with_evidence(EvidenceCompleteness::new(0.9, 5, true, 0.95))
            .with_escalation(EscalationPolicy::on_failed_reviews(3, 5));

        assert!(cp.requires_human_review());
        assert!(cp.evidence.is_some());
        assert!(cp.escalation.is_some());
    }

    #[test]
    fn checkpoint_full_approval_flow() {
        // Simulate a full approval flow with evidence
        let cp = ApprovalCheckpoint::new(
            "sensitive-mutation",
            "Sensitive Mutation Approval",
            Some(HumanReviewRequirement::new(
                true,
                "Mutation affects core runtime",
                Some(48),
            )),
            Some(EvidenceCompleteness::new(0.95, 5, true, 0.9)),
            Some(EscalationPolicy::on_failed_reviews(2, 5)),
        );

        // Evidence meets all criteria
        let good_evidence = Evidence::new(0.98, 7, true, 0.95);
        assert!(cp.check_evidence_completeness(&good_evidence));
        assert!(cp.requires_human_review());
        assert!(!cp
            .escalation
            .as_ref()
            .unwrap()
            .should_escalate(1, false, None, true));

        // Evidence is incomplete
        let bad_evidence = Evidence::new(0.7, 2, false, 0.6);
        assert!(!cp.check_evidence_completeness(&bad_evidence));
    }

    // ── Evidence ─────────────────────────────────────────────────────────────

    #[test]
    fn evidence_none() {
        let e = Evidence::none();
        assert_eq!(e.replay_success_rate, 0.0);
        assert_eq!(e.successful_replays, 0);
        assert!(!e.environment_match);
        assert_eq!(e.confidence, 0.0);
    }
}
