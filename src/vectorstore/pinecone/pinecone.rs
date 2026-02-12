use std::collections::BTreeMap;

use async_trait::async_trait;
use pinecone_rs::models::{Match, QueryRequest, QueryResponse, Vector};
use pinecone_rs::Index;
use serde_json::Value;

use crate::{
    embedding::embedder_trait::Embedder,
    schemas::Document,
    vectorstore::{VecStoreOptions, VectorStore, VectorStoreError},
};

pub struct Store {
    pub index: Index,
    pub embedder: std::sync::Arc<dyn Embedder>,
}

pub type PineconeOptions = VecStoreOptions<Value>;

fn metadata_to_btreemap(doc: &Document) -> BTreeMap<String, Value> {
    let mut m: BTreeMap<String, Value> = doc
        .metadata
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    m.insert(
        "content".to_string(),
        Value::String(doc.page_content.clone()),
    );
    m
}

fn filters_to_btreemap(f: &Option<Value>) -> Option<BTreeMap<String, Value>> {
    f.as_ref().and_then(|v| {
        v.as_object()
            .map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
    })
}

#[async_trait]
impl VectorStore for Store {
    type Options = PineconeOptions;

    async fn add_documents(
        &self,
        docs: &[Document],
        opt: &PineconeOptions,
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
        let namespace = opt.name_space.as_deref().unwrap_or("").to_string();
        let vectors: Vec<Vector> = ids
            .iter()
            .zip(docs.iter())
            .zip(vectors.into_iter())
            .map(|((id, doc), vec_f64)| {
                let values: Vec<f32> = vec_f64.into_iter().map(|x| x as f32).collect();
                let metadata = Some(metadata_to_btreemap(doc));
                Vector {
                    id: id.clone(),
                    values,
                    sparse_values: None,
                    metadata,
                }
            })
            .collect();
        self.index
            .upsert(namespace, vectors)
            .await
            .map_err(|e| VectorStoreError::Unknown(e.to_string()))?;
        Ok(ids)
    }

    async fn similarity_search(
        &self,
        query: &str,
        limit: usize,
        opt: &PineconeOptions,
    ) -> Result<Vec<Document>, VectorStoreError> {
        let embedder = opt.embedder.as_ref().unwrap_or(&self.embedder);
        let qv = embedder.embed_query(query).await?;
        let qv_f32: Vec<f32> = qv.into_iter().map(|x| x as f32).collect();
        let namespace = opt.name_space.clone();
        let filter = filters_to_btreemap(&opt.filters);
        let request = QueryRequest {
            namespace,
            top_k: limit,
            filter,
            include_values: false,
            include_metadata: true,
            vector: Some(qv_f32),
            sparse_vector: None,
            id: None,
        };
        let resp: QueryResponse = self
            .index
            .query(request)
            .await
            .map_err(|e| VectorStoreError::Unknown(e.to_string()))?;
        let score_threshold = opt
            .score_threshold
            .map(f64::from)
            .unwrap_or(f64::NEG_INFINITY);
        let docs: Vec<Document> = resp
            .matches
            .into_iter()
            .filter_map(|m: Match| {
                let score = m.score.map(f64::from).unwrap_or(0.0);
                if score < score_threshold {
                    return None;
                }
                let (page_content, metadata) = match m.metadata {
                    Some(meta) => {
                        let pc = meta
                            .get("content")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let md: std::collections::HashMap<String, Value> =
                            meta.into_iter().filter(|(k, _)| k != "content").collect();
                        (pc, md)
                    }
                    None => (String::new(), std::collections::HashMap::new()),
                };
                Some(Document {
                    page_content,
                    metadata,
                    score,
                })
            })
            .collect();
        Ok(docs)
    }

    async fn delete(
        &self,
        _ids: &[String],
        _opt: &PineconeOptions,
    ) -> Result<(), VectorStoreError> {
        Err(VectorStoreError::DeleteNotSupported)
    }
}
