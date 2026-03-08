//! GEP-compatible Capsule definition.
//!
//! A Capsule records a single successful evolution. It captures what triggered
//! the evolution, which gene was used, the outcome, and the actual code changes.

use super::content_hash::{compute_asset_id, AssetIdError};
use super::gene::GeneCategory;
use serde::{Deserialize, Serialize};

/// Capsule content - structured description of the evolution
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct CapsuleContent {
    /// Intent of the evolution
    #[serde(default)]
    pub intent: String,
    /// Strategy followed
    #[serde(default)]
    pub strategy: String,
    /// Scope of changes
    #[serde(default)]
    pub scope: String,
    /// Files that were changed
    #[serde(default, rename = "changed_files")]
    pub changed_files: Vec<String>,
    /// Rationale behind changes
    #[serde(default)]
    pub rationale: String,
    /// Outcome description
    #[serde(default)]
    pub outcome: String,
}

/// Trigger context - full context that triggered this evolution
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct TriggerContext {
    /// Original user/agent prompt (max 2000 chars)
    #[serde(default)]
    pub prompt: Option<String>,
    /// Agent's reasoning chain before executing (max 4000 chars)
    #[serde(default, rename = "reasoning_trace")]
    pub reasoning_trace: Option<String>,
    /// Additional contextual signals beyond trigger
    #[serde(default, rename = "context_signals")]
    pub context_signals: Vec<String>,
    /// Session identifier for cross-session tracking
    #[serde(default)]
    pub session_id: Option<String>,
    /// The LLM model used
    #[serde(default, rename = "agent_model")]
    pub agent_model: Option<String>,
}

/// Blast radius - scope of changes
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct BlastRadius {
    /// Number of files changed
    pub files: usize,
    /// Number of lines changed
    pub lines: usize,
}

/// Outcome result
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapsuleOutcome {
    /// Status: success or failed
    pub status: CapsuleStatus,
    /// Score from 0.0 to 1.0
    pub score: f32,
    /// Optional note
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CapsuleStatus {
    Success,
    Failed,
}

impl Default for CapsuleOutcome {
    fn default() -> Self {
        Self {
            status: CapsuleStatus::Failed,
            score: 0.0,
            note: None,
        }
    }
}

/// Environment fingerprint snapshot
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct EnvFingerprint {
    /// Runtime environment snapshot
    #[serde(default)]
    pub runtime: Option<String>,
    /// OS version
    #[serde(default)]
    pub os: Option<String>,
    /// Other env details
    #[serde(flatten)]
    pub extra: std::collections::HashMap<String, serde_json::Value>,
}

/// GEP-compatible Capsule definition
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GepCapsule {
    /// Asset type - always "Capsule"
    #[serde(rename = "type")]
    pub capsule_type: String,
    /// Protocol schema version
    #[serde(rename = "schema_version")]
    pub schema_version: String,
    /// Unique identifier (e.g., capsule_1708123456789)
    pub id: String,
    /// Signals that triggered this evolution
    pub trigger: Vec<String>,
    /// ID of the gene used
    pub gene: String,
    /// Human-readable description
    pub summary: String,
    /// Structured description
    #[serde(default)]
    pub content: Option<CapsuleContent>,
    /// Git diff of actual code changes
    #[serde(default)]
    pub diff: Option<String>,
    /// Ordered execution steps
    #[serde(default)]
    pub strategy: Option<Vec<String>>,
    /// Confidence 0.0-1.0
    pub confidence: f32,
    /// Blast radius
    #[serde(default, rename = "blast_radius")]
    pub blast_radius: BlastRadius,
    /// Outcome
    pub outcome: CapsuleOutcome,
    /// Consecutive successes with this gene
    #[serde(default, rename = "success_streak")]
    pub success_streak: Option<u32>,
    /// Runtime environment snapshot
    #[serde(default, rename = "env_fingerprint")]
    pub env_fingerprint: Option<EnvFingerprint>,
    /// LLM model that produced this capsule
    #[serde(default, rename = "model_name")]
    pub model_name: Option<String>,
    /// Content-addressable hash
    #[serde(rename = "asset_id")]
    pub asset_id: String,
    /// Trigger context (optional)
    #[serde(default, rename = "trigger_context")]
    pub trigger_context: Option<TriggerContext>,
    /// ID of reused capsule (if reused)
    #[serde(default, rename = "reused_asset_id")]
    pub reused_asset_id: Option<String>,
    /// Source type: generated, reused, or reference
    #[serde(default, rename = "source_type")]
    pub source_type: CapsuleSourceType,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CapsuleSourceType {
    Generated,
    Reused,
    Reference,
}

impl Default for CapsuleSourceType {
    fn default() -> Self {
        Self::Generated
    }
}

impl GepCapsule {
    /// Create a new GEP Capsule with computed asset_id
    pub fn new(
        id: String,
        trigger: Vec<String>,
        gene: String,
        summary: String,
        diff: String,
        confidence: f32,
    ) -> Result<Self, AssetIdError> {
        let outcome = CapsuleOutcome {
            status: CapsuleStatus::Success,
            score: confidence,
            note: None,
        };

        let mut capsule = Self {
            capsule_type: "Capsule".to_string(),
            schema_version: super::GEP_SCHEMA_VERSION.to_string(),
            id,
            trigger,
            gene,
            summary,
            content: None,
            diff: Some(diff),
            strategy: None,
            confidence,
            blast_radius: BlastRadius::default(),
            outcome,
            success_streak: None,
            env_fingerprint: None,
            model_name: None,
            asset_id: String::new(),
            trigger_context: None,
            reused_asset_id: None,
            source_type: CapsuleSourceType::Generated,
        };

        capsule.asset_id = compute_asset_id(&capsule, &["asset_id"])?;
        Ok(capsule)
    }

    /// Validate the capsule has minimum substance
    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_empty() {
            return Err("Capsule id cannot be empty".to_string());
        }
        if self.gene.is_empty() {
            return Err("Capsule gene cannot be empty".to_string());
        }

        // Check substance requirement: at least one of content, diff, strategy, or code_snippet >= 50 chars
        let has_substance = self
            .content
            .as_ref()
            .map(|c| c.intent.len() + c.rationale.len() + c.outcome.len())
            .unwrap_or(0)
            >= 50
            || self.diff.as_ref().map(|d| d.len()).unwrap_or(0) >= 50
            || self
                .strategy
                .as_ref()
                .map(|s| s.join("").len())
                .unwrap_or(0)
                >= 50;

        if !has_substance {
            return Err("Capsule must have at least 50 characters of substance".to_string());
        }

        Ok(())
    }

    /// Set the content
    pub fn with_content(mut self, content: CapsuleContent) -> Self {
        self.content = Some(content);
        self
    }

    /// Set the strategy
    pub fn with_strategy(mut self, strategy: Vec<String>) -> Self {
        self.strategy = Some(strategy);
        self
    }

    /// Set the blast radius
    pub fn with_blast_radius(mut self, files: usize, lines: usize) -> Self {
        self.blast_radius = BlastRadius { files, lines };
        self
    }

    /// Set the trigger context
    pub fn with_trigger_context(mut self, ctx: TriggerContext) -> Self {
        self.trigger_context = Some(ctx);
        self
    }

    /// Mark as reused capsule
    pub fn as_reused(mut self, reused_id: String) -> Self {
        self.source_type = CapsuleSourceType::Reused;
        self.reused_asset_id = Some(reused_id);
        self
    }
}

/// Convert from Oris core Capsule to GEP Capsule
impl From<&crate::Capsule> for GepCapsule {
    fn from(oris_capsule: &crate::Capsule) -> Self {
        let outcome = CapsuleOutcome {
            status: if oris_capsule.outcome.success {
                CapsuleStatus::Success
            } else {
                CapsuleStatus::Failed
            },
            score: oris_capsule.confidence,
            note: None,
        };

        GepCapsule {
            capsule_type: "Capsule".to_string(),
            schema_version: super::GEP_SCHEMA_VERSION.to_string(),
            id: oris_capsule.id.clone(),
            trigger: vec![], // Placeholder
            gene: oris_capsule.gene_id.clone(),
            summary: format!("Capsule from mutation {}", oris_capsule.mutation_id),
            content: None,
            diff: Some(oris_capsule.diff_hash.clone()),
            strategy: None,
            confidence: oris_capsule.confidence,
            blast_radius: BlastRadius {
                files: oris_capsule.outcome.changed_files.len(),
                lines: oris_capsule.outcome.lines_changed,
            },
            outcome,
            success_streak: None,
            env_fingerprint: Some(EnvFingerprint {
                runtime: Some(oris_capsule.env.rustc_version.clone()),
                os: Some(oris_capsule.env.os.clone()),
                extra: std::collections::HashMap::new(),
            }),
            model_name: None,
            asset_id: oris_capsule.diff_hash.clone(),
            trigger_context: None,
            reused_asset_id: None,
            source_type: CapsuleSourceType::Generated,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capsule_creation() {
        let capsule = GepCapsule::new(
            "capsule_1708123456789".to_string(),
            vec!["timeout".to_string(), "error".to_string()],
            "gene_001".to_string(),
            "Fixed connection timeout issue".to_string(),
            "--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,5 +1,5 @@\n".to_string(),
            0.85,
        )
        .unwrap();

        assert_eq!(capsule.capsule_type, "Capsule");
        assert_eq!(capsule.schema_version, "1.5.0");
        assert_eq!(capsule.outcome.status, CapsuleStatus::Success);
    }

    #[test]
    fn test_capsule_validate() {
        let capsule = GepCapsule::new(
            "capsule_test".to_string(),
            vec!["test".to_string()],
            "gene_test".to_string(),
            "Short".to_string(),
            "short".to_string(), // Too short
            0.5,
        )
        .unwrap();

        assert!(capsule.validate().is_err());
    }

    #[test]
    fn test_capsule_with_content() {
        let capsule = GepCapsule::new(
            "capsule_002".to_string(),
            vec!["error".to_string()],
            "gene_001".to_string(),
            "Fixed bug".to_string(),
            "diff content here that is definitely longer than fifty characters to pass validation"
                .to_string(),
            0.9,
        )
        .unwrap()
        .with_content(CapsuleContent {
            intent: "Fix error handling".to_string(),
            strategy: "Add try-catch block".to_string(),
            scope: "src/api.rs".to_string(),
            changed_files: vec!["src/api.rs".to_string()],
            rationale: "Error was unhandled".to_string(),
            outcome: "Fixed".to_string(),
        });

        assert!(capsule.validate().is_ok());
    }

    #[test]
    fn test_reused_capsule() {
        let capsule = GepCapsule::new(
            "capsule_reused".to_string(),
            vec!["timeout".to_string()],
            "gene_001".to_string(),
            "Applied fix from capsule_001".to_string(),
            "diff content that is definitely longer than fifty characters to pass validation checks".to_string(),
            0.95,
        ).unwrap()
        .as_reused("capsule_original_001".to_string());

        assert_eq!(capsule.source_type, CapsuleSourceType::Reused);
        assert_eq!(
            capsule.reused_asset_id,
            Some("capsule_original_001".to_string())
        );
    }

    #[test]
    fn test_trigger_context() {
        let ctx = TriggerContext {
            prompt: Some("Fix the timeout bug".to_string()),
            reasoning_trace: Some("Analyzed error logs, found timeout in connection".to_string()),
            context_signals: vec!["signal1".to_string()],
            session_id: Some("session_123".to_string()),
            agent_model: Some("claude-sonnet-4".to_string()),
        };

        let capsule = GepCapsule::new(
            "capsule_ctx".to_string(),
            vec!["timeout".to_string()],
            "gene_001".to_string(),
            "Fixed with context".to_string(),
            "diff content that is definitely longer than fifty characters to pass validation requirements".to_string(),
            0.88,
        ).unwrap()
        .with_trigger_context(ctx);

        assert!(capsule.trigger_context.is_some());
        assert_eq!(
            capsule.trigger_context.as_ref().unwrap().agent_model,
            Some("claude-sonnet-4".to_string())
        );
    }
}
