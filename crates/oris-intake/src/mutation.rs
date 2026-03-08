//! Mutation builder for converting intake events to evolution mutations

use crate::signal::ExtractedSignal;
use crate::source::IntakeEvent;
use crate::{IntakeError, IntakeResult};
use serde::{Deserialize, Serialize};

/// A mutation proposal generated from intake events
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IntakeMutation {
    /// Unique mutation ID
    pub mutation_id: String,

    /// Intent description
    pub intent: String,

    /// Target for the mutation
    pub target: MutationTarget,

    /// Expected effect
    pub expected_effect: String,

    /// Risk level
    pub risk: MutationRisk,

    /// Extracted signals that triggered this mutation
    pub signals: Vec<String>,

    /// Source event IDs
    pub source_event_ids: Vec<String>,

    /// Priority based on severity
    pub priority: i32,
}

/// Mutation target
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationTarget {
    /// Apply to entire workspace
    WorkspaceRoot,
    /// Apply to specific crate
    Crate { name: String },
    /// Apply to specific paths
    Paths { allow: Vec<String> },
}

/// Mutation risk level
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MutationRisk {
    Low,
    Medium,
    High,
    Critical,
}

impl Default for MutationRisk {
    fn default() -> Self {
        Self::Medium
    }
}

impl From<crate::source::IssueSeverity> for MutationRisk {
    fn from(severity: crate::source::IssueSeverity) -> Self {
        match severity {
            crate::source::IssueSeverity::Critical => MutationRisk::Critical,
            crate::source::IssueSeverity::High => MutationRisk::High,
            crate::source::IssueSeverity::Medium => MutationRisk::Medium,
            crate::source::IssueSeverity::Low => MutationRisk::Low,
            crate::source::IssueSeverity::Info => MutationRisk::Low,
        }
    }
}

/// Builder for creating mutations from intake events
pub struct MutationBuilder {
    /// Default risk level
    default_risk: MutationRisk,

    /// Maximum signals per mutation
    max_signals: usize,
}

impl MutationBuilder {
    /// Create a new mutation builder
    pub fn new() -> Self {
        Self {
            default_risk: MutationRisk::Medium,
            max_signals: 5,
        }
    }

    /// Build a mutation from an intake event and extracted signals
    pub fn build(&self, event: &IntakeEvent, signals: &[ExtractedSignal]) -> IntakeMutation {
        let risk: MutationRisk = event.severity.clone().into();

        let signals_str: Vec<String> = signals
            .iter()
            .take(self.max_signals)
            .map(|s| s.content.clone())
            .collect();

        let intent = format!("Auto-intake: {} - {}", event.title, signals_str.join(", "));

        let expected_effect = format!(
            "Resolve {} from {} source",
            event.title,
            event.source_type.to_string()
        );

        let priority = match event.severity {
            crate::source::IssueSeverity::Critical => 100,
            crate::source::IssueSeverity::High => 75,
            crate::source::IssueSeverity::Medium => 50,
            crate::source::IssueSeverity::Low => 25,
            crate::source::IssueSeverity::Info => 10,
        };

        IntakeMutation {
            mutation_id: uuid::Uuid::new_v4().to_string(),
            intent,
            target: MutationTarget::WorkspaceRoot,
            expected_effect,
            risk,
            signals: signals_str,
            source_event_ids: vec![event.event_id.clone()],
            priority,
        }
    }

    /// Build mutations from multiple events
    pub fn build_batch(
        &self,
        events: &[IntakeEvent],
        signals_map: &[Vec<ExtractedSignal>],
    ) -> Vec<IntakeMutation> {
        events
            .iter()
            .zip(signals_map.iter())
            .map(|(event, signals)| self.build(event, signals))
            .collect()
    }
}

impl Default for MutationBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert intake mutation to oris-evolution MutationIntent
impl From<&IntakeMutation> for oris_evolution::MutationIntent {
    fn from(mutation: &IntakeMutation) -> Self {
        let target = match &mutation.target {
            MutationTarget::WorkspaceRoot => oris_evolution::MutationTarget::WorkspaceRoot,
            MutationTarget::Crate { name } => {
                oris_evolution::MutationTarget::Crate { name: name.clone() }
            }
            MutationTarget::Paths { allow } => oris_evolution::MutationTarget::Paths {
                allow: allow.clone(),
            },
        };

        let risk = match mutation.risk {
            MutationRisk::Low => oris_evolution::RiskLevel::Low,
            MutationRisk::Medium => oris_evolution::RiskLevel::Medium,
            MutationRisk::High | MutationRisk::Critical => oris_evolution::RiskLevel::High,
        };

        oris_evolution::MutationIntent {
            id: mutation.mutation_id.clone(),
            intent: mutation.intent.clone(),
            target,
            expected_effect: mutation.expected_effect.clone(),
            risk,
            signals: mutation.signals.clone(),
            spec_id: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signal::SignalType;
    use crate::source::{IntakeSourceType, IssueSeverity};

    #[test]
    fn test_mutation_builder() {
        let builder = MutationBuilder::new();

        let event = IntakeEvent {
            event_id: "event-1".to_string(),
            source_type: IntakeSourceType::Github,
            source_event_id: Some("run-123".to_string()),
            title: "Build failed".to_string(),
            description: "Borrow checker error in main.rs".to_string(),
            severity: IssueSeverity::High,
            signals: vec![],
            raw_payload: None,
            timestamp_ms: 0,
        };

        let signals = vec![ExtractedSignal {
            signal_id: "sig-1".to_string(),
            content: "compiler_error:borrow checker".to_string(),
            signal_type: SignalType::CompilerError,
            confidence: 0.8,
            source: "github".to_string(),
        }];

        let mutation = builder.build(&event, &signals);
        assert_eq!(mutation.risk, MutationRisk::High);
        assert!(mutation.intent.contains("Build failed"));
    }

    #[test]
    fn test_severity_to_risk() {
        assert_eq!(
            MutationRisk::from(IssueSeverity::Critical),
            MutationRisk::Critical
        );
        assert_eq!(MutationRisk::from(IssueSeverity::High), MutationRisk::High);
        assert_eq!(
            MutationRisk::from(IssueSeverity::Medium),
            MutationRisk::Medium
        );
        assert_eq!(MutationRisk::from(IssueSeverity::Low), MutationRisk::Low);
        assert_eq!(MutationRisk::from(IssueSeverity::Info), MutationRisk::Low);
    }
}
