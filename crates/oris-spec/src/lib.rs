//! OUSL v0.1 YAML spec contracts.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use oris_evolution::{MutationIntent, MutationTarget, RiskLevel};

pub type SpecId = String;
pub type SpecVersion = String;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpecConstraint {
    pub key: String,
    pub value: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpecMutation {
    pub strategy: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpecDocument {
    pub id: SpecId,
    pub version: SpecVersion,
    pub intent: String,
    #[serde(default)]
    pub signals: Vec<String>,
    #[serde(default)]
    pub constraints: Vec<SpecConstraint>,
    pub mutation: SpecMutation,
    #[serde(default)]
    pub validation: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompiledMutationPlan {
    pub mutation_intent: MutationIntent,
    pub validation_profile: String,
}

#[derive(Debug, Error)]
pub enum SpecCompileError {
    #[error("spec parse error: {0}")]
    Parse(String),
    #[error("invalid spec: {0}")]
    Invalid(String),
}

pub struct SpecCompiler;

impl SpecCompiler {
    pub fn from_yaml(input: &str) -> Result<SpecDocument, SpecCompileError> {
        serde_yaml::from_str(input).map_err(|err| SpecCompileError::Parse(err.to_string()))
    }

    pub fn compile(doc: &SpecDocument) -> Result<CompiledMutationPlan, SpecCompileError> {
        if doc.id.trim().is_empty() {
            return Err(SpecCompileError::Invalid("spec id cannot be empty".into()));
        }
        if doc.intent.trim().is_empty() {
            return Err(SpecCompileError::Invalid(
                "spec intent cannot be empty".into(),
            ));
        }
        if doc.mutation.strategy.trim().is_empty() {
            return Err(SpecCompileError::Invalid(
                "spec mutation strategy cannot be empty".into(),
            ));
        }

        Ok(CompiledMutationPlan {
            mutation_intent: MutationIntent {
                id: format!("spec-{}", doc.id),
                intent: doc.intent.clone(),
                target: MutationTarget::WorkspaceRoot,
                expected_effect: doc.mutation.strategy.clone(),
                risk: RiskLevel::Low,
                signals: doc.signals.clone(),
                spec_id: Some(doc.id.clone()),
            },
            validation_profile: if doc.validation.is_empty() {
                "spec-default".into()
            } else {
                doc.validation.join(",")
            },
        })
    }
}
