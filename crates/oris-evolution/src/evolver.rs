//! Evolver automation feedback loop for self-evolution.
//!
//! This module provides the core components for automated evolution:
//! - Signal extraction from execution feedback
//! - Mutation proposal generation
//! - Validation before application
//! - Feedback collection and iteration

use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Generate a unique ID
fn generate_id(prefix: &str) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}-{:x}", prefix, now.as_nanos())
}

/// A signal extracted from execution feedback
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EvolutionSignal {
    pub signal_id: String,
    pub signal_type: SignalType,
    pub source_task_id: String,
    pub confidence: f32,
    pub description: String,
    pub metadata: serde_json::Value,
}

/// Types of evolution signals
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SignalType {
    /// Performance improvement opportunity
    Performance {
        metric: String,
        improvement_potential: f32,
    },
    /// Error pattern detected
    ErrorPattern { error_type: String, frequency: u32 },
    /// Resource optimization
    ResourceOptimization {
        resource_type: String,
        current_usage: f32,
    },
    /// Quality issue
    QualityIssue { issue_type: String, severity: f32 },
    /// Success pattern that could be generalized
    SuccessPattern { pattern: String, repeatability: f32 },
}

/// A proposed mutation based on signals
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MutationProposal {
    pub proposal_id: String,
    pub signal_ids: Vec<String>,
    pub gene_id: String,
    pub description: String,
    pub estimated_impact: f32,
    pub risk_level: MutationRiskLevel,
    pub proposed_changes: serde_json::Value,
}

/// Risk level for mutations
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum MutationRiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

/// Status of a mutation proposal
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum ProposalStatus {
    Proposed,
    Validating,
    Validated,
    Rejected { reason: String },
    Approved,
    Applied,
    Failed { error: String },
}

/// Result of validation
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ValidationResult {
    pub proposal_id: String,
    pub passed: bool,
    pub score: f32,
    pub issues: Vec<ValidationIssue>,
    pub simulation_results: Option<serde_json::Value>,
}

/// A validation issue
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ValidationIssue {
    pub severity: IssueSeverity,
    pub description: String,
    pub location: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum IssueSeverity {
    Info,
    Warning,
    Error,
}

/// Configuration for the evolver automation
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EvolverConfig {
    /// Enable automated evolution
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Minimum signal confidence to trigger proposal
    #[serde(default = "default_min_confidence")]
    pub min_signal_confidence: f32,
    /// Maximum proposals per cycle
    #[serde(default = "default_max_proposals")]
    pub max_proposals_per_cycle: usize,
    /// Whether to require governor approval for high-risk mutations
    #[serde(default = "default_governor_required")]
    pub governor_required_for_high_risk: bool,
    /// Number of validation iterations
    #[serde(default = "default_validation_iterations")]
    pub validation_iterations: u32,
    /// Confidence threshold for auto-approval
    #[serde(default = "default_auto_approve_threshold")]
    pub auto_approve_threshold: f32,
}

fn default_enabled() -> bool {
    false
}
fn default_min_confidence() -> f32 {
    0.7
}
fn default_max_proposals() -> usize {
    10
}
fn default_governor_required() -> bool {
    true
}
fn default_validation_iterations() -> u32 {
    3
}
fn default_auto_approve_threshold() -> f32 {
    0.9
}

/// The evolver automation engine
#[derive(Clone)]
pub struct EvolverAutomation {
    config: EvolverConfig,
    signals: Arc<RwLock<Vec<EvolutionSignal>>>,
    proposals: Arc<RwLock<Vec<MutationProposal>>>,
    local_peer_id: String,
}

impl EvolverAutomation {
    /// Create a new evolver automation instance
    pub fn new(config: EvolverConfig, local_peer_id: String) -> Self {
        Self {
            config,
            signals: Arc::new(RwLock::new(Vec::new())),
            proposals: Arc::new(RwLock::new(Vec::new())),
            local_peer_id,
        }
    }

    /// Add a signal extracted from feedback
    pub fn add_signal(&self, signal: EvolutionSignal) {
        if signal.confidence >= self.config.min_signal_confidence {
            let mut signals = self.signals.write().unwrap();
            signals.push(signal);
        }
    }

    /// Get all signals
    pub fn get_signals(&self) -> Vec<EvolutionSignal> {
        self.signals.read().unwrap().clone()
    }

    /// Clear processed signals
    pub fn clear_signals(&self, signal_ids: &[String]) {
        let mut signals = self.signals.write().unwrap();
        signals.retain(|s| !signal_ids.contains(&s.signal_id));
    }

    /// Generate mutation proposals from signals
    pub fn generate_proposals(&self) -> Vec<MutationProposal> {
        let signals = self.signals.read().unwrap();
        let mut proposals = Vec::new();

        // Group signals by gene/target
        let mut by_gene: std::collections::HashMap<String, Vec<&EvolutionSignal>> =
            std::collections::HashMap::new();
        for signal in signals.iter() {
            if let Some(gene_id) = signal.metadata.get("gene_id").and_then(|v| v.as_str()) {
                by_gene.entry(gene_id.to_string()).or_default().push(signal);
            }
        }

        // Generate proposals
        let max = self.config.max_proposals_per_cycle;
        for (gene_id, gene_signals) in by_gene.iter().take(max) {
            let avg_confidence: f32 =
                gene_signals.iter().map(|s| s.confidence).sum::<f32>() / gene_signals.len() as f32;
            let risk_level = if avg_confidence > self.config.auto_approve_threshold {
                MutationRiskLevel::Low
            } else if avg_confidence > 0.5 {
                MutationRiskLevel::Medium
            } else {
                MutationRiskLevel::High
            };

            let proposal = MutationProposal {
                proposal_id: generate_id("proposal"),
                signal_ids: gene_signals.iter().map(|s| s.signal_id.clone()).collect(),
                gene_id: gene_id.clone(),
                description: format!(
                    "{} mutation proposal based on {} signals",
                    gene_id,
                    gene_signals.len()
                ),
                estimated_impact: avg_confidence,
                risk_level,
                proposed_changes: serde_json::json!({
                    "signals": gene_signals.iter().map(|s| s.signal_type.clone()).collect::<Vec<_>>()
                }),
            };
            proposals.push(proposal);
        }

        // Store proposals
        let mut stored = self.proposals.write().unwrap();
        stored.extend(proposals.clone());

        proposals
    }

    /// Validate a proposal (simplified - real impl would use sandbox)
    pub fn validate_proposal(&self, proposal_id: &str) -> ValidationResult {
        let proposals = self.proposals.read().unwrap();
        let proposal = proposals.iter().find(|p| p.proposal_id == proposal_id);

        if let Some(p) = proposal {
            // Simplified validation
            let passed = p.risk_level != MutationRiskLevel::Critical;
            let score = if passed { p.estimated_impact } else { 0.0 };

            ValidationResult {
                proposal_id: proposal_id.to_string(),
                passed,
                score,
                issues: if !passed {
                    vec![ValidationIssue {
                        severity: IssueSeverity::Error,
                        description: "Critical risk mutations require governor approval"
                            .to_string(),
                        location: None,
                    }]
                } else {
                    vec![]
                },
                simulation_results: Some(serde_json::json!({
                    "estimated_improvement": p.estimated_impact,
                    "risk_level": p.risk_level
                })),
            }
        } else {
            ValidationResult {
                proposal_id: proposal_id.to_string(),
                passed: false,
                score: 0.0,
                issues: vec![ValidationIssue {
                    severity: IssueSeverity::Error,
                    description: "Proposal not found".to_string(),
                    location: None,
                }],
                simulation_results: None,
            }
        }
    }

    /// Approve a proposal
    pub fn approve_proposal(&self, proposal_id: &str) -> bool {
        let mut proposals = self.proposals.write().unwrap();
        if let Some(p) = proposals.iter_mut().find(|p| p.proposal_id == proposal_id) {
            // Check if governor is required
            if self.config.governor_required_for_high_risk
                && p.risk_level == MutationRiskLevel::Critical
            {
                return false;
            }
            return true;
        }
        false
    }

    /// Get proposals by status
    pub fn get_proposals(&self, _status: Option<ProposalStatus>) -> Vec<MutationProposal> {
        let proposals = self.proposals.read().unwrap();
        proposals.clone()
    }

    /// Get config
    pub fn config(&self) -> &EvolverConfig {
        &self.config
    }
}

/// Builder for creating signals
pub struct SignalBuilder {
    signal_type: Option<SignalType>,
    source_task_id: String,
    confidence: f32,
    description: String,
    metadata: serde_json::Value,
}

impl SignalBuilder {
    pub fn new(source_task_id: String) -> Self {
        Self {
            signal_type: None,
            source_task_id,
            confidence: 0.0,
            description: String::new(),
            metadata: serde_json::json!({}),
        }
    }

    pub fn signal_type(mut self, signal_type: SignalType) -> Self {
        self.signal_type = Some(signal_type);
        self
    }

    pub fn confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence;
        self
    }

    pub fn description(mut self, description: String) -> Self {
        self.description = description;
        self
    }

    pub fn metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = metadata;
        self
    }

    pub fn build(self) -> Option<EvolutionSignal> {
        let signal_type = self.signal_type?;

        Some(EvolutionSignal {
            signal_id: generate_id("signal"),
            signal_type,
            source_task_id: self.source_task_id,
            confidence: self.confidence,
            description: self.description,
            metadata: self.metadata,
        })
    }
}

impl Default for EvolverConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            min_signal_confidence: default_min_confidence(),
            max_proposals_per_cycle: default_max_proposals(),
            governor_required_for_high_risk: default_governor_required(),
            validation_iterations: default_validation_iterations(),
            auto_approve_threshold: default_auto_approve_threshold(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signal_builder() {
        let signal = SignalBuilder::new("task-123".to_string())
            .signal_type(SignalType::Performance {
                metric: "latency".to_string(),
                improvement_potential: 0.5,
            })
            .confidence(0.8)
            .description("High latency detected".to_string())
            .build();

        assert!(signal.is_some());
        let s = signal.unwrap();
        assert_eq!(s.source_task_id, "task-123");
        assert!(s.confidence >= 0.7); // Above min threshold
    }

    #[test]
    fn test_proposal_generation() {
        let config = EvolverConfig {
            enabled: true,
            min_signal_confidence: 0.5,
            ..Default::default()
        };
        let evolver = EvolverAutomation::new(config, "local".to_string());

        // Add a signal
        let signal = SignalBuilder::new("task-1".to_string())
            .signal_type(SignalType::Performance {
                metric: "throughput".to_string(),
                improvement_potential: 0.8,
            })
            .confidence(0.9)
            .metadata(serde_json::json!({ "gene_id": "gene-1" }))
            .build()
            .unwrap();

        evolver.add_signal(signal);

        let proposals = evolver.generate_proposals();
        assert!(!proposals.is_empty());
    }

    #[test]
    fn test_validation() {
        let config = EvolverConfig::default();
        let evolver = EvolverAutomation::new(config, "local".to_string());

        let result = evolver.validate_proposal("non-existent");
        assert!(!result.passed);
    }
}
