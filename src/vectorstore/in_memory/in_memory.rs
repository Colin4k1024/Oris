use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use serde_json::Value;

use crate::{
    embedding::embedder_trait::Embedder,
    schemas::Document,
    vectorstore::{VecStoreOptions, VectorStore, VectorStoreError},
};

static IN_MEMORY_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

fn next_id() -> String {
    format!(
        "inmem-{}",
        IN_MEMORY_ID_COUNTER.fetch_add(1, Ordering::SeqCst)
    )
}

/// In-memory entry: (id, document, embedding, namespace)
type Entry = (String, Document, Vec<f64>, Option<String>);

pub struct Store {
    data: RwLock<Vec<Entry>>,
    embedder: Arc<dyn Embedder>,
}

pub struct StoreBuilder {
    embedder: Option<Arc<dyn Embedder>>,
}

impl StoreBuilder {
    pub fn new() -> Self {
        StoreBuilder { embedder: None }
    }

    pub fn embedder<E: Embedder + 'static>(mut self, embedder: E) -> Self {
        self.embedder = Some(Arc::new(embedder));
        self
    }

    pub fn build(self) -> Result<Store, VectorStoreError> {
        let embedder = self.embedder.ok_or("embedder is required".to_string())?;
        Ok(Store {
            data: RwLock::new(Vec::new()),
            embedder,
        })
    }
}

impl Default for StoreBuilder {
    fn default() -> Self {
        Self::new()
    }
}

fn cosine_similarity(a: &[f64], b: &[f64]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt();
    let norm_b: f64 = b.iter().map(|x| x * x).sum::<f64>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

fn metadata_matches(
    doc_metadata: &HashMap<String, Value>,
    filter: &serde_json::Map<String, Value>,
) -> bool {
    for (k, v) in filter {
        match doc_metadata.get(k) {
            Some(dv) if dv == v => {}
            _ => return false,
        }
    }
    true
}

pub type InMemoryOptions = VecStoreOptions<Value>;

#[async_trait]
impl VectorStore for Store {
    type Options = InMemoryOptions;

    async fn add_documents(
        &self,
        docs: &[Document],
        opt: &InMemoryOptions,
    ) -> Result<Vec<String>, VectorStoreError> {
        let texts: Vec<String> = docs.iter().map(|d| d.page_content.clone()).collect();
        let embedder = opt.embedder.as_ref().unwrap_or(&self.embedder);
        let vectors = embedder.embed_documents(&texts).await?;
        if vectors.len() != docs.len() {
            return Err("Number of vectors and documents do not match".into());
        }
        let namespace = opt.name_space.clone();
        let mut data = self.data.write().map_err(|e| e.to_string())?;
        let mut ids = Vec::with_capacity(docs.len());
        for (doc, vector) in docs.iter().zip(vectors.iter()) {
            let id = next_id();
            ids.push(id.clone());
            let mut doc = doc.clone();
            doc.score = 0.0;
            data.push((id, doc, vector.clone(), namespace.clone()));
        }
        Ok(ids)
    }

    async fn similarity_search(
        &self,
        query: &str,
        limit: usize,
        opt: &InMemoryOptions,
    ) -> Result<Vec<Document>, VectorStoreError> {
        let embedder = opt.embedder.as_ref().unwrap_or(&self.embedder);
        let query_vector = embedder.embed_query(query).await?;
        let data = self.data.read().map_err(|e| e.to_string())?;
        let namespace_filter = opt.name_space.as_deref();
        let score_threshold = opt
            .score_threshold
            .map(f64::from)
            .unwrap_or(f64::NEG_INFINITY);
        let filter_map = opt.filters.as_ref().and_then(|v| v.as_object());

        let mut scored: Vec<(f64, Document)> = data
            .iter()
            .filter(|(_, _, _, ns)| match (namespace_filter, ns) {
                (None, _) => true,
                (Some(n), Some(s)) => n == s,
                (Some(_), None) => false,
            })
            .filter(|(_, doc, _, _)| {
                filter_map.map_or(true, |m| metadata_matches(&doc.metadata, m))
            })
            .map(|(_, doc, emb, _)| {
                let score = cosine_similarity(&query_vector, emb);
                (score, doc.clone())
            })
            .filter(|(s, _)| *s >= score_threshold)
            .collect();

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        let result: Vec<Document> = scored
            .into_iter()
            .take(limit)
            .map(|(score, mut doc)| {
                doc.score = score;
                doc
            })
            .collect();
        Ok(result)
    }

    async fn delete(&self, ids: &[String], _opt: &InMemoryOptions) -> Result<(), VectorStoreError> {
        if ids.is_empty() {
            return Ok(());
        }
        let ids_set: std::collections::HashSet<_> = ids.iter().collect();
        let mut data = self.data.write().map_err(|e| e.to_string())?;
        data.retain(|(id, _, _, _)| !ids_set.contains(id));
        Ok(())
    }
}
