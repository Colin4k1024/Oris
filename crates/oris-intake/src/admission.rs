//! Feasibility/risk-aware admission logic (#401) and clean rejection paths (#402).
//!
//! Implements candidate admission with feasibility scoring, blast-radius-aware
//! classification, and bounded rejection rules. Biases toward rejection over
//! unsafe admission.

use oris_agent_contract::{
    AutonomousCandidateSource, AutonomousDenialCondition, AutonomousPlanReasonCode,
    AutonomousRiskTier, BoundedTaskClass,
};
use serde::{Deserialize, Serialize};

// ─── Admission config ────────────────────────────────────────────────────────

/// Configuration for the admission gate.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AdmissionConfig {
    /// Minimum feasibility score to admit (0.0–1.0). Default: 0.5.
    pub min_feasibility: f32,
    /// Maximum allowed risk tier for auto-admission.
    pub max_auto_admit_risk: AutonomousRiskTier,
    /// Maximum files a candidate may touch for auto-admission.
    pub max_files_changed: usize,
    /// Maximum lines a candidate may change for auto-admission.
    pub max_lines_changed: usize,
    /// Whether to allow candidates with no signal classification.
    pub allow_unclassified: bool,
}

impl Default for AdmissionConfig {
    fn default() -> Self {
        Self {
            min_feasibility: 0.5,
            max_auto_admit_risk: AutonomousRiskTier::Low,
            max_files_changed: 5,
            max_lines_changed: 200,
            allow_unclassified: false,
        }
    }
}

// ─── Admission input ─────────────────────────────────────────────────────────

/// Input to the admission gate.
#[derive(Clone, Debug)]
pub struct AdmissionInput {
    /// Unique candidate identifier (dedupe key).
    pub dedupe_key: String,
    /// Source of the candidate.
    pub candidate_source: AutonomousCandidateSource,
    /// Inferred bounded task class, if any.
    pub task_class: Option<BoundedTaskClass>,
    /// Raw signal strings from intake.
    pub raw_signals: Vec<String>,
    /// Estimated blast radius: files changed.
    pub estimated_files: usize,
    /// Estimated blast radius: lines changed.
    pub estimated_lines: usize,
}

// ─── Admission decision ──────────────────────────────────────────────────────

/// Outcome of the admission gate.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AdmissionDecision {
    /// Whether the candidate was admitted.
    pub admitted: bool,
    /// Feasibility score (0.0–1.0).
    pub feasibility_score: f32,
    /// Assessed risk tier.
    pub risk_tier: AutonomousRiskTier,
    /// Reason code for the decision.
    pub reason_code: AutonomousPlanReasonCode,
    /// Human-readable summary.
    pub summary: String,
    /// Denial details (populated on rejection).
    pub denial: Option<AutonomousDenialCondition>,
}

// ─── Rejection reason ────────────────────────────────────────────────────────

/// Structured rejection reason for clean feedback (#402).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RejectionFeedback {
    /// The reason code.
    pub reason_code: AutonomousPlanReasonCode,
    /// Human-readable explanation of why the candidate was rejected.
    pub explanation: String,
    /// Suggested recovery action, if any.
    pub recovery_hint: Option<String>,
    /// Whether the candidate should be escalated to human review.
    pub escalate_to_human: bool,
}

// ─── Admission gate ──────────────────────────────────────────────────────────

/// Feasibility/risk-aware admission gate.
///
/// Evaluates candidates against configured thresholds and produces either
/// an admission or a structured rejection with clear feedback.
pub struct AdmissionGate {
    config: AdmissionConfig,
}

impl AdmissionGate {
    pub fn new(config: AdmissionConfig) -> Self {
        Self { config }
    }

    /// Evaluate a candidate for admission.
    pub fn evaluate(&self, input: &AdmissionInput) -> AdmissionDecision {
        // Step 1: Check task class classification
        if input.task_class.is_none() && !self.config.allow_unclassified {
            return self.reject(
                0.0,
                AutonomousRiskTier::High,
                AutonomousPlanReasonCode::DeniedUnsupportedClass,
                "Candidate has no recognised task class".into(),
                Some("Ensure signals map to a supported BoundedTaskClass".into()),
                false,
            );
        }

        // Step 2: Assess risk tier from blast radius
        let risk_tier = self.assess_risk(input);

        // Step 3: Check risk tier against auto-admit threshold
        if risk_tier_ord(&risk_tier) > risk_tier_ord(&self.config.max_auto_admit_risk) {
            return self.reject(
                self.compute_feasibility(input),
                risk_tier,
                AutonomousPlanReasonCode::DeniedHighRisk,
                format!(
                    "Risk tier {:?} exceeds auto-admit threshold {:?}",
                    risk_tier, self.config.max_auto_admit_risk
                ),
                Some("Reduce blast radius or request human review".into()),
                true,
            );
        }

        // Step 4: Compute feasibility score
        let feasibility = self.compute_feasibility(input);
        if feasibility < self.config.min_feasibility {
            return self.reject(
                feasibility,
                risk_tier,
                AutonomousPlanReasonCode::DeniedLowFeasibility,
                format!(
                    "Feasibility score {:.2} below threshold {:.2}",
                    feasibility, self.config.min_feasibility
                ),
                Some("Provide clearer signals or reduce scope".into()),
                false,
            );
        }

        // Step 5: Check for empty signals (ambiguous input)
        if input.raw_signals.is_empty() {
            return self.reject(
                feasibility,
                risk_tier,
                AutonomousPlanReasonCode::DeniedNoEvidence,
                "No signals provided — ambiguous candidate".into(),
                Some("Provide at least one diagnostic signal".into()),
                false,
            );
        }

        // Admitted
        AdmissionDecision {
            admitted: true,
            feasibility_score: feasibility,
            risk_tier,
            reason_code: AutonomousPlanReasonCode::Approved,
            summary: format!(
                "Candidate admitted: feasibility={:.2}, risk={:?}",
                feasibility, risk_tier
            ),
            denial: None,
        }
    }

    /// Produce a structured rejection with feedback.
    pub fn rejection_feedback(&self, decision: &AdmissionDecision) -> Option<RejectionFeedback> {
        if decision.admitted {
            return None;
        }
        Some(RejectionFeedback {
            reason_code: decision.reason_code.clone(),
            explanation: decision.summary.clone(),
            recovery_hint: decision.denial.as_ref().map(|d| d.recovery_hint.clone()),
            escalate_to_human: decision
                .denial
                .as_ref()
                .map(|d| {
                    matches!(
                        d.reason_code,
                        AutonomousPlanReasonCode::DeniedHighRisk
                            | AutonomousPlanReasonCode::UnknownFailClosed
                    )
                })
                .unwrap_or(false),
        })
    }

    fn assess_risk(&self, input: &AdmissionInput) -> AutonomousRiskTier {
        if input.estimated_files > self.config.max_files_changed
            || input.estimated_lines > self.config.max_lines_changed
        {
            return AutonomousRiskTier::High;
        }
        if input.estimated_files > self.config.max_files_changed / 2
            || input.estimated_lines > self.config.max_lines_changed / 2
        {
            return AutonomousRiskTier::Medium;
        }
        AutonomousRiskTier::Low
    }

    fn compute_feasibility(&self, input: &AdmissionInput) -> f32 {
        let mut score = 0.0f32;

        // Signal clarity: more signals = higher feasibility (up to 0.4)
        let signal_score = (input.raw_signals.len() as f32 / 5.0).min(1.0) * 0.4;
        score += signal_score;

        // Task class match: classified = 0.3
        if input.task_class.is_some() {
            score += 0.3;
        }

        // Blast radius: smaller = higher feasibility (up to 0.3)
        let blast_ratio = (input.estimated_files as f32 / self.config.max_files_changed as f32)
            .min(1.0);
        score += (1.0 - blast_ratio) * 0.3;

        score
    }

    fn reject(
        &self,
        feasibility: f32,
        risk_tier: AutonomousRiskTier,
        reason_code: AutonomousPlanReasonCode,
        summary: String,
        recovery_hint: Option<String>,
        _escalate: bool,
    ) -> AdmissionDecision {
        AdmissionDecision {
            admitted: false,
            feasibility_score: feasibility,
            risk_tier: risk_tier.clone(),
            reason_code: reason_code.clone(),
            summary: summary.clone(),
            denial: Some(AutonomousDenialCondition {
                reason_code,
                description: summary,
                recovery_hint: recovery_hint.unwrap_or_default(),
            }),
        }
    }
}

impl Default for AdmissionGate {
    fn default() -> Self {
        Self::new(AdmissionConfig::default())
    }
}

fn risk_tier_ord(tier: &AutonomousRiskTier) -> u8 {
    match tier {
        AutonomousRiskTier::Low => 0,
        AutonomousRiskTier::Medium => 1,
        AutonomousRiskTier::High => 2,
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_input(
        task_class: Option<BoundedTaskClass>,
        signals: Vec<String>,
        files: usize,
        lines: usize,
    ) -> AdmissionInput {
        AdmissionInput {
            dedupe_key: "test-key".into(),
            candidate_source: AutonomousCandidateSource::CiFailure,
            task_class,
            raw_signals: signals,
            estimated_files: files,
            estimated_lines: lines,
        }
    }

    #[test]
    fn admits_low_risk_classified_candidate() {
        let gate = AdmissionGate::default();
        let input = make_input(
            Some(BoundedTaskClass::LintFix),
            vec!["warning: unused import".into()],
            1,
            10,
        );
        let decision = gate.evaluate(&input);
        assert!(decision.admitted);
        assert!(matches!(
            decision.reason_code,
            AutonomousPlanReasonCode::Approved
        ));
    }

    #[test]
    fn rejects_unclassified_candidate() {
        let gate = AdmissionGate::default();
        let input = make_input(None, vec!["some signal".into()], 1, 10);
        let decision = gate.evaluate(&input);
        assert!(!decision.admitted);
        assert!(matches!(
            decision.reason_code,
            AutonomousPlanReasonCode::DeniedUnsupportedClass
        ));
    }

    #[test]
    fn rejects_high_risk_candidate() {
        let gate = AdmissionGate::default();
        let input = make_input(
            Some(BoundedTaskClass::LintFix),
            vec!["signal".into()],
            20,
            500,
        );
        let decision = gate.evaluate(&input);
        assert!(!decision.admitted);
        assert!(matches!(
            decision.reason_code,
            AutonomousPlanReasonCode::DeniedHighRisk
        ));
    }

    #[test]
    fn rejects_empty_signals() {
        let gate = AdmissionGate::new(AdmissionConfig {
            allow_unclassified: true,
            ..Default::default()
        });
        let input = make_input(Some(BoundedTaskClass::LintFix), vec![], 1, 5);
        let decision = gate.evaluate(&input);
        assert!(!decision.admitted);
        assert!(matches!(
            decision.reason_code,
            AutonomousPlanReasonCode::DeniedNoEvidence
        ));
    }

    #[test]
    fn rejection_feedback_populated_on_denial() {
        let gate = AdmissionGate::default();
        let input = make_input(None, vec!["signal".into()], 1, 10);
        let decision = gate.evaluate(&input);
        let feedback = gate.rejection_feedback(&decision);
        assert!(feedback.is_some());
        let fb = feedback.unwrap();
        assert!(!fb.explanation.is_empty());
    }

    #[test]
    fn rejection_feedback_none_on_admission() {
        let gate = AdmissionGate::default();
        let input = make_input(
            Some(BoundedTaskClass::LintFix),
            vec!["signal".into()],
            1,
            10,
        );
        let decision = gate.evaluate(&input);
        assert!(gate.rejection_feedback(&decision).is_none());
    }

    #[test]
    fn high_risk_rejection_escalates_to_human() {
        let gate = AdmissionGate::default();
        let input = make_input(
            Some(BoundedTaskClass::LintFix),
            vec!["signal".into()],
            20,
            500,
        );
        let decision = gate.evaluate(&input);
        let feedback = gate.rejection_feedback(&decision).unwrap();
        assert!(feedback.escalate_to_human);
    }

    #[test]
    fn medium_risk_admitted_when_config_allows() {
        let gate = AdmissionGate::new(AdmissionConfig {
            max_auto_admit_risk: AutonomousRiskTier::Medium,
            ..Default::default()
        });
        let input = make_input(
            Some(BoundedTaskClass::LintFix),
            vec!["signal1".into(), "signal2".into()],
            3,
            120,
        );
        let decision = gate.evaluate(&input);
        assert!(decision.admitted);
    }

    #[test]
    fn feasibility_increases_with_more_signals() {
        let gate = AdmissionGate::default();
        let few = make_input(
            Some(BoundedTaskClass::LintFix),
            vec!["s1".into()],
            1,
            10,
        );
        let many = make_input(
            Some(BoundedTaskClass::LintFix),
            vec!["s1".into(), "s2".into(), "s3".into(), "s4".into(), "s5".into()],
            1,
            10,
        );
        let d_few = gate.evaluate(&few);
        let d_many = gate.evaluate(&many);
        assert!(d_many.feasibility_score > d_few.feasibility_score);
    }
}
