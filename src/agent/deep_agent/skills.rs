//! Directory-based skills with SKILL.md and frontmatter (progressive disclosure).
//!
//! Follows the [Agent Skills standard](https://agentskills.io/) and
//! [Deep Agent Skills](https://docs.langchain.com/oss/python/deepagents/skills):
//! each skill is a directory containing a `SKILL.md` with YAML frontmatter
//! (e.g. `name`, `description`) and a body with instructions. At startup only
//! frontmatter is read; when the agent receives a prompt, matching skills are
//! loaded and their content is injected into context.

use std::path::PathBuf;

/// Metadata for one skill parsed from SKILL.md frontmatter.
#[derive(Clone, Debug)]
pub struct SkillMeta {
    /// Directory containing the skill (e.g. `skills/langgraph-docs`).
    pub dir: PathBuf,
    /// Full path to SKILL.md.
    pub skill_md_path: PathBuf,
    /// Skill name from frontmatter.
    pub name: String,
    /// Skill description from frontmatter (used for matching).
    pub description: String,
}

/// Parse YAML-like frontmatter between first `---` and second `---`.
/// Returns (name, description) if both are present. Keys are case-sensitive.
fn parse_frontmatter(content: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = content.splitn(3, "---").collect();
    // content is: [before first ---, between --- and ---, after second ---]
    if parts.len() < 2 {
        return None;
    }
    let block = parts[1].trim();
    let mut name: Option<String> = None;
    let mut description: Option<String> = None;
    for line in block.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            let k = k.trim();
            let v = v.trim().trim_matches('"').trim_matches('\'').to_string();
            match k {
                "name" => name = Some(v),
                "description" => description = Some(v),
                _ => {}
            }
        }
    }
    match (name, description) {
        (Some(n), Some(d)) if !n.is_empty() && !d.is_empty() => Some((n, d)),
        _ => None,
    }
}

/// Load skill index from a list of skill directories. Each directory must
/// contain a `SKILL.md`; only frontmatter is read.
pub fn load_skill_index(skill_dirs: &[PathBuf]) -> Result<Vec<SkillMeta>, std::io::Error> {
    let mut index = Vec::new();
    for dir in skill_dirs {
        let skill_md = dir.join("SKILL.md");
        if !skill_md.is_file() {
            continue;
        }
        let content = std::fs::read_to_string(&skill_md)?;
        if let Some((name, description)) = parse_frontmatter(&content) {
            index.push(SkillMeta {
                dir: dir.clone(),
                skill_md_path: skill_md,
                name,
                description,
            });
        }
    }
    Ok(index)
}

/// Simple matching: a skill matches if the user message contains any word from
/// the skill name or description (after lowercasing), or if the description
/// contains the whole message when short. Non-alphanumeric are treated as separators.
pub fn match_skills(index: &[SkillMeta], user_message: &str) -> Vec<SkillMeta> {
    let msg_lower = user_message.to_lowercase();
    let msg_words: Vec<&str> = msg_lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| s.len() > 1)
        .collect();
    let mut matched = Vec::new();
    for meta in index {
        let name_lower = meta.name.to_lowercase();
        let desc_lower = meta.description.to_lowercase();
        let name_words: Vec<&str> = name_lower
            .split(|c: char| !c.is_alphanumeric())
            .filter(|s| s.len() > 1)
            .collect();
        let desc_words: Vec<&str> = desc_lower
            .split(|c: char| !c.is_alphanumeric())
            .filter(|s| s.len() > 1)
            .collect();
        let mut score = 0u32;
        for w in &msg_words {
            if name_lower.contains(*w) || name_words.iter().any(|nw| *nw == *w) {
                score += 2;
            }
            if desc_lower.contains(*w) || desc_words.iter().any(|dw| *dw == *w) {
                score += 1;
            }
        }
        if msg_lower.len() <= 100
            && (desc_lower.contains(&msg_lower) || name_lower.contains(&msg_lower))
        {
            score += 5;
        }
        if score > 0 {
            matched.push((score, meta.clone()));
        }
    }
    matched.sort_by(|a, b| b.0.cmp(&a.0));
    matched.into_iter().map(|(_, m)| m).collect()
}

/// Load full skill content from SKILL.md: the body after the frontmatter block.
pub fn load_skill_full_content(meta: &SkillMeta) -> Result<String, std::io::Error> {
    let content = std::fs::read_to_string(&meta.skill_md_path)?;
    let parts: Vec<&str> = content.splitn(3, "---").collect();
    let body = if parts.len() >= 3 {
        parts[2].trim()
    } else if parts.len() == 2 {
        parts[1].trim()
    } else {
        content.trim()
    };
    Ok(body.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_frontmatter() {
        let s = r#"---
name: langgraph-docs
description: Use this skill for requests related to LangGraph.
---
# langgraph-docs
Body here."#;
        let (name, desc) = parse_frontmatter(s).unwrap();
        assert_eq!(name, "langgraph-docs");
        assert!(desc.contains("LangGraph"));
    }

    #[test]
    fn test_match_skills() {
        let index = vec![
            SkillMeta {
                dir: PathBuf::from("a"),
                skill_md_path: PathBuf::from("a/SKILL.md"),
                name: "langgraph-docs".to_string(),
                description: "Use for LangGraph documentation.".to_string(),
            },
            SkillMeta {
                dir: PathBuf::from("b"),
                skill_md_path: PathBuf::from("b/SKILL.md"),
                name: "arxiv".to_string(),
                description: "Search arXiv papers.".to_string(),
            },
        ];
        let m = match_skills(&index, "What is langgraph?");
        assert!(!m.is_empty());
        assert_eq!(m[0].name, "langgraph-docs");
    }
}
