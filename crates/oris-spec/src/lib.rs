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

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_SPEC: &str = r#"
id: example-spec
version: "1.0"
intent: Fix borrow checker error
signals:
  - rust borrow error
constraints:
  - key: crate
    value: oris-kernel
mutation:
  strategy: tighten_borrow_scope
validation:
  - cargo check
"#;

    #[test]
    fn test_spec_compiler_from_yaml() {
        let doc = SpecCompiler::from_yaml(SAMPLE_SPEC).unwrap();
        assert_eq!(doc.id, "example-spec");
        assert_eq!(doc.version, "1.0");
        assert_eq!(doc.signals.len(), 1);
    }

    #[test]
    fn test_spec_compiler_compile() {
        let doc = SpecCompiler::from_yaml(SAMPLE_SPEC).unwrap();
        let plan = SpecCompiler::compile(&doc).unwrap();
        assert!(plan.mutation_intent.id.starts_with("spec-"));
        assert_eq!(plan.validation_profile, "cargo check");
    }

    #[test]
    fn test_spec_compile_empty_id() {
        let doc = SpecDocument {
            id: "".into(),
            version: "1.0".into(),
            intent: "test".into(),
            signals: vec![],
            constraints: vec![],
            mutation: SpecMutation {
                strategy: "test".into(),
            },
            validation: vec![],
        };
        let result = SpecCompiler::compile(&doc);
        assert!(result.is_err());
    }

    #[test]
    fn test_spec_compile_empty_intent() {
        let doc = SpecDocument {
            id: "test".into(),
            version: "1.0".into(),
            intent: "".into(),
            signals: vec![],
            constraints: vec![],
            mutation: SpecMutation {
                strategy: "test".into(),
            },
            validation: vec![],
        };
        let result = SpecCompiler::compile(&doc);
        assert!(result.is_err());
    }

    #[test]
    fn test_spec_compile_empty_strategy() {
        let doc = SpecDocument {
            id: "test".into(),
            version: "1.0".into(),
            intent: "test".into(),
            signals: vec![],
            constraints: vec![],
            mutation: SpecMutation {
                strategy: "".into(),
            },
            validation: vec![],
        };
        let result = SpecCompiler::compile(&doc);
        assert!(result.is_err());
    }

    #[test]
    fn test_default_validation_profile() {
        let doc = SpecDocument {
            id: "test".into(),
            version: "1.0".into(),
            intent: "test".into(),
            signals: vec![],
            constraints: vec![],
            mutation: SpecMutation {
                strategy: "test".into(),
            },
            validation: vec![],
        };
        let plan = SpecCompiler::compile(&doc).unwrap();
        assert_eq!(plan.validation_profile, "spec-default");
    }
}
