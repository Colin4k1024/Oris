//! Core types for the IPC protocol

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Protocol version
pub const PROTOCOL_VERSION: &str = "1.0";

/// IPC method names
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Evolve,
    Solidify,
    Revert,
    Query,
    List,
    Ping,
}

impl Method {
    pub fn as_str(&self) -> &'static str {
        match self {
            Method::Evolve => "evolve",
            Method::Solidify => "solidify",
            Method::Revert => "revert",
            Method::Query => "query",
            Method::List => "list",
            Method::Ping => "ping",
        }
    }
}

/// Signal type from runtime
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum RuntimeSignalType {
    /// Compiler error (rustc, gcc, etc.)
    CompilerError,
    /// Runtime panic with stack trace
    Panic,
    /// Test failure
    TestFailure,
    /// Runtime warning
    Warning,
    /// Custom signal
    Custom,
}

/// Source location of a signal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceLocation {
    pub file: String,
    pub line: u32,
    pub column: Option<u32>,
}

/// Runtime signal from the harness
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeSignal {
    /// Unique signal ID
    pub id: Uuid,
    /// Signal type
    #[serde(rename = "type")]
    pub signal_type: RuntimeSignalType,
    /// Signal content (error message, stack trace, etc.)
    pub content: String,
    /// Optional source location
    pub location: Option<SourceLocation>,
    /// Signal severity (0.0 - 1.0)
    pub severity: f32,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
}

/// Source tag for Gene traceability
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceTag {
    /// Error type that generated this gene
    pub error_type: String,
    /// User ID who initiated the evolution
    pub user_id: Uuid,
    /// Session ID where the evolution occurred
    pub session_id: Uuid,
    /// When the gene was created
    pub timestamp: DateTime<Utc>,
}

/// Evolution context from Claude Code harness
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvolutionContext {
    /// Current session ID
    pub session_id: Uuid,
    /// Current user ID
    pub user_id: Uuid,
    /// Workspace root path
    pub workspace: String,
    /// Optional user confirmation for low-confidence evolutions
    pub user_confirmation: Option<bool>,
}

/// Gene query for similarity search
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneQuery {
    /// Search pattern (error message, code snippet, etc.)
    pub pattern: String,
    /// Maximum number of results
    pub limit: Option<usize>,
    /// Minimum confidence threshold
    pub min_confidence: Option<f32>,
}

/// A Gene in the pool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gene {
    /// Unique gene ID
    pub id: Uuid,
    /// Content hash for deduplication
    pub content_hash: String,
    /// Gene confidence score (0.0 - 1.0)
    pub confidence: f32,
    /// Ed25519 signature
    pub signature: String,
    /// Source tag for traceability
    pub source_tag: SourceTag,
    /// When the gene was created
    pub created_at: DateTime<Utc>,
    /// Gene metadata
    pub metadata: GeneMetadata,
}

/// Gene metadata
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GeneMetadata {
    /// Programming language
    pub language: Option<String>,
    /// File extensions this gene applies to
    pub file_extensions: Vec<String>,
    /// Tags for categorization
    pub tags: Vec<String>,
}

/// Evolution action recommendation
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvolutionAction {
    /// Solidify the gene to the pool
    Solidify,
    /// Apply once but don't solidify
    ApplyOnce,
    /// Reject this evolution
    Reject,
}

/// Evolution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvolutionResult {
    /// Resulting gene ID (if any)
    pub gene_id: Option<Uuid>,
    /// Confidence score
    pub confidence: f32,
    /// Recommended action
    pub action: EvolutionAction,
    /// Whether auto-revert was triggered
    pub revert_triggered: bool,
    /// Evaluation report summary
    pub evaluation_summary: String,
}

/// Revert reason
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevertReason {
    /// Gene ID to revert
    pub gene_id: Uuid,
    /// Reason for revert
    pub reason: String,
    /// Optional confidence drop percentage
    pub confidence_drop: Option<f32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_method_as_str() {
        assert_eq!(Method::Evolve.as_str(), "evolve");
        assert_eq!(Method::Solidify.as_str(), "solidify");
        assert_eq!(Method::Revert.as_str(), "revert");
        assert_eq!(Method::Query.as_str(), "query");
        assert_eq!(Method::List.as_str(), "list");
    }

    #[test]
    fn test_signal_serialization() {
        let signal = RuntimeSignal {
            id: Uuid::new_v4(),
            signal_type: RuntimeSignalType::CompilerError,
            content: "error[E0502]: borrow of mutable field".to_string(),
            location: Some(SourceLocation {
                file: "src/main.rs".to_string(),
                line: 42,
                column: Some(5),
            }),
            severity: 0.8,
            timestamp: Utc::now(),
        };

        let json = serde_json::to_string(&signal).unwrap();
        let deserialized: RuntimeSignal = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.id, signal.id);
        assert_eq!(deserialized.content, signal.content);
    }
}
