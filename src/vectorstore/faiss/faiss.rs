use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use hnsw_rs::hnsw::Hnsw;
use hnsw_rs::prelude::DistL2;
use serde_json::Value;

use crate::{
    embedding::embedder_trait::Embedder,
    schemas::Document,
    vectorstore::{VecStoreOptions, VectorStore, VectorStoreError},
};

pub struct Store {
    pub(crate) hnsw: RwLock<Hnsw<'static, f32, DistL2>>,
    pub(crate) docstore: RwLock<Vec<Document>>,
    pub(crate) ids: RwLock<Vec<String>>,
    pub(crate) embedder: Arc<dyn Embedder>,
    pub(crate) dim: usize,
}

pub type FaissOptions = VecStoreOptions<Value>;

#[async_trait]
impl VectorStore for Store {
    type Options = FaissOptions;

    async fn add_documents(
        &self,
        docs: &[Document],
        opt: &FaissOptions,
    ) -> Result<Vec<String>, VectorStoreError> {
        let embedder = opt.embedder.as_ref().unwrap_or(&self.embedder);
        let texts: Vec<String> = docs.iter().map(|d| d.page_content.clone()).collect();
        let vectors = embedder.embed_documents(&texts).await?;
        if vectors.len() != docs.len() {
            return Err(VectorStoreError::InternalError(
                "Number of vectors and documents do not match".to_string(),
            ));
        }
        let mut ids = Vec::with_capacity(docs.len());
        let hnsw = self
            .hnsw
            .write()
            .map_err(|e| VectorStoreError::InternalError(e.to_string()))?;
        let mut docstore = self
            .docstore
            .write()
            .map_err(|e| VectorStoreError::InternalError(e.to_string()))?;
        let mut id_vec = self
            .ids
            .write()
            .map_err(|e| VectorStoreError::InternalError(e.to_string()))?;
        for (doc, vector) in docs.iter().zip(vectors.iter()) {
            let id = uuid::Uuid::new_v4().to_string();
            ids.push(id.clone());
            let idx = docstore.len();
            let mut d = doc.clone();
            d.score = 0.0;
            docstore.push(d);
            id_vec.push(id);
            let v_f32: Vec<f32> = vector.iter().map(|x| *x as f32).collect();
            hnsw.insert((v_f32.as_slice(), idx));
        }
        Ok(ids)
    }

    async fn similarity_search(
        &self,
        query: &str,
        limit: usize,
        opt: &FaissOptions,
    ) -> Result<Vec<Document>, VectorStoreError> {
        let embedder = opt.embedder.as_ref().unwrap_or(&self.embedder);
        let qv = embedder.embed_query(query).await?;
        let qv_f32: Vec<f32> = qv.into_iter().map(|x| x as f32).collect();
        let ef = (limit * 2).max(32);
        let hnsw = self
            .hnsw
            .read()
            .map_err(|e| VectorStoreError::InternalError(e.to_string()))?;
        let docstore = self
            .docstore
            .read()
            .map_err(|e| VectorStoreError::InternalError(e.to_string()))?;
        let neighbours = hnsw.search(qv_f32.as_slice(), limit, ef);
        let score_threshold = opt
            .score_threshold
            .map(f64::from)
            .unwrap_or(f64::NEG_INFINITY);
        let mut result: Vec<Document> = neighbours
            .into_iter()
            .filter_map(|n| {
                let idx = n.d_id;
                let dist = n.distance as f64;
                // L2: lower is more similar. Use -dist as score (higher = more similar).
                let score = -dist;
                if score < score_threshold {
                    return None;
                }
                docstore.get(idx).cloned().map(|mut d| {
                    d.score = score;
                    d
                })
            })
            .collect();
        result.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(result)
    }

    async fn delete(&self, _ids: &[String], _opt: &FaissOptions) -> Result<(), VectorStoreError> {
        Err(VectorStoreError::DeleteNotSupported)
    }
}
