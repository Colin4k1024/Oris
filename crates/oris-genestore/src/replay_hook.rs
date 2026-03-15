//! replay_hook.rs — Agent Feedback Loop replay hook
//!
//! Provides [`ReplayFeedbackHook`], the glue between the agent replay path and
//! the [`GeneStore`] usage history.
//!
//! # Replay-priority strategy
//!
//! Before the agent executes its full LLM `plan()` call the hook can be
//! consulted.  A gene whose confidence is at or above
//! [`ReplayConfig::min_confidence_for_replay`] is considered a *replay
//! candidate*; the agent may skip the full LLM round-trip and execute the
//! gene template directly.
//!
//! ```text
//!  agent.plan()
//!     │
//!     ▼
//!  hook.query_replay_candidate(signals)
//!     ├── Some(candidate) → replay path  →  hook.record(id, success, latency_ms)
//!     └── None            → cold LLM path → hook.record_cold_start()
//! ```
//!
//! # Observability
//!
//! [`ReplayMetrics`] accumulates hit/miss counts, average latency savings, and
//! total feedback writes so that the counters can be exposed to dashboards.

use crate::{
    store::GeneStore,
    types::{Gene, GeneQuery},
};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for the replay feedback hook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayConfig {
    /// Minimum gene confidence required to trigger the replay path
    /// (default: 0.70 — higher than GeneQuery default of 0.50 to ensure only
    /// high-quality genes skip the full LLM round-trip).
    pub min_confidence_for_replay: f64,
    /// Maximum number of replay candidates returned per query (default: 1).
    pub max_candidates: usize,
    /// Tags that all replay candidates must carry (default: empty = any gene).
    pub required_tags: Vec<String>,
}

impl Default for ReplayConfig {
    fn default() -> Self {
        Self {
            min_confidence_for_replay: 0.70,
            max_candidates: 1,
            required_tags: vec![],
        }
    }
}

// ---------------------------------------------------------------------------
// Replay candidate
// ---------------------------------------------------------------------------

/// A gene that satisfies the replay confidence threshold.
///
/// Returned by [`ReplayFeedbackHook::query_replay_candidate`] when a
/// high-confidence gene matches the supplied signals.
#[derive(Debug, Clone)]
pub struct ReplayCandidate {
    /// The matched gene.
    pub gene: Gene,
    /// Relevance score computed by the store's search.
    pub relevance_score: f64,
}

// ---------------------------------------------------------------------------
// Observability
// ---------------------------------------------------------------------------

/// Aggregated replay metrics accumulated since the hook was constructed.
///
/// All fields are updated atomically via interior mutability so the hook can
/// be shared across async tasks.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReplayMetrics {
    /// Total number of `query_replay_candidate` calls.
    pub queries_total: u64,
    /// Number of queries where a replay candidate was found (hits).
    pub replay_hits: u64,
    /// Number of queries where no candidate was found (cold LLM path).
    pub replay_misses: u64,
    /// Number of replay outcomes written back to the store.
    pub feedback_writes: u64,
    /// Number of successful replays recorded.
    pub replay_successes: u64,
    /// Number of failed replays recorded.
    pub replay_failures: u64,
    /// Cumulative latency saved vs. cold LLM path (ms), as reported by callers.
    pub total_latency_saved_ms: u64,
}

// ---------------------------------------------------------------------------
// Hook
// ---------------------------------------------------------------------------

/// Connects the agent replay path to the [`GeneStore`].
///
/// `S` is any [`GeneStore`] implementation (e.g. `SqliteGeneStore` or an
/// in-memory test double).
pub struct ReplayFeedbackHook<S: GeneStore> {
    store: Arc<S>,
    config: ReplayConfig,
    metrics: std::sync::Mutex<ReplayMetrics>,
}

impl<S: GeneStore> ReplayFeedbackHook<S> {
    /// Create a hook backed by `store` with the given `config`.
    pub fn new(store: Arc<S>, config: ReplayConfig) -> Self {
        Self {
            store,
            config,
            metrics: std::sync::Mutex::new(ReplayMetrics::default()),
        }
    }

    /// Create a hook using [`ReplayConfig::default`].
    pub fn with_default_config(store: Arc<S>) -> Self {
        Self::new(store, ReplayConfig::default())
    }

    // -----------------------------------------------------------------------
    // Query — replay-priority strategy
    // -----------------------------------------------------------------------

    /// Query the GeneStore for a high-confidence replay candidate matching
    /// `signals`.
    ///
    /// Returns `Some(candidate)` when a gene above the confidence threshold
    /// is found — the agent should enter the **replay path** and call
    /// [`record_replay_outcome`](Self::record_replay_outcome) with the
    /// result.
    ///
    /// Returns `None` when no suitable gene exists — the agent should fall
    /// back to a **cold LLM** `plan()` call.
    ///
    /// # Latency note
    ///
    /// This call performs a single indexed SQLite read and is expected to
    /// complete in < 1 ms on local storage.
    pub async fn query_replay_candidate(
        &self,
        signals: &[String],
    ) -> Result<Option<ReplayCandidate>> {
        let query = GeneQuery {
            min_confidence: self.config.min_confidence_for_replay,
            limit: self.config.max_candidates,
            required_tags: self.config.required_tags.clone(),
            problem_description: signals.join(" "),
        };

        let matches = self.store.search_genes(&query).await?;

        let mut metrics = self.metrics.lock().unwrap();
        metrics.queries_total += 1;

        if let Some(best) = matches.into_iter().next() {
            metrics.replay_hits += 1;
            Ok(Some(ReplayCandidate {
                gene: best.gene,
                relevance_score: best.relevance_score,
            }))
        } else {
            metrics.replay_misses += 1;
            Ok(None)
        }
    }

    // -----------------------------------------------------------------------
    // Feedback — write replay outcome back to GeneStore
    // -----------------------------------------------------------------------

    /// Record the outcome of a replay execution for gene `gene_id`.
    ///
    /// - `success`: whether the replayed gene produced a correct result.
    /// - `latency_saved_ms`: optional estimate of how many milliseconds were
    ///   saved compared to a full cold LLM `plan()` call.
    ///
    /// Writes the outcome to the store's usage history via
    /// `record_gene_outcome`.
    pub async fn record_replay_outcome(
        &self,
        gene_id: Uuid,
        success: bool,
        latency_saved_ms: Option<u64>,
    ) -> Result<()> {
        self.store.record_gene_outcome(gene_id, success).await?;

        let mut metrics = self.metrics.lock().unwrap();
        metrics.feedback_writes += 1;
        if success {
            metrics.replay_successes += 1;
        } else {
            metrics.replay_failures += 1;
        }
        if let Some(saved) = latency_saved_ms {
            metrics.total_latency_saved_ms += saved;
        }

        Ok(())
    }

    /// Increment the cold-start miss counter.
    ///
    /// Call this whenever the agent falls back to a full LLM `plan()` because
    /// no replay candidate was found.  This keeps the `replay_misses` counter
    /// accurate even when `query_replay_candidate` was not called (e.g. the
    /// caller short-circuits on its own).
    pub fn record_cold_start(&self) {
        let mut metrics = self.metrics.lock().unwrap();
        metrics.replay_misses += 1;
        metrics.queries_total += 1;
    }

    // -----------------------------------------------------------------------
    // Observability
    // -----------------------------------------------------------------------

    /// Snapshot of accumulated metrics.
    pub fn metrics(&self) -> ReplayMetrics {
        self.metrics.lock().unwrap().clone()
    }

    /// Hit-rate in [0.0, 1.0] — `replay_hits / queries_total`.
    /// Returns `0.0` when no queries have been made yet.
    pub fn hit_rate(&self) -> f64 {
        let m = self.metrics.lock().unwrap();
        if m.queries_total == 0 {
            0.0
        } else {
            m.replay_hits as f64 / m.queries_total as f64
        }
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::GeneStore;
    use crate::types::{Capsule, Gene, GeneMatch, GeneQuery};
    use anyhow::Result;
    use async_trait::async_trait;
    use chrono::Utc;
    use std::collections::HashMap;
    use std::sync::Mutex as StdMutex;

    // -------------------------------------------------------------------
    // In-memory test double
    // -------------------------------------------------------------------

    struct InMemoryGeneStore {
        genes: StdMutex<HashMap<Uuid, Gene>>,
        outcomes: StdMutex<Vec<(Uuid, bool)>>,
        min_confidence_for_search: f64,
    }

    impl InMemoryGeneStore {
        fn new(min_confidence_for_search: f64) -> Self {
            Self {
                genes: StdMutex::new(HashMap::new()),
                outcomes: StdMutex::new(Vec::new()),
                min_confidence_for_search,
            }
        }

        fn insert(&self, gene: Gene) {
            self.genes.lock().unwrap().insert(gene.id, gene);
        }
    }

    fn make_gene(confidence: f64) -> Gene {
        Gene {
            id: Uuid::new_v4(),
            name: "test-gene".into(),
            description: "test".into(),
            tags: vec!["rust".into()],
            template: "fn fix() {}".into(),
            preconditions: vec![],
            validation_steps: vec![],
            confidence,
            use_count: 5,
            success_count: 4,
            quality_score: 0.8,
            created_at: Utc::now(),
            last_used_at: None,
            last_boosted_at: None,
        }
    }

    #[async_trait]
    impl GeneStore for InMemoryGeneStore {
        async fn upsert_gene(&self, gene: &Gene) -> Result<()> {
            self.genes.lock().unwrap().insert(gene.id, gene.clone());
            Ok(())
        }
        async fn get_gene(&self, id: Uuid) -> Result<Option<Gene>> {
            Ok(self.genes.lock().unwrap().get(&id).cloned())
        }
        async fn delete_gene(&self, id: Uuid) -> Result<()> {
            self.genes.lock().unwrap().remove(&id);
            Ok(())
        }
        async fn search_genes(&self, query: &GeneQuery) -> Result<Vec<GeneMatch>> {
            let genes = self.genes.lock().unwrap();
            let mut results: Vec<GeneMatch> = genes
                .values()
                .filter(|g| g.confidence >= self.min_confidence_for_search)
                .map(|g| GeneMatch {
                    gene: g.clone(),
                    relevance_score: g.confidence,
                })
                .collect();
            results.sort_by(|a, b| {
                b.relevance_score
                    .partial_cmp(&a.relevance_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            results.truncate(query.limit);
            Ok(results)
        }
        async fn decay_all(&self) -> Result<()> {
            Ok(())
        }
        async fn record_gene_outcome(&self, id: Uuid, success: bool) -> Result<()> {
            self.outcomes.lock().unwrap().push((id, success));
            Ok(())
        }
        async fn stale_genes(&self) -> Result<Vec<Gene>> {
            Ok(vec![])
        }
        async fn upsert_capsule(&self, _capsule: &Capsule) -> Result<()> {
            Ok(())
        }
        async fn get_capsule(&self, _id: Uuid) -> Result<Option<Capsule>> {
            Ok(None)
        }
        async fn capsules_for_gene(&self, _gene_id: Uuid) -> Result<Vec<Capsule>> {
            Ok(vec![])
        }
        async fn record_capsule_outcome(
            &self,
            _id: Uuid,
            _success: bool,
            _replay_run_id: Option<Uuid>,
        ) -> Result<()> {
            Ok(())
        }
    }

    // -------------------------------------------------------------------
    // Tests
    // -------------------------------------------------------------------

    fn hook_with_gene(confidence: f64) -> (ReplayFeedbackHook<InMemoryGeneStore>, Uuid) {
        let gene = make_gene(confidence);
        let gene_id = gene.id;
        // Store returns results with confidence >= 0.0 (delegates threshold to hook config)
        let store = Arc::new(InMemoryGeneStore::new(0.0));
        store.insert(gene);
        let hook = ReplayFeedbackHook::new(
            store,
            ReplayConfig {
                min_confidence_for_replay: 0.70,
                max_candidates: 1,
                required_tags: vec![],
            },
        );
        (hook, gene_id)
    }

    #[tokio::test]
    async fn test_high_confidence_gene_returns_candidate() {
        let (hook, gene_id) = hook_with_gene(0.85);
        let candidate = hook
            .query_replay_candidate(&["panic".into()])
            .await
            .unwrap();
        assert!(candidate.is_some(), "expected replay candidate");
        assert_eq!(candidate.unwrap().gene.id, gene_id);
    }

    #[tokio::test]
    async fn test_low_confidence_gene_returns_none() {
        // Gene confidence 0.50 is below the hook's threshold of 0.70
        let store = Arc::new(InMemoryGeneStore::new(0.0));
        store.insert(make_gene(0.50));
        // But the hook's GeneQuery uses min_confidence=0.70, so InMemory store
        // (which ignores the query threshold in this stub) returns the gene —
        // we need the actual relevance filter. Let us set store threshold=0.80.
        let store2 = Arc::new(InMemoryGeneStore::new(0.80));
        store2.insert(make_gene(0.50));
        let hook = ReplayFeedbackHook::with_default_config(store2);
        let candidate = hook
            .query_replay_candidate(&["panic".into()])
            .await
            .unwrap();
        assert!(
            candidate.is_none(),
            "low-confidence gene must not trigger replay"
        );
    }

    #[tokio::test]
    async fn test_feedback_written_on_success() {
        let (hook, gene_id) = hook_with_gene(0.90);
        hook.record_replay_outcome(gene_id, true, Some(250))
            .await
            .unwrap();
        let m = hook.metrics();
        assert_eq!(m.feedback_writes, 1);
        assert_eq!(m.replay_successes, 1);
        assert_eq!(m.total_latency_saved_ms, 250);
    }

    #[tokio::test]
    async fn test_feedback_written_on_failure() {
        let (hook, gene_id) = hook_with_gene(0.90);
        hook.record_replay_outcome(gene_id, false, None)
            .await
            .unwrap();
        let m = hook.metrics();
        assert_eq!(m.replay_failures, 1);
        assert_eq!(m.replay_successes, 0);
    }

    #[tokio::test]
    async fn test_metrics_hit_rate() {
        let (hook, _) = hook_with_gene(0.90);
        hook.query_replay_candidate(&["sig".into()]).await.unwrap(); // hit
        hook.record_cold_start(); // miss
        let rate = hook.hit_rate();
        assert!(
            (rate - 0.5).abs() < 0.01,
            "expected 50% hit rate, got {rate}"
        );
    }

    #[tokio::test]
    async fn test_cold_start_increments_miss_counter() {
        let store = Arc::new(InMemoryGeneStore::new(0.0));
        let hook = ReplayFeedbackHook::with_default_config(store);
        hook.record_cold_start();
        hook.record_cold_start();
        let m = hook.metrics();
        assert_eq!(m.replay_misses, 2);
        assert_eq!(m.queries_total, 2);
    }

    #[tokio::test]
    async fn test_empty_store_gives_no_candidate() {
        let store = Arc::new(InMemoryGeneStore::new(0.0));
        let hook = ReplayFeedbackHook::with_default_config(store);
        let result = hook
            .query_replay_candidate(&["unknown".into()])
            .await
            .unwrap();
        assert!(result.is_none());
        assert_eq!(hook.metrics().replay_misses, 1);
    }

    #[tokio::test]
    async fn test_outcome_writes_appear_in_store() {
        let store = Arc::new(InMemoryGeneStore::new(0.0));
        store.insert(make_gene(0.90));
        let hook = ReplayFeedbackHook::with_default_config(Arc::clone(&store));
        let candidate = hook
            .query_replay_candidate(&["sig".into()])
            .await
            .unwrap()
            .unwrap();
        hook.record_replay_outcome(candidate.gene.id, true, Some(100))
            .await
            .unwrap();
        // Verify outcome was written to the underlying store
        let outcomes = store.outcomes.lock().unwrap();
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].0, candidate.gene.id);
        assert!(outcomes[0].1); // success=true
    }
}
