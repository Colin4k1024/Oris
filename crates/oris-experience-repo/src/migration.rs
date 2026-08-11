//! Explicit, loss-aware adapters from pre-v1 experience models.

use chrono::Utc;
use oris_experience_contract::*;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// Convert a legacy genestore Gene without inventing validation evidence.
/// Confidence and counters are retained in metadata; lifecycle remains candidate.
pub fn genestore_gene_to_bundle(gene: &oris_genestore::Gene) -> Result<ExperienceBundleV1, String> {
    let steps: Vec<ProcedureStep> = gene
        .template
        .lines()
        .filter(|line| !line.trim().is_empty())
        .enumerate()
        .map(|(index, line)| ProcedureStep {
            id: format!("legacy-step-{}", index + 1),
            instruction: line.into(),
            tool: None,
            requires_approval: false,
            expected_output: None,
        })
        .collect();
    let checks: Vec<ValidationCheck> = gene
        .validation_steps
        .iter()
        .enumerate()
        .map(|(index, check)| ValidationCheck {
            id: format!("legacy-check-{}", index + 1),
            command_or_assertion: check.clone(),
            evidence_kind: EvidenceKind::Command,
            timeout_seconds: None,
        })
        .collect();
    if steps.is_empty() || checks.is_empty() {
        return Err("legacy Gene has no lossless procedural steps or validation checks".into());
    }
    let mut metadata = BTreeMap::new();
    metadata.insert(
        "legacy_confidence".into(),
        serde_json::json!(gene.confidence),
    );
    metadata.insert(
        "legacy_quality_score".into(),
        serde_json::json!(gene.quality_score),
    );
    metadata.insert("legacy_use_count".into(), serde_json::json!(gene.use_count));
    metadata.insert(
        "legacy_success_count_unverified".into(),
        serde_json::json!(gene.success_count),
    );
    Ok(ExperienceBundleV1 {
        schema_version: EXPERIENCE_BUNDLE_V1.into(),
        gene: GeneV1 {
            id: gene.id.to_string(),
            version: 1,
            name: gene.name.clone(),
            description: gene.description.clone(),
            scope: ExperienceScope::Local,
            task_category: gene
                .tags
                .first()
                .cloned()
                .unwrap_or_else(|| "legacy.unclassified".into()),
            applicability: Applicability {
                required_signals: gene.tags.clone(),
                excluded_signals: vec![],
                environments: vec![],
                project_ids: vec![],
                tenant_ids: vec![],
                do_not_use_when: vec!["Legacy import has not yet accumulated v1 evidence".into()],
            },
            steps,
            tool_requirements: vec![],
            safety: SafetyConstraints {
                suggestion_only: true,
                forbidden_operations: vec![],
                required_approvals: vec![],
                secret_handling: SecretHandling::Redact,
            },
            validation: ValidationContract {
                checks,
                success_condition: ValidationSuccessCondition::All,
            },
            provenance: Provenance {
                source_agent: gene
                    .contributor_id
                    .clone()
                    .unwrap_or_else(|| "legacy-genestore".into()),
                source_run_id: format!("legacy-import:{}", gene.id),
                trace_refs: vec![],
                extractor_version: Some("oris-legacy-adapter/1".into()),
                verified_successes: 0,
                verified_failures: 0,
                distinct_task_contexts: 0,
            },
            lifecycle: LifecycleState::Candidate,
            created_at: gene.created_at,
            updated_at: Utc::now(),
            metadata,
        },
        capsules: vec![],
        usage_receipts: vec![],
    })
}

/// Attach a legacy Capsule as inconclusive evidence. Hashes preserve integrity,
/// but the adapter does not upgrade its historic success counters to v1 proof.
pub fn attach_genestore_capsule(
    bundle: &mut ExperienceBundleV1,
    capsule: &oris_genestore::Capsule,
) -> Result<(), String> {
    if capsule.gene_id.to_string() != bundle.gene.id {
        return Err("capsule references a different Gene".into());
    }
    let content_hash = sha256(&capsule.content);
    let context_hash = sha256(&format!("{}:{}", capsule.env_fingerprint, capsule.id));
    bundle.capsules.push(CapsuleV1 {
        id: capsule.id.to_string(),
        gene_id: bundle.gene.id.clone(),
        gene_version: bundle.gene.version,
        environment_fingerprint: capsule.env_fingerprint.clone(),
        task_context_hash: format!("sha256:{context_hash}"),
        execution_evidence_hash: format!("sha256:{content_hash}"),
        validation: ValidationResult {
            status: OutcomeStatus::Inconclusive,
            checks: vec![],
            summary: Some("Imported legacy capsule; revalidation required".into()),
        },
        artifact_refs: vec![format!("legacy://capsules/{}", capsule.id)],
        redaction: RedactionStatus::ContainsReferencesOnly,
        created_at: capsule.created_at,
    });
    Ok(())
}

fn sha256(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_success_count_is_not_promoted_as_verified_evidence() {
        let gene = oris_genestore::Gene {
            id: uuid::Uuid::new_v4(),
            name: "legacy".into(),
            description: "old".into(),
            tags: vec!["build".into()],
            template: "inspect\npatch".into(),
            preconditions: vec![],
            validation_steps: vec!["cargo test".into()],
            confidence: 0.9,
            use_count: 10,
            success_count: 10,
            quality_score: 0.8,
            created_at: Utc::now(),
            last_used_at: None,
            last_boosted_at: None,
            contributor_id: None,
        };
        let bundle = genestore_gene_to_bundle(&gene).unwrap();
        assert_eq!(bundle.gene.lifecycle, LifecycleState::Candidate);
        assert_eq!(bundle.gene.provenance.verified_successes, 0);
        bundle.validate().unwrap();
    }
}
