//! oris-genestore/src/migrate.rs
//!
//! One-shot JSONL → SQLite migration for Gene assets.
//!
//! # Format
//! The JSONL file must contain one JSON-encoded `Gene` record per line.
//! Blank lines and lines starting with `#` are skipped.
//!
//! # Usage
//! Open a store and call [`from_jsonl`] with the path to a JSONL file.
//! Each valid `Gene` line is upserted; invalid lines are skipped with a warning.

use crate::store::GeneStore;
use crate::types::Gene;
use anyhow::{Context, Result};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

/// Migrate genes from a JSONL file into an existing `SqliteGeneStore`.
///
/// Each non-blank, non-comment line is parsed as a JSON `Gene` and
/// upserted via [`GeneStore::upsert_gene`].  The function returns the number
/// of genes successfully migrated.
///
/// Lines that fail to parse are reported as warnings but do **not** abort the
/// migration — partial progress is committed so the file can be re-run safely
/// (upsert is idempotent).
pub async fn from_jsonl<S: GeneStore>(path: impl AsRef<Path>, store: &S) -> Result<usize> {
    let path = path.as_ref();
    let file = File::open(path)
        .with_context(|| format!("failed to open JSONL file: {}", path.display()))?;
    let reader = BufReader::new(file);

    let mut migrated = 0usize;
    let mut errors = 0usize;

    for (lineno, line) in reader.lines().enumerate() {
        let line = line.with_context(|| format!("I/O error reading line {}", lineno + 1))?;
        let trimmed = line.trim();

        // Skip blank lines and comments
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        match serde_json::from_str::<Gene>(trimmed) {
            Ok(gene) => {
                store
                    .upsert_gene(&gene)
                    .await
                    .with_context(|| format!("failed to upsert gene at line {}", lineno + 1))?;
                migrated += 1;
            }
            Err(e) => {
                eprintln!(
                    "[oris-genestore migrate] WARN line {}: parse error — {} (skipping)",
                    lineno + 1,
                    e
                );
                errors += 1;
            }
        }
    }

    if errors > 0 {
        eprintln!(
            "[oris-genestore migrate] {} line(s) skipped due to parse errors",
            errors
        );
    }

    Ok(migrated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::SqliteGeneStore;
    use chrono::Utc;
    use std::io::Write;
    use tempfile::NamedTempFile;
    use uuid::Uuid;

    fn make_gene_json(id: Uuid, tags: &[&str]) -> String {
        serde_json::json!({
            "id": id,
            "name": "migration-test",
            "description": "gene for migration test",
            "tags": tags,
            "template": "fix: {desc}",
            "preconditions": [],
            "validation_steps": ["cargo test"],
            "confidence": 0.75,
            "use_count": 0,
            "success_count": 0,
            "quality_score": 0.60,
            "created_at": Utc::now().to_rfc3339(),
            "last_used_at": null,
            "last_boosted_at": null
        })
        .to_string()
    }

    #[tokio::test]
    async fn test_migration_roundtrip() {
        // Write a JSONL temp file
        let mut file = NamedTempFile::new().unwrap();
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        writeln!(file, "{}", make_gene_json(id1, &["rust", "panic"])).unwrap();
        writeln!(file, "# this is a comment").unwrap();
        writeln!(file).unwrap(); // blank line
        writeln!(file, "{}", make_gene_json(id2, &["cargo", "build"])).unwrap();
        file.flush().unwrap();

        let store = SqliteGeneStore::open(":memory:").unwrap();
        let count = from_jsonl(file.path(), &store).await.unwrap();
        assert_eq!(count, 2);

        let g1 = store
            .get_gene(id1)
            .await
            .unwrap()
            .expect("gene 1 should exist");
        assert!(g1.tags.contains(&"rust".to_string()));
        let g2 = store
            .get_gene(id2)
            .await
            .unwrap()
            .expect("gene 2 should exist");
        assert!(g2.tags.contains(&"cargo".to_string()));
    }

    #[tokio::test]
    async fn test_migration_skips_invalid_lines() {
        let mut file = NamedTempFile::new().unwrap();
        let id = Uuid::new_v4();
        writeln!(file, "{}", make_gene_json(id, &["valid"])).unwrap();
        writeln!(file, "{{not valid json}}").unwrap();
        file.flush().unwrap();

        let store = SqliteGeneStore::open(":memory:").unwrap();
        // Should succeed; one valid gene migrated, one line skipped
        let count = from_jsonl(file.path(), &store).await.unwrap();
        assert_eq!(count, 1);
    }
}
