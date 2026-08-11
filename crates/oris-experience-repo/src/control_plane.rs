//! Governed storage, retrieval, and lifecycle transitions for experience assets.

use crate::skill_projection::{render_portable_skill, PortableSkillProjection};
use chrono::Utc;
use oris_experience_contract::{
    CapsuleV1, ExperienceBundleV1, ExperienceScope, GeneV1, LifecycleState, OutcomeStatus,
    UsageReceiptV1,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum ControlPlaneError {
    #[error("contract validation failed: {0}")]
    Contract(String),
    #[error("experience asset not found: {0}")]
    NotFound(String),
    #[error("governance permission required")]
    GovernanceRequired,
    #[error("invalid lifecycle transition: {0}")]
    InvalidTransition(String),
    #[error("storage error: {0}")]
    Storage(#[from] rusqlite::Error),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExperienceSearchQuery {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub task_category: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub tenant_id: Option<String>,
    #[serde(default)]
    pub available_tools: Vec<String>,
    #[serde(default)]
    pub environment: BTreeMap<String, serde_json::Value>,
    #[serde(default = "default_search_limit")]
    pub limit: usize,
    #[serde(default)]
    pub cursor: Option<String>,
}

fn default_search_limit() -> usize {
    10
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperienceSearchResult {
    pub gene: GeneV1,
    pub score: f64,
    pub match_reasons: Vec<String>,
    pub applicability_boundaries: Vec<String>,
    pub do_not_use_when: Vec<String>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UseSession {
    pub id: String,
    pub gene_id: String,
    pub gene_version: u32,
    pub agent_id: String,
    pub run_id: String,
    pub task_context_hash: String,
    pub started_at: chrono::DateTime<Utc>,
}

/// SQLite-backed control plane. Genes are immutable per `(id, version)`; outcome
/// data is stored separately and folded into a new governed projection.
pub struct ExperienceControlPlane {
    conn: Connection,
}

impl ExperienceControlPlane {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ControlPlaneError> {
        let conn = Connection::open(path)?;
        let store = Self { conn };
        store.init_schema()?;
        Ok(store)
    }

    pub fn memory() -> Result<Self, ControlPlaneError> {
        let store = Self {
            conn: Connection::open_in_memory()?,
        };
        store.init_schema()?;
        Ok(store)
    }

    fn init_schema(&self) -> Result<(), ControlPlaneError> {
        self.conn.execute_batch(
            r#"
            PRAGMA foreign_keys = ON;
            CREATE TABLE IF NOT EXISTS experience_genes (
                gene_id TEXT NOT NULL,
                version INTEGER NOT NULL,
                lifecycle TEXT NOT NULL,
                task_category TEXT NOT NULL,
                scope TEXT NOT NULL,
                payload TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                PRIMARY KEY (gene_id, version)
            );
            CREATE TABLE IF NOT EXISTS experience_capsules (
                capsule_id TEXT PRIMARY KEY,
                gene_id TEXT NOT NULL,
                gene_version INTEGER NOT NULL,
                task_context_hash TEXT NOT NULL,
                payload TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS experience_usage_receipts (
                receipt_id TEXT PRIMARY KEY,
                gene_id TEXT NOT NULL,
                gene_version INTEGER NOT NULL,
                task_context_hash TEXT NOT NULL,
                outcome TEXT NOT NULL,
                payload TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS experience_use_sessions (
                session_id TEXT PRIMARY KEY,
                gene_id TEXT NOT NULL,
                gene_version INTEGER NOT NULL,
                agent_id TEXT NOT NULL,
                run_id TEXT NOT NULL,
                task_context_hash TEXT NOT NULL,
                started_at TEXT NOT NULL,
                completed_at TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_experience_gene_filter
              ON experience_genes(lifecycle, task_category, scope);
            CREATE INDEX IF NOT EXISTS idx_experience_receipt_gene
              ON experience_usage_receipts(gene_id, gene_version, created_at);
            "#,
        )?;
        Ok(())
    }

    pub fn propose(&self, bundle: &ExperienceBundleV1) -> Result<GeneV1, ControlPlaneError> {
        bundle
            .validate()
            .map_err(|e| ControlPlaneError::Contract(e.to_string()))?;
        let mut gene = bundle.gene.clone();
        // Proposals never import elevated lifecycle or sharing scope implicitly.
        gene.lifecycle = LifecycleState::Candidate;
        if matches!(gene.scope, ExperienceScope::Team | ExperienceScope::Network) {
            gene.scope = ExperienceScope::Local;
        }
        gene.updated_at = Utc::now();
        self.upsert_gene(&gene)?;
        for capsule in &bundle.capsules {
            self.insert_capsule(capsule)?;
        }
        for receipt in &bundle.usage_receipts {
            self.record_outcome(receipt, None)?;
        }
        Ok(gene)
    }

    fn upsert_gene(&self, gene: &GeneV1) -> Result<(), ControlPlaneError> {
        self.conn.execute(
            r#"INSERT INTO experience_genes
               (gene_id, version, lifecycle, task_category, scope, payload, created_at, updated_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
               ON CONFLICT(gene_id, version) DO UPDATE SET
                 lifecycle=excluded.lifecycle, task_category=excluded.task_category,
                 scope=excluded.scope, payload=excluded.payload, updated_at=excluded.updated_at"#,
            params![
                gene.id,
                gene.version,
                enum_json(&gene.lifecycle)?,
                gene.task_category,
                enum_json(&gene.scope)?,
                serde_json::to_string(gene)?,
                gene.created_at.to_rfc3339(),
                gene.updated_at.to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn get_gene(&self, id: &str, version: Option<u32>) -> Result<GeneV1, ControlPlaneError> {
        let payload: Option<String> = if let Some(version) = version {
            self.conn
                .query_row(
                    "SELECT payload FROM experience_genes WHERE gene_id=?1 AND version=?2",
                    params![id, version],
                    |r| r.get(0),
                )
                .optional()?
        } else {
            self.conn.query_row("SELECT payload FROM experience_genes WHERE gene_id=?1 ORDER BY version DESC LIMIT 1",
                params![id], |r| r.get(0)).optional()?
        };
        payload
            .map(|p| serde_json::from_str(&p))
            .transpose()?
            .ok_or_else(|| ControlPlaneError::NotFound(id.to_string()))
    }

    pub fn get_capsule(&self, id: &str) -> Result<CapsuleV1, ControlPlaneError> {
        let payload: Option<String> = self
            .conn
            .query_row(
                "SELECT payload FROM experience_capsules WHERE capsule_id=?1",
                params![id],
                |r| r.get(0),
            )
            .optional()?;
        payload
            .map(|p| serde_json::from_str(&p))
            .transpose()?
            .ok_or_else(|| ControlPlaneError::NotFound(id.to_string()))
    }

    pub fn bundle(
        &self,
        id: &str,
        version: Option<u32>,
    ) -> Result<ExperienceBundleV1, ControlPlaneError> {
        let gene = self.get_gene(id, version)?;
        let capsules = self.load_payloads::<CapsuleV1>(
            "SELECT payload FROM experience_capsules WHERE gene_id=?1 AND gene_version=?2 ORDER BY created_at", &gene.id, gene.version)?;
        let usage_receipts = self.load_payloads::<UsageReceiptV1>(
            "SELECT payload FROM experience_usage_receipts WHERE gene_id=?1 AND gene_version=?2 ORDER BY created_at", &gene.id, gene.version)?;
        Ok(ExperienceBundleV1 {
            schema_version: oris_experience_contract::EXPERIENCE_BUNDLE_V1.into(),
            gene,
            capsules,
            usage_receipts,
        })
    }

    pub fn skill_projection(
        &self,
        id: &str,
        version: Option<u32>,
    ) -> Result<PortableSkillProjection, ControlPlaneError> {
        render_portable_skill(&self.bundle(id, version)?)
            .map_err(ControlPlaneError::InvalidTransition)
    }

    fn load_payloads<T: serde::de::DeserializeOwned>(
        &self,
        sql: &str,
        id: &str,
        version: u32,
    ) -> Result<Vec<T>, ControlPlaneError> {
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map(params![id, version], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(serde_json::from_str(&row?)?);
        }
        Ok(out)
    }

    pub fn search(
        &self,
        query: &ExperienceSearchQuery,
    ) -> Result<Vec<ExperienceSearchResult>, ControlPlaneError> {
        let offset = query
            .cursor
            .as_deref()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0);
        let mut stmt = self.conn.prepare(
            "SELECT payload FROM experience_genes WHERE lifecycle IN ('candidate','stable') ORDER BY updated_at DESC"
        )?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let query_tokens = tokenize(&query.text);
        let mut results = Vec::new();
        for row in rows {
            let gene: GeneV1 = serde_json::from_str(&row?)?;
            if !structurally_compatible(&gene, query) {
                continue;
            }
            let document = format!(
                "{} {} {} {}",
                gene.name,
                gene.description,
                gene.task_category,
                gene.applicability.required_signals.join(" ")
            );
            let doc_tokens = tokenize(&document);
            let lexical = bm25_lite(&query_tokens, &doc_tokens);
            let semantic = hashed_cosine(&query_tokens, &doc_tokens);
            if !query_tokens.is_empty() && lexical == 0.0 && semantic < 0.12 {
                continue;
            }
            let success_rate =
                if gene.provenance.verified_successes + gene.provenance.verified_failures == 0 {
                    0.5
                } else {
                    gene.provenance.verified_successes as f64
                        / (gene.provenance.verified_successes + gene.provenance.verified_failures)
                            as f64
                };
            let environment_match = environment_score(&gene, query);
            let lifecycle_boost = if gene.lifecycle == LifecycleState::Stable {
                1.0
            } else {
                0.6
            };
            let score = 0.35 * lexical
                + 0.25 * semantic
                + 0.2 * success_rate
                + 0.15 * environment_match
                + 0.05 * lifecycle_boost;
            let mut reasons = Vec::new();
            if lexical > 0.0 {
                reasons.push("keyword/BM25 signals matched".into());
            }
            if semantic >= 0.12 {
                reasons.push("semantic token-vector matched".into());
            }
            if environment_match >= 0.75 {
                reasons.push("environment constraints matched".into());
            }
            if gene.lifecycle == LifecycleState::Stable {
                reasons.push("verified stable experience".into());
            }
            results.push(ExperienceSearchResult {
                applicability_boundaries: gene
                    .applicability
                    .environments
                    .iter()
                    .map(|v| format!("{} {:?} {}", v.key, v.operator, v.value))
                    .collect(),
                do_not_use_when: gene.applicability.do_not_use_when.clone(),
                gene,
                score,
                match_reasons: reasons,
                next_cursor: None,
            });
        }
        results.sort_by(|a, b| b.score.total_cmp(&a.score));
        let limit = query.limit.clamp(1, 100);
        let total = results.len();
        let mut page: Vec<_> = results.into_iter().skip(offset).take(limit).collect();
        if offset + page.len() < total {
            let next = (offset + page.len()).to_string();
            for item in &mut page {
                item.next_cursor = Some(next.clone());
            }
        }
        Ok(page)
    }

    pub fn begin_use(
        &self,
        gene_id: &str,
        version: u32,
        agent_id: &str,
        run_id: &str,
        task_context_hash: &str,
    ) -> Result<UseSession, ControlPlaneError> {
        self.get_gene(gene_id, Some(version))?;
        let session = UseSession {
            id: uuid::Uuid::new_v4().to_string(),
            gene_id: gene_id.into(),
            gene_version: version,
            agent_id: agent_id.into(),
            run_id: run_id.into(),
            task_context_hash: task_context_hash.into(),
            started_at: Utc::now(),
        };
        self.conn.execute("INSERT INTO experience_use_sessions (session_id,gene_id,gene_version,agent_id,run_id,task_context_hash,started_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![session.id, session.gene_id, session.gene_version, session.agent_id, session.run_id, session.task_context_hash, session.started_at.to_rfc3339()])?;
        Ok(session)
    }

    pub fn record_outcome(
        &self,
        receipt: &UsageReceiptV1,
        capsule: Option<&CapsuleV1>,
    ) -> Result<GeneV1, ControlPlaneError> {
        receipt
            .validate()
            .map_err(|e| ControlPlaneError::Contract(e.to_string()))?;
        let mut gene = self.get_gene(&receipt.gene_id, Some(receipt.gene_version))?;
        if let Some(capsule) = capsule {
            capsule
                .validate()
                .map_err(|e| ControlPlaneError::Contract(e.to_string()))?;
            if capsule.gene_id != receipt.gene_id
                || capsule.gene_version != receipt.gene_version
                || capsule.task_context_hash != receipt.task_context_hash
            {
                return Err(ControlPlaneError::Contract(
                    "capsule and receipt references differ".into(),
                ));
            }
            self.insert_capsule(capsule)?;
        }
        self.conn.execute("INSERT INTO experience_usage_receipts (receipt_id,gene_id,gene_version,task_context_hash,outcome,payload,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![receipt.id, receipt.gene_id, receipt.gene_version, receipt.task_context_hash, enum_json(&receipt.outcome)?, serde_json::to_string(receipt)?, receipt.created_at.to_rfc3339()])?;
        self.conn.execute("UPDATE experience_use_sessions SET completed_at=?1 WHERE gene_id=?2 AND gene_version=?3 AND run_id=?4",
            params![Utc::now().to_rfc3339(), receipt.gene_id, receipt.gene_version, receipt.run_id])?;

        let (successes, failures, contexts, consecutive_failures) =
            self.outcome_stats(&gene.id, gene.version)?;
        gene.provenance.verified_successes = successes;
        gene.provenance.verified_failures = failures;
        gene.provenance.distinct_task_contexts = contexts;
        gene.updated_at = Utc::now();
        if receipt.outcome == OutcomeStatus::SafetyFailed {
            gene.lifecycle = LifecycleState::Quarantined;
        } else if consecutive_failures >= 2 {
            gene.lifecycle = LifecycleState::Candidate;
        } else if successes >= 3 && contexts >= 2 && failures == 0 {
            gene.lifecycle = LifecycleState::Stable;
        }
        self.upsert_gene(&gene)?;
        Ok(gene)
    }

    pub fn promote(
        &self,
        id: &str,
        version: u32,
        scope: ExperienceScope,
        governance: bool,
    ) -> Result<GeneV1, ControlPlaneError> {
        if !governance {
            return Err(ControlPlaneError::GovernanceRequired);
        }
        let mut gene = self.get_gene(id, Some(version))?;
        if gene.lifecycle != LifecycleState::Stable {
            return Err(ControlPlaneError::InvalidTransition(
                "only stable genes can be published".into(),
            ));
        }
        gene.scope = scope;
        gene.updated_at = Utc::now();
        self.upsert_gene(&gene)?;
        Ok(gene)
    }

    pub fn revoke(
        &self,
        id: &str,
        version: u32,
        quarantine: bool,
        governance: bool,
    ) -> Result<GeneV1, ControlPlaneError> {
        if !governance {
            return Err(ControlPlaneError::GovernanceRequired);
        }
        let mut gene = self.get_gene(id, Some(version))?;
        gene.lifecycle = if quarantine {
            LifecycleState::Quarantined
        } else {
            LifecycleState::Revoked
        };
        gene.updated_at = Utc::now();
        self.upsert_gene(&gene)?;
        Ok(gene)
    }

    fn insert_capsule(&self, capsule: &CapsuleV1) -> Result<(), ControlPlaneError> {
        self.conn.execute("INSERT OR IGNORE INTO experience_capsules (capsule_id,gene_id,gene_version,task_context_hash,payload,created_at) VALUES (?1,?2,?3,?4,?5,?6)",
            params![capsule.id, capsule.gene_id, capsule.gene_version, capsule.task_context_hash, serde_json::to_string(capsule)?, capsule.created_at.to_rfc3339()])?;
        Ok(())
    }

    fn outcome_stats(
        &self,
        id: &str,
        version: u32,
    ) -> Result<(u64, u64, u64, u64), ControlPlaneError> {
        let mut stmt = self.conn.prepare("SELECT outcome, task_context_hash FROM experience_usage_receipts WHERE gene_id=?1 AND gene_version=?2 ORDER BY created_at")?;
        let rows = stmt.query_map(params![id, version], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        let mut successes = 0;
        let mut failures = 0;
        let mut contexts = HashSet::new();
        let mut consecutive = 0;
        for row in rows {
            let (outcome, context) = row?;
            contexts.insert(context);
            match outcome.as_str() {
                "succeeded" => {
                    successes += 1;
                    consecutive = 0;
                }
                "failed" | "safety_failed" => {
                    failures += 1;
                    consecutive += 1;
                }
                _ => {}
            }
        }
        Ok((successes, failures, contexts.len() as u64, consecutive))
    }
}

fn enum_json<T: Serialize>(value: &T) -> Result<String, serde_json::Error> {
    Ok(serde_json::to_string(value)?.trim_matches('"').to_string())
}

fn tokenize(value: &str) -> Vec<String> {
    value
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
        .filter(|v| v.len() > 1)
        .map(str::to_owned)
        .collect()
}

fn bm25_lite(query: &[String], document: &[String]) -> f64 {
    if query.is_empty() {
        return 0.5;
    }
    let counts = document.iter().fold(HashMap::new(), |mut acc, token| {
        *acc.entry(token).or_insert(0usize) += 1;
        acc
    });
    let dl = document.len().max(1) as f64;
    let score: f64 = query
        .iter()
        .map(|token| {
            let tf = *counts.get(token).unwrap_or(&0) as f64;
            if tf == 0.0 {
                0.0
            } else {
                tf * 2.2 / (tf + 1.2 * (0.25 + 0.75 * dl / 30.0))
            }
        })
        .sum();
    (score / query.len() as f64).min(1.0)
}

fn hashed_cosine(a: &[String], b: &[String]) -> f64 {
    const N: usize = 64;
    fn vector(tokens: &[String]) -> [f64; N] {
        let mut v = [0.0; N];
        for t in tokens {
            let mut h = 1469598103934665603u64;
            for byte in t.bytes() {
                h ^= byte as u64;
                h = h.wrapping_mul(1099511628211)
            }
            v[h as usize % N] += 1.0;
        }
        v
    }
    let va = vector(a);
    let vb = vector(b);
    let dot: f64 = va.iter().zip(vb).map(|(x, y)| x * y).sum();
    let na: f64 = va.iter().map(|x| x * x).sum::<f64>().sqrt();
    let nb: f64 = vb.iter().map(|x| x * x).sum::<f64>().sqrt();
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na * nb)
    }
}

fn structurally_compatible(gene: &GeneV1, query: &ExperienceSearchQuery) -> bool {
    if let Some(category) = &query.task_category {
        if gene.task_category != *category {
            return false;
        }
    }
    if let Some(project) = &query.project_id {
        if !gene.applicability.project_ids.is_empty()
            && !gene.applicability.project_ids.contains(project)
        {
            return false;
        }
    }
    if let Some(tenant) = &query.tenant_id {
        if !gene.applicability.tenant_ids.is_empty()
            && !gene.applicability.tenant_ids.contains(tenant)
        {
            return false;
        }
    }
    let tools: HashSet<String> = query
        .available_tools
        .iter()
        .flat_map(|tool| normalized_tool_capabilities(tool))
        .collect();
    if !gene
        .tool_requirements
        .iter()
        .all(|required| tools.is_empty() || tools.contains(&required.name))
    {
        return false;
    }
    environment_score(gene, query) > 0.0
}

/// Normalize harness-specific tool names into portable procedural capabilities.
/// A Gene records what must be possible, while each Agent reports the concrete
/// tool names it can use. Exact names remain supported for specialized tools.
fn normalized_tool_capabilities(tool: &str) -> Vec<String> {
    let normalized = tool.to_ascii_lowercase().replace('-', "_");
    let mut capabilities = vec![tool.to_owned(), normalized.clone()];
    match normalized.as_str() {
        "bash"
        | "exec_command"
        | "run_command"
        | "run_terminal_command"
        | "shell"
        | "cargo"
        | "pytest"
        | "unittest"
        | "test_runner" => {
            capabilities.push("test-runner".into());
        }
        "read" | "read_file" | "rg" | "ripgrep" | "grep" | "code_search" | "search_files" => {
            capabilities.push("code-search".into());
        }
        "edit" | "apply_patch" | "search_replace" | "write_file" | "editor" => {
            capabilities.push("editor".into());
        }
        _ => {}
    }
    capabilities
}

fn environment_score(gene: &GeneV1, query: &ExperienceSearchQuery) -> f64 {
    if gene.applicability.environments.is_empty() {
        return 1.0;
    }
    let matched = gene
        .applicability
        .environments
        .iter()
        .filter(|constraint| {
            let Some(actual) = query.environment.get(&constraint.key) else {
                return false;
            };
            match constraint.operator {
                oris_experience_contract::ConstraintOperator::Equals => actual == &constraint.value,
                oris_experience_contract::ConstraintOperator::NotEquals => {
                    actual != &constraint.value
                }
                oris_experience_contract::ConstraintOperator::Contains => actual
                    .to_string()
                    .contains(constraint.value.as_str().unwrap_or("")),
                oris_experience_contract::ConstraintOperator::Exists => true,
                oris_experience_contract::ConstraintOperator::Semver => actual
                    .as_str()
                    .zip(constraint.value.as_str())
                    .map(|(a, b)| a.starts_with(b.trim_end_matches(".*")))
                    .unwrap_or(false),
            }
        })
        .count();
    matched as f64 / gene.applicability.environments.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use oris_experience_contract::AdoptionStatus;

    fn fixture() -> ExperienceBundleV1 {
        serde_json::from_str(include_str!(
            "../../../spec/experience/golden/experience-bundle-v1.json"
        ))
        .unwrap()
    }

    fn receipt(
        bundle: &ExperienceBundleV1,
        id: &str,
        context: &str,
        outcome: OutcomeStatus,
    ) -> UsageReceiptV1 {
        UsageReceiptV1 {
            id: id.into(),
            gene_id: bundle.gene.id.clone(),
            gene_version: 1,
            agent_id: "codex".into(),
            run_id: format!("run-{id}"),
            task_context_hash: context.into(),
            adoption: AdoptionStatus::Adopted,
            applied_step_ids: vec!["patch".into()],
            outcome,
            failure_reason: (outcome != OutcomeStatus::Succeeded)
                .then(|| "validation failed".into()),
            test_evidence_refs: (outcome == OutcomeStatus::Succeeded)
                .then(|| format!("artifact://test/{id}"))
                .into_iter()
                .collect(),
            cost: None,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn safety_failure_quarantines_immediately() {
        let store = ExperienceControlPlane::memory().unwrap();
        let bundle = fixture();
        store.propose(&bundle).unwrap();
        let receipt = UsageReceiptV1 {
            id: "receipt-safe".into(),
            gene_id: bundle.gene.id.clone(),
            gene_version: 1,
            agent_id: "codex".into(),
            run_id: "run-safe".into(),
            task_context_hash: "sha256:unsafe".into(),
            adoption: AdoptionStatus::Adopted,
            applied_step_ids: vec!["patch".into()],
            outcome: OutcomeStatus::SafetyFailed,
            failure_reason: Some("secret leak".into()),
            test_evidence_refs: vec![],
            cost: None,
            created_at: Utc::now(),
        };
        assert_eq!(
            store.record_outcome(&receipt, None).unwrap().lifecycle,
            LifecycleState::Quarantined
        );
    }

    #[test]
    fn promotes_only_after_three_successes_in_two_contexts() {
        let store = ExperienceControlPlane::memory().unwrap();
        let bundle = fixture();
        store.propose(&bundle).unwrap();
        assert_eq!(
            store
                .record_outcome(
                    &receipt(&bundle, "one", "ctx-a", OutcomeStatus::Succeeded),
                    None
                )
                .unwrap()
                .lifecycle,
            LifecycleState::Candidate
        );
        assert_eq!(
            store
                .record_outcome(
                    &receipt(&bundle, "two", "ctx-a", OutcomeStatus::Succeeded),
                    None
                )
                .unwrap()
                .lifecycle,
            LifecycleState::Candidate
        );
        let gene = store
            .record_outcome(
                &receipt(&bundle, "three", "ctx-b", OutcomeStatus::Succeeded),
                None,
            )
            .unwrap();
        assert_eq!(gene.lifecycle, LifecycleState::Stable);
        assert_eq!(gene.provenance.verified_successes, 3);
        assert_eq!(gene.provenance.distinct_task_contexts, 2);
    }

    #[test]
    fn two_consecutive_failures_demote_a_stable_gene() {
        let store = ExperienceControlPlane::memory().unwrap();
        let bundle = fixture();
        store.propose(&bundle).unwrap();
        for (id, ctx) in [("one", "ctx-a"), ("two", "ctx-a"), ("three", "ctx-b")] {
            store
                .record_outcome(&receipt(&bundle, id, ctx, OutcomeStatus::Succeeded), None)
                .unwrap();
        }
        assert_eq!(
            store
                .record_outcome(
                    &receipt(&bundle, "fail-one", "ctx-c", OutcomeStatus::Failed),
                    None
                )
                .unwrap()
                .lifecycle,
            LifecycleState::Stable
        );
        assert_eq!(
            store
                .record_outcome(
                    &receipt(&bundle, "fail-two", "ctx-d", OutcomeStatus::Failed),
                    None
                )
                .unwrap()
                .lifecycle,
            LifecycleState::Candidate
        );
    }

    #[test]
    fn search_applies_environment_and_tool_filters_before_ranking() {
        let store = ExperienceControlPlane::memory().unwrap();
        let bundle = fixture();
        store.propose(&bundle).unwrap();
        let compatible = ExperienceSearchQuery {
            text: "transient rust http timeout".into(),
            available_tools: vec!["cargo".into()],
            environment: BTreeMap::from([("language".into(), serde_json::json!("rust"))]),
            ..Default::default()
        };
        let results = store.search(&compatible).unwrap();
        assert_eq!(results.len(), 1);
        assert!(!results[0].do_not_use_when.is_empty());
        let incompatible = ExperienceSearchQuery {
            environment: BTreeMap::from([("language".into(), serde_json::json!("python"))]),
            ..compatible
        };
        assert!(store.search(&incompatible).unwrap().is_empty());
    }

    #[test]
    fn search_normalizes_cross_agent_tool_capabilities() {
        let store = ExperienceControlPlane::memory().unwrap();
        let mut bundle = fixture();
        bundle.gene.tool_requirements = vec![
            oris_experience_contract::ToolRequirement {
                name: "test-runner".into(),
                minimum_version: None,
                required_permissions: vec!["execute-tests".into()],
            },
            oris_experience_contract::ToolRequirement {
                name: "code-search".into(),
                minimum_version: None,
                required_permissions: vec!["read-source".into()],
            },
            oris_experience_contract::ToolRequirement {
                name: "editor".into(),
                minimum_version: None,
                required_permissions: vec!["modify-source".into()],
            },
        ];
        store.propose(&bundle).unwrap();
        let grok_tools = ExperienceSearchQuery {
            text: "transient rust http timeout".into(),
            available_tools: vec![
                "run_command".into(),
                "read_file".into(),
                "apply_patch".into(),
            ],
            environment: BTreeMap::from([("language".into(), serde_json::json!("rust"))]),
            ..Default::default()
        };
        assert_eq!(store.search(&grok_tools).unwrap().len(), 1);
    }
}
