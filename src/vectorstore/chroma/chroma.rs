use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chromadb::collection::{ChromaCollection, CollectionEntries, QueryOptions, QueryResult};
use serde_json::{Map, Value};

use crate::{
    embedding::embedder_trait::Embedder,
    schemas::Document,
    vectorstore::{VecStoreOptions, VectorStore, VectorStoreError},
};

pub struct Store {
    pub collection: ChromaCollection,
    pub embedder: Arc<dyn Embedder>,
}

pub type ChromaOptions = VecStoreOptions<Value>;

#[async_trait]
impl VectorStore for Store {
    type Options = ChromaOptions;

    async fn add_documents(
        &self,
        docs: &[Document],
        opt: &ChromaOptions,
    ) -> Result<Vec<String>, VectorStoreError> {
        let embedder = opt.embedder.as_ref().unwrap_or(&self.embedder);
        let texts: Vec<String> = docs.iter().map(|d| d.page_content.clone()).collect();
        let vectors = embedder.embed_documents(&texts).await?;
        if vectors.len() != docs.len() {
            return Err(VectorStoreError::InternalError(
                "Number of vectors and documents do not match".to_string(),
            ));
        }
        let ids: Vec<String> = docs
            .iter()
            .map(|_| uuid::Uuid::new_v4().to_string())
            .collect();
        let embeddings_f32: Vec<Vec<f32>> = vectors
            .into_iter()
            .map(|v| v.into_iter().map(|x| x as f32).collect())
            .collect();
        let metadatas: Vec<Map<String, Value>> = docs
            .iter()
            .map(|d| {
                d.metadata
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect()
            })
            .collect();
        let entries = CollectionEntries {
            ids: ids.iter().map(|s| s.as_str()).collect(),
            embeddings: Some(embeddings_f32),
            metadatas: Some(metadatas),
            documents: Some(docs.iter().map(|d| d.page_content.as_str()).collect()),
        };
        self.collection
            .upsert(entries, None)
            .await
            .map_err(|e| VectorStoreError::Unknown(e.to_string()))?;
        Ok(ids)
    }

    async fn similarity_search(
        &self,
        query: &str,
        limit: usize,
        opt: &ChromaOptions,
    ) -> Result<Vec<Document>, VectorStoreError> {
        let embedder = opt.embedder.as_ref().unwrap_or(&self.embedder);
        let qv = embedder.embed_query(query).await?;
        let qv_f32: Vec<f32> = qv.into_iter().map(|x| x as f32).collect();
        let query_opts = QueryOptions {
            query_embeddings: Some(vec![qv_f32]),
            query_texts: None,
            n_results: Some(limit),
            where_metadata: opt.filters.clone(),
            where_document: None,
            include: Some(vec!["documents", "metadatas", "distances"]),
        };
        let result: QueryResult = self
            .collection
            .query(query_opts, None)
            .await
            .map_err(|e| VectorStoreError::Unknown(e.to_string()))?;
        let documents = result.documents.and_then(|d| d.into_iter().next());
        let metadatas = result.metadatas.and_then(|m| m.into_iter().next());
        let distances = result.distances.and_then(|d| d.into_iter().next());
        let docs = match (documents, metadatas, distances) {
            (Some(docs), meta, dist) => {
                let meta = meta.unwrap_or_else(|| (0..docs.len()).map(|_| None).collect());
                let dist = dist.unwrap_or_else(|| (0..docs.len()).map(|_| 0.0_f32).collect());
                docs.into_iter()
                    .zip(meta.into_iter().zip(dist.into_iter()))
                    .map(|(page_content, (metadata, d))| {
                        let metadata: HashMap<String, Value> = metadata
                            .map(|m| m.into_iter().collect())
                            .unwrap_or_default();
                        // Chroma returns distance (lower = more similar). Use 1 - normalized as score.
                        let score = 1.0 - (d as f64).min(1.0).max(0.0);
                        Document {
                            page_content,
                            metadata,
                            score,
                        }
                    })
                    .collect()
            }
            _ => Vec::new(),
        };
        Ok(docs)
    }

    async fn delete(&self, ids: &[String], _opt: &ChromaOptions) -> Result<(), VectorStoreError> {
        if ids.is_empty() {
            return Ok(());
        }
        let id_refs: Vec<&str> = ids.iter().map(|s| s.as_str()).collect();
        self.collection
            .delete(Some(id_refs), None, None)
            .await
            .map_err(|e| VectorStoreError::Unknown(e.to_string()))?;
        Ok(())
    }
}
