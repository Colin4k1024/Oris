//! oris-genestore/src/store.rs
//!
//! `GeneStore` trait + `SqliteGeneStore` implementation.

use crate::types::{Capsule, Gene, GeneMatch, GeneQuery};
use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use rusqlite::{params, Connection};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

// ─────────────────────────────────────────────────────────────────────────────
// Trait
// ─────────────────────────────────────────────────────────────────────────────

#[async_trait]
pub trait GeneStore: Send + Sync {
    // ── Genes ─────────────────────────────────────────────────────────────────
    async fn upsert_gene(&self, gene: &Gene) -> Result<()>;
    async fn get_gene(&self, id: Uuid) -> Result<Option<Gene>>;
    async fn delete_gene(&self, id: Uuid) -> Result<()>;

    /// Search Genes by tag overlap + confidence; returns ranked matches.
    async fn search_genes(&self, query: &GeneQuery) -> Result<Vec<GeneMatch>>;

    /// Apply confidence decay to all Genes (call periodically or per-query).
    async fn decay_all(&self) -> Result<()>;

    /// Record the outcome of a Gene reuse and update confidence + counters.
    async fn record_gene_outcome(&self, id: Uuid, success: bool) -> Result<()>;

    /// Return Genes below the stale threshold — candidates for re-evolution.
    async fn stale_genes(&self) -> Result<Vec<Gene>>;

    // ── Capsules ──────────────────────────────────────────────────────────────
    async fn upsert_capsule(&self, capsule: &Capsule) -> Result<()>;
    async fn get_capsule(&self, id: Uuid) -> Result<Option<Capsule>>;
    async fn capsules_for_gene(&self, gene_id: Uuid) -> Result<Vec<Capsule>>;
    async fn record_capsule_outcome(
        &self,
        id: Uuid,
        success: bool,
        replay_run_id: Option<Uuid>,
    ) -> Result<()>;
}

// ─────────────────────────────────────────────────────────────────────────────
// SQLite implementation
// ─────────────────────────────────────────────────────────────────────────────

pub struct SqliteGeneStore {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteGeneStore {
    /// Open (or create) the store at `path`. Use `":memory:"` for tests.
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        // WAL mode: readers don't block writers, writers don't block readers.
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS genes (
                id               TEXT PRIMARY KEY,
                name             TEXT NOT NULL,
                description      TEXT NOT NULL,
                tags_json        TEXT NOT NULL,
                template         TEXT NOT NULL,
                preconditions_json   TEXT NOT NULL,
                validation_steps_json TEXT NOT NULL,
                confidence       REAL NOT NULL DEFAULT 0.70,
                use_count        INTEGER NOT NULL DEFAULT 0,
                success_count    INTEGER NOT NULL DEFAULT 0,
                quality_score    REAL NOT NULL DEFAULT 0.0,
                created_at       TEXT NOT NULL,
                last_used_at     TEXT,
                last_boosted_at  TEXT
            );

            CREATE TABLE IF NOT EXISTS gene_tags (
                gene_id TEXT NOT NULL REFERENCES genes(id) ON DELETE CASCADE,
                tag     TEXT NOT NULL,
                PRIMARY KEY (gene_id, tag)
            );
            CREATE INDEX IF NOT EXISTS idx_gene_tags_tag ON gene_tags(tag);

            CREATE INDEX IF NOT EXISTS idx_genes_confidence ON genes(confidence);

            CREATE TABLE IF NOT EXISTS capsules (
                id                  TEXT PRIMARY KEY,
                gene_id             TEXT NOT NULL REFERENCES genes(id) ON DELETE CASCADE,
                content             TEXT NOT NULL,
                env_fingerprint     TEXT NOT NULL,
                quality_score       REAL NOT NULL DEFAULT 0.0,
                confidence          REAL NOT NULL DEFAULT 0.80,
                use_count           INTEGER NOT NULL DEFAULT 0,
                success_count       INTEGER NOT NULL DEFAULT 0,
                last_replay_run_id  TEXT,
                created_at          TEXT NOT NULL,
                last_used_at        TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_capsules_gene_id ON capsules(gene_id);
            CREATE INDEX IF NOT EXISTS idx_capsules_confidence ON capsules(confidence);
        "#,
        )?;
        Ok(())
    }
}

#[async_trait]
impl GeneStore for SqliteGeneStore {
    async fn upsert_gene(&self, gene: &Gene) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            r#"INSERT INTO genes
               (id, name, description, tags_json, template,
                preconditions_json, validation_steps_json,
                confidence, use_count, success_count, quality_score,
                created_at, last_used_at, last_boosted_at)
               VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)
               ON CONFLICT(id) DO UPDATE SET
                 name=excluded.name, description=excluded.description,
                 tags_json=excluded.tags_json, template=excluded.template,
                 preconditions_json=excluded.preconditions_json,
                 validation_steps_json=excluded.validation_steps_json,
                 confidence=excluded.confidence, use_count=excluded.use_count,
                 success_count=excluded.success_count,
                 quality_score=excluded.quality_score,
                 last_used_at=excluded.last_used_at,
                 last_boosted_at=excluded.last_boosted_at"#,
            params![
                gene.id.to_string(),
                gene.name,
                gene.description,
                serde_json::to_string(&gene.tags)?,
                gene.template,
                serde_json::to_string(&gene.preconditions)?,
                serde_json::to_string(&gene.validation_steps)?,
                gene.confidence,
                gene.use_count as i64,
                gene.success_count as i64,
                gene.quality_score,
                gene.created_at.to_rfc3339(),
                gene.last_used_at.map(|d| d.to_rfc3339()),
                gene.last_boosted_at.map(|d| d.to_rfc3339()),
            ],
        )?;

        // Rebuild tag index for this gene.
        conn.execute(
            "DELETE FROM gene_tags WHERE gene_id=?1",
            params![gene.id.to_string()],
        )?;
        for tag in &gene.tags {
            conn.execute(
                "INSERT OR IGNORE INTO gene_tags (gene_id, tag) VALUES (?1, ?2)",
                params![gene.id.to_string(), tag],
            )?;
        }
        Ok(())
    }

    async fn get_gene(&self, id: Uuid) -> Result<Option<Gene>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id,name,description,tags_json,template,
                    preconditions_json,validation_steps_json,
                    confidence,use_count,success_count,quality_score,
                    created_at,last_used_at,last_boosted_at
             FROM genes WHERE id=?1",
        )?;
        let mut rows = stmt.query(params![id.to_string()])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row_to_gene(row)?))
        } else {
            Ok(None)
        }
    }

    async fn delete_gene(&self, id: Uuid) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM genes WHERE id=?1", params![id.to_string()])?;
        Ok(())
    }

    async fn search_genes(&self, query: &GeneQuery) -> Result<Vec<GeneMatch>> {
        let conn = self.conn.lock().unwrap();

        // Step 1: tag-filter — fetch candidates that have at least one required tag,
        // or all genes if no required tags specified.
        let candidate_ids: Vec<String> = if query.required_tags.is_empty() {
            let mut stmt = conn.prepare(
                "SELECT id FROM genes WHERE confidence >= ?1 ORDER BY confidence DESC LIMIT ?2",
            )?;
            let rows = stmt
                .query_map(params![query.min_confidence, query.limit as i64 * 3], |r| {
                    r.get(0)
                })?;
            rows.filter_map(|r| r.ok()).collect()
        } else {
            // Genes that match ALL required tags via intersection.
            let placeholders = query
                .required_tags
                .iter()
                .enumerate()
                .map(|(i, _)| format!("?{}", i + 2))
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!(
                "SELECT gene_id FROM gene_tags \
                 WHERE tag IN ({}) \
                 GROUP BY gene_id HAVING COUNT(DISTINCT tag) = ?1 \
                 LIMIT {}",
                placeholders,
                query.limit * 3
            );
            let mut stmt = conn.prepare(&sql)?;
            let mut param_vals: Vec<Box<dyn rusqlite::ToSql>> =
                vec![Box::new(query.required_tags.len() as i64)];
            for tag in &query.required_tags {
                param_vals.push(Box::new(tag.clone()));
            }
            let param_refs: Vec<&dyn rusqlite::ToSql> =
                param_vals.iter().map(|b| b.as_ref()).collect();
            let rows = stmt.query_map(param_refs.as_slice(), |r| r.get(0))?;
            rows.filter_map(|r| r.ok()).collect()
        };

        // Step 2: fetch full Gene rows and score them.
        let mut matches = Vec::new();
        for gene_id in &candidate_ids {
            let mut stmt = conn.prepare(
                "SELECT id,name,description,tags_json,template,
                        preconditions_json,validation_steps_json,
                        confidence,use_count,success_count,quality_score,
                        created_at,last_used_at,last_boosted_at
                 FROM genes WHERE id=?1 AND confidence >= ?2",
            )?;
            let mut rows = stmt.query(params![gene_id, query.min_confidence])?;
            if let Some(row) = rows.next()? {
                let gene = row_to_gene(row)?;
                let relevance = relevance_score(&gene, query);
                matches.push(GeneMatch {
                    gene,
                    relevance_score: relevance,
                });
            }
        }

        // Step 3: rank by relevance desc, truncate.
        matches.sort_by(|a, b| b.relevance_score.partial_cmp(&a.relevance_score).unwrap());
        matches.truncate(query.limit);
        Ok(matches)
    }

    async fn decay_all(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE genes SET confidence = MAX(?1, confidence - ?2)",
            params![Gene::STALE_THRESHOLD, Gene::DECAY_PER_QUERY],
        )?;
        conn.execute(
            "UPDATE capsules SET confidence = MAX(?1, confidence - ?2)",
            params![Capsule::STALE_THRESHOLD, Gene::DECAY_PER_QUERY],
        )?;
        Ok(())
    }

    async fn record_gene_outcome(&self, id: Uuid, success: bool) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        if success {
            conn.execute(
                "UPDATE genes SET
                   use_count     = use_count + 1,
                   success_count = success_count + 1,
                   confidence    = MIN(1.0, confidence + ?1),
                   last_used_at  = ?2,
                   last_boosted_at = ?2
                 WHERE id = ?3",
                params![
                    Gene::BOOST_ON_SUCCESS,
                    Utc::now().to_rfc3339(),
                    id.to_string()
                ],
            )?;
        } else {
            conn.execute(
                "UPDATE genes SET
                   use_count    = use_count + 1,
                   confidence   = MAX(?1, confidence - ?2),
                   last_used_at = ?3
                 WHERE id = ?4",
                params![
                    Gene::STALE_THRESHOLD,
                    Gene::PENALTY_ON_FAILURE,
                    Utc::now().to_rfc3339(),
                    id.to_string()
                ],
            )?;
        }
        Ok(())
    }

    async fn stale_genes(&self) -> Result<Vec<Gene>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id,name,description,tags_json,template,
                    preconditions_json,validation_steps_json,
                    confidence,use_count,success_count,quality_score,
                    created_at,last_used_at,last_boosted_at
             FROM genes WHERE confidence <= ?1",
        )?;
        let genes = stmt
            .query_map(params![Gene::STALE_THRESHOLD], |r| Ok(row_to_gene(r)))?
            .filter_map(|r| r.ok().and_then(|g| g.ok()))
            .collect();
        Ok(genes)
    }

    async fn upsert_capsule(&self, capsule: &Capsule) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            r#"INSERT INTO capsules
               (id,gene_id,content,env_fingerprint,quality_score,confidence,
                use_count,success_count,last_replay_run_id,created_at,last_used_at)
               VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
               ON CONFLICT(id) DO UPDATE SET
                 content=excluded.content,
                 env_fingerprint=excluded.env_fingerprint,
                 quality_score=excluded.quality_score,
                 confidence=excluded.confidence,
                 use_count=excluded.use_count,
                 success_count=excluded.success_count,
                 last_replay_run_id=excluded.last_replay_run_id,
                 last_used_at=excluded.last_used_at"#,
            params![
                capsule.id.to_string(),
                capsule.gene_id.to_string(),
                capsule.content,
                capsule.env_fingerprint,
                capsule.quality_score,
                capsule.confidence,
                capsule.use_count as i64,
                capsule.success_count as i64,
                capsule.last_replay_run_id.map(|u| u.to_string()),
                capsule.created_at.to_rfc3339(),
                capsule.last_used_at.map(|d| d.to_rfc3339()),
            ],
        )?;
        Ok(())
    }

    async fn get_capsule(&self, id: Uuid) -> Result<Option<Capsule>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id,gene_id,content,env_fingerprint,quality_score,confidence,
                    use_count,success_count,last_replay_run_id,created_at,last_used_at
             FROM capsules WHERE id=?1",
        )?;
        let mut rows = stmt.query(params![id.to_string()])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row_to_capsule(row)?))
        } else {
            Ok(None)
        }
    }

    async fn capsules_for_gene(&self, gene_id: Uuid) -> Result<Vec<Capsule>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id,gene_id,content,env_fingerprint,quality_score,confidence,
                    use_count,success_count,last_replay_run_id,created_at,last_used_at
             FROM capsules WHERE gene_id=?1 ORDER BY confidence DESC",
        )?;
        let capsules = stmt
            .query_map(params![gene_id.to_string()], |r| Ok(row_to_capsule(r)))?
            .filter_map(|r| r.ok().and_then(|c| c.ok()))
            .collect();
        Ok(capsules)
    }

    async fn record_capsule_outcome(
        &self,
        id: Uuid,
        success: bool,
        replay_run_id: Option<Uuid>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        if success {
            conn.execute(
                "UPDATE capsules SET
                   use_count=use_count+1, success_count=success_count+1,
                   confidence=MIN(1.0, confidence+?1),
                   last_replay_run_id=?2, last_used_at=?3
                 WHERE id=?4",
                params![
                    Gene::BOOST_ON_SUCCESS,
                    replay_run_id.map(|u| u.to_string()),
                    now,
                    id.to_string()
                ],
            )?;
        } else {
            conn.execute(
                "UPDATE capsules SET
                   use_count=use_count+1,
                   confidence=MAX(?1, confidence-?2),
                   last_used_at=?3
                 WHERE id=?4",
                params![
                    Capsule::STALE_THRESHOLD,
                    Gene::PENALTY_ON_FAILURE,
                    now,
                    id.to_string()
                ],
            )?;
        }
        Ok(())
    }
}

// ─── Row mappers ───────────────────────────────────────────────────────────

fn row_to_gene(row: &rusqlite::Row) -> rusqlite::Result<Gene> {
    let tags_json: String = row.get(3)?;
    let pre_json: String = row.get(5)?;
    let val_json: String = row.get(6)?;
    let created_at_str: String = row.get(11)?;
    let last_used_str: Option<String> = row.get(12)?;
    let last_boost_str: Option<String> = row.get(13)?;

    Ok(Gene {
        id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap(),
        name: row.get(1)?,
        description: row.get(2)?,
        tags: serde_json::from_str(&tags_json).unwrap_or_default(),
        template: row.get(4)?,
        preconditions: serde_json::from_str(&pre_json).unwrap_or_default(),
        validation_steps: serde_json::from_str(&val_json).unwrap_or_default(),
        confidence: row.get(7)?,
        use_count: row.get::<_, i64>(8)? as u64,
        success_count: row.get::<_, i64>(9)? as u64,
        quality_score: row.get(10)?,
        created_at: created_at_str.parse().unwrap_or_else(|_| Utc::now()),
        last_used_at: last_used_str.and_then(|s| s.parse().ok()),
        last_boosted_at: last_boost_str.and_then(|s| s.parse().ok()),
    })
}

fn row_to_capsule(row: &rusqlite::Row) -> rusqlite::Result<Capsule> {
    let created_str: String = row.get(9)?;
    let used_str: Option<String> = row.get(10)?;
    Ok(Capsule {
        id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap(),
        gene_id: Uuid::parse_str(&row.get::<_, String>(1)?).unwrap(),
        content: row.get(2)?,
        env_fingerprint: row.get(3)?,
        quality_score: row.get(4)?,
        confidence: row.get(5)?,
        use_count: row.get::<_, i64>(6)? as u64,
        success_count: row.get::<_, i64>(7)? as u64,
        last_replay_run_id: row
            .get::<_, Option<String>>(8)?
            .and_then(|s| Uuid::parse_str(&s).ok()),
        created_at: created_str.parse().unwrap_or_else(|_| Utc::now()),
        last_used_at: used_str.and_then(|s| s.parse().ok()),
    })
}

// ─── Relevance scoring (shared, no DB dependency) ─────────────────────────

fn relevance_score(gene: &Gene, query: &GeneQuery) -> f64 {
    // Tag overlap ratio.
    let description_lower = query.problem_description.to_lowercase();
    let keyword_hits = gene
        .tags
        .iter()
        .filter(|t| description_lower.contains(t.as_str()))
        .count();
    let tag_score = if gene.tags.is_empty() {
        0.5
    } else {
        keyword_hits as f64 / gene.tags.len() as f64
    };

    // Blend tag score with confidence and quality.
    0.40 * tag_score + 0.35 * gene.confidence + 0.25 * gene.quality_score
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Capsule, Gene, GeneQuery};
    use chrono::Utc;
    use uuid::Uuid;

    fn sample_gene(tags: &[&str]) -> Gene {
        Gene {
            id: Uuid::new_v4(),
            name: "test-gene".into(),
            description: "A test gene".into(),
            tags: tags.iter().map(|s| s.to_string()).collect(),
            template: "fix: {description}".into(),
            preconditions: vec!["tests pass".into()],
            validation_steps: vec!["cargo test".into()],
            confidence: 0.80,
            use_count: 0,
            success_count: 0,
            quality_score: 0.70,
            created_at: Utc::now(),
            last_used_at: None,
            last_boosted_at: None,
        }
    }

    fn sample_capsule(gene_id: Uuid) -> Capsule {
        Capsule {
            id: Uuid::new_v4(),
            gene_id,
            content: "patch content".into(),
            env_fingerprint: "linux-x86_64".into(),
            quality_score: 0.75,
            confidence: 0.85,
            use_count: 0,
            success_count: 0,
            last_replay_run_id: None,
            created_at: Utc::now(),
            last_used_at: None,
        }
    }

    #[tokio::test]
    async fn test_upsert_and_get_gene() {
        let store = SqliteGeneStore::open(":memory:").unwrap();
        let gene = sample_gene(&["rust", "compiler"]);
        let id = gene.id;

        store.upsert_gene(&gene).await.unwrap();

        let fetched = store
            .get_gene(id)
            .await
            .unwrap()
            .expect("gene should exist");
        assert_eq!(fetched.id, id);
        assert_eq!(fetched.name, "test-gene");
        assert!(fetched.tags.contains(&"rust".to_string()));
    }

    #[tokio::test]
    async fn test_upsert_is_idempotent() {
        let store = SqliteGeneStore::open(":memory:").unwrap();
        let mut gene = sample_gene(&["rust"]);
        store.upsert_gene(&gene).await.unwrap();

        gene.confidence = 0.95;
        store.upsert_gene(&gene).await.unwrap();

        let fetched = store.get_gene(gene.id).await.unwrap().unwrap();
        assert!((fetched.confidence - 0.95).abs() < 1e-6);
    }

    #[tokio::test]
    async fn test_delete_gene() {
        let store = SqliteGeneStore::open(":memory:").unwrap();
        let gene = sample_gene(&[]);
        let id = gene.id;
        store.upsert_gene(&gene).await.unwrap();
        store.delete_gene(id).await.unwrap();
        assert!(store.get_gene(id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_search_genes_by_tag() {
        let store = SqliteGeneStore::open(":memory:").unwrap();
        let gene_a = sample_gene(&["rust", "memory"]);
        let gene_b = sample_gene(&["python"]);
        store.upsert_gene(&gene_a).await.unwrap();
        store.upsert_gene(&gene_b).await.unwrap();

        let query = GeneQuery {
            problem_description: "rust memory issue".into(),
            required_tags: vec!["rust".into()],
            min_confidence: 0.0,
            limit: 10,
        };
        let results = store.search_genes(&query).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].gene.id, gene_a.id);
    }

    #[tokio::test]
    async fn test_record_gene_outcome_success() {
        let store = SqliteGeneStore::open(":memory:").unwrap();
        let gene = sample_gene(&[]);
        let initial_confidence = gene.confidence;
        store.upsert_gene(&gene).await.unwrap();

        store.record_gene_outcome(gene.id, true).await.unwrap();

        let fetched = store.get_gene(gene.id).await.unwrap().unwrap();
        assert_eq!(fetched.use_count, 1);
        assert_eq!(fetched.success_count, 1);
        assert!(fetched.confidence > initial_confidence);
    }

    #[tokio::test]
    async fn test_record_gene_outcome_failure() {
        let store = SqliteGeneStore::open(":memory:").unwrap();
        let gene = sample_gene(&[]);
        let initial_confidence = gene.confidence;
        store.upsert_gene(&gene).await.unwrap();

        store.record_gene_outcome(gene.id, false).await.unwrap();

        let fetched = store.get_gene(gene.id).await.unwrap().unwrap();
        assert_eq!(fetched.use_count, 1);
        assert_eq!(fetched.success_count, 0);
        assert!(fetched.confidence < initial_confidence);
    }

    #[tokio::test]
    async fn test_decay_all() {
        let store = SqliteGeneStore::open(":memory:").unwrap();
        let gene = sample_gene(&[]);
        let initial = gene.confidence;
        store.upsert_gene(&gene).await.unwrap();

        store.decay_all().await.unwrap();

        let fetched = store.get_gene(gene.id).await.unwrap().unwrap();
        // confidence should decrease by DECAY_PER_QUERY or clamp to STALE_THRESHOLD
        assert!(fetched.confidence <= initial);
    }

    #[tokio::test]
    async fn test_stale_genes() {
        let store = SqliteGeneStore::open(":memory:").unwrap();
        let mut gene = sample_gene(&[]);
        // Set confidence to exactly the stale threshold
        gene.confidence = Gene::STALE_THRESHOLD;
        store.upsert_gene(&gene).await.unwrap();

        let stale = store.stale_genes().await.unwrap();
        assert!(!stale.is_empty());
        assert_eq!(stale[0].id, gene.id);
    }

    #[tokio::test]
    async fn test_capsule_crud() {
        let store = SqliteGeneStore::open(":memory:").unwrap();
        let gene = sample_gene(&[]);
        store.upsert_gene(&gene).await.unwrap();

        let capsule = sample_capsule(gene.id);
        let cid = capsule.id;
        store.upsert_capsule(&capsule).await.unwrap();

        let fetched = store
            .get_capsule(cid)
            .await
            .unwrap()
            .expect("capsule should exist");
        assert_eq!(fetched.id, cid);
        assert_eq!(fetched.gene_id, gene.id);
    }

    #[tokio::test]
    async fn test_capsules_for_gene() {
        let store = SqliteGeneStore::open(":memory:").unwrap();
        let gene = sample_gene(&[]);
        store.upsert_gene(&gene).await.unwrap();

        let c1 = sample_capsule(gene.id);
        let c2 = sample_capsule(gene.id);
        store.upsert_capsule(&c1).await.unwrap();
        store.upsert_capsule(&c2).await.unwrap();

        let caps = store.capsules_for_gene(gene.id).await.unwrap();
        assert_eq!(caps.len(), 2);
    }

    #[tokio::test]
    async fn test_record_capsule_outcome() {
        let store = SqliteGeneStore::open(":memory:").unwrap();
        let gene = sample_gene(&[]);
        store.upsert_gene(&gene).await.unwrap();

        let capsule = sample_capsule(gene.id);
        let initial_conf = capsule.confidence;
        store.upsert_capsule(&capsule).await.unwrap();

        store
            .record_capsule_outcome(capsule.id, true, None)
            .await
            .unwrap();

        let fetched = store.get_capsule(capsule.id).await.unwrap().unwrap();
        assert_eq!(fetched.use_count, 1);
        assert_eq!(fetched.success_count, 1);
        assert!(fetched.confidence > initial_conf);
    }
}
