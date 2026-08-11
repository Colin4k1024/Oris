//! Render a governed Gene into one portable AgentSkills package.

use oris_experience_contract::{ExperienceBundleV1, LifecycleState};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortableSkillProjection {
    pub skill_name: String,
    pub gene_id: String,
    pub gene_version: u32,
    pub lifecycle: LifecycleState,
    pub installable: bool,
    pub files: BTreeMap<String, String>,
}

pub fn render_portable_skill(
    bundle: &ExperienceBundleV1,
) -> Result<PortableSkillProjection, String> {
    let gene = &bundle.gene;
    if matches!(
        gene.lifecycle,
        LifecycleState::Quarantined | LifecycleState::Revoked
    ) {
        return Err("quarantined or revoked Genes cannot be projected".into());
    }
    let skill_name = slug(&format!("oris-{}", gene.name));
    let description=yaml_single_line(&format!("{} Use for {} tasks when its applicability and safety boundaries match the current environment.",gene.description,gene.task_category));
    let mut body = format!(
        "---\nname: {skill_name}\ndescription: '{description}'\n---\n\n# {}\n\n{}\n\n",
        gene.name, gene.description
    );
    body.push_str("## Applicability\n\n");
    if !gene.applicability.required_signals.is_empty() {
        body.push_str(&format!(
            "Required signals: {}.\n\n",
            gene.applicability.required_signals.join(", ")
        ))
    }
    if !gene.applicability.do_not_use_when.is_empty() {
        body.push_str("Do not use when:\n\n");
        for item in &gene.applicability.do_not_use_when {
            body.push_str(&format!("- {item}\n"))
        }
        body.push('\n')
    }
    body.push_str("## Procedure\n\n");
    for (index, step) in gene.steps.iter().enumerate() {
        body.push_str(&format!("{}. {}", index + 1, step.instruction));
        if step.requires_approval {
            body.push_str(" Obtain the required approval before this step.");
        }
        body.push('\n')
    }
    body.push('\n');
    body.push_str("## Safety and validation\n\nTreat this Skill as a suggestion. Preserve the Agent's permissions, sandbox, approvals, and repository rules.\n\n");
    for operation in &gene.safety.forbidden_operations {
        body.push_str(&format!("- Never: {operation}\n"))
    }
    body.push_str("\nRun these checks and record an Oris UsageReceipt; never claim success without evidence:\n\n");
    for check in &gene.validation.checks {
        body.push_str(&format!(
            "- `{}` ({:?})\n",
            check.command_or_assertion, check.evidence_kind
        ))
    }
    body.push_str("\nRead `references/oris-evidence.json` for provenance and immutable evidence references.\n");
    let mut files = BTreeMap::new();
    files.insert("SKILL.md".into(), body);
    files.insert(
        "references/oris-evidence.json".into(),
        serde_json::to_string_pretty(bundle).map_err(|e| e.to_string())?,
    );
    files.insert("agents/openai.yaml".into(),format!("interface:\n  display_name: \"{}\"\n  short_description: \"{}\"\n  default_prompt: \"Apply this Oris procedure only when applicable, validate it, and record the outcome.\"\n",yaml_double(&gene.name),yaml_double(&truncate(&gene.description,80))));
    Ok(PortableSkillProjection {
        skill_name,
        gene_id: gene.id.clone(),
        gene_version: gene.version,
        lifecycle: gene.lifecycle,
        installable: gene.lifecycle == LifecycleState::Stable,
        files,
    })
}

fn slug(value: &str) -> String {
    let value = value.to_lowercase();
    let mut out = String::new();
    let mut dash = false;
    for c in value.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
            dash = false
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true
        }
    }
    out.trim_matches('-').chars().take(64).collect()
}
fn yaml_single_line(value: &str) -> String {
    value.replace(['\n', '\r'], " ").replace('\'', "''")
}
fn yaml_double(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace(['\n', '\r'], " ")
}
fn truncate(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn candidate_projection_is_reviewable_but_not_installable() {
        let bundle: ExperienceBundleV1 = serde_json::from_str(include_str!(
            "../../../spec/experience/golden/experience-bundle-v1.json"
        ))
        .unwrap();
        let projection = render_portable_skill(&bundle).unwrap();
        assert!(!projection.installable);
        assert!(projection.files["SKILL.md"].contains("record an Oris UsageReceipt"));
        assert!(projection.files.contains_key("agents/openai.yaml"));
    }
    #[test]
    fn stable_projection_is_installable_by_all_agent_skill_runtimes() {
        let mut bundle: ExperienceBundleV1 = serde_json::from_str(include_str!(
            "../../../spec/experience/golden/experience-bundle-v1.json"
        ))
        .unwrap();
        bundle.gene.lifecycle = LifecycleState::Stable;
        bundle.gene.provenance.verified_successes = 3;
        bundle.gene.provenance.distinct_task_contexts = 2;
        bundle.validate().unwrap();
        assert!(render_portable_skill(&bundle).unwrap().installable);
    }
}
