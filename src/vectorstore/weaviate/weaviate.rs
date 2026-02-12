use std::collections::HashMap;

use async_trait::async_trait;
use serde_json::Value;
use uuid::Uuid;
use weaviate_community::collections::objects::{Object, ObjectBuilder};
use weaviate_community::collections::query::GetBuilder;
use weaviate_community::WeaviateClient;

use crate::{
    embedding::embedder_trait::Embedder,
    schemas::Document,
    vectorstore::{VecStoreOptions, VectorStore, VectorStoreError},
};

pub struct Store {
    pub client: WeaviateClient,
    pub class_name: String,
    pub embedder: std::sync::Arc<dyn Embedder>,
}

pub type WeaviateOptions = VecStoreOptions<Value>;

#[async_trait]
impl VectorStore for Store {
    type Options = WeaviateOptions;

    async fn add_documents(
        &self,
        docs: &[Document],
        opt: &WeaviateOptions,
    ) -> Result<Vec<String>, VectorStoreError> {
        let _ = opt;
        let embedder = opt.embedder.as_ref().unwrap_or(&self.embedder);
        let texts: Vec<String> = docs.iter().map(|d| d.page_content.clone()).collect();
        let vectors = embedder.embed_documents(&texts).await?;
        if vectors.len() != docs.len() {
            return Err(VectorStoreError::InternalError(
                "Number of vectors and documents do not match".to_string(),
            ));
        }
        let mut ids = Vec::with_capacity(docs.len());
        for (doc, vec_f64) in docs.iter().zip(vectors.into_iter()) {
            let id = Uuid::new_v4();
            ids.push(id.to_string());
            let metadata_json =
                serde_json::to_string(&doc.metadata).unwrap_or_else(|_| "{}".to_string());
            let properties = serde_json::json!({
                "content": doc.page_content,
                "metadata": metadata_json,
            });
            let obj: Object = ObjectBuilder::new(&self.class_name, properties)
                .with_id(id)
                .with_vector(vec_f64)
                .build();
            self.client
                .objects
                .create(&obj, None)
                .await
                .map_err(|e| VectorStoreError::Unknown(e.to_string()))?;
        }
        Ok(ids)
    }

    async fn similarity_search(
        &self,
        query: &str,
        limit: usize,
        opt: &WeaviateOptions,
    ) -> Result<Vec<Document>, VectorStoreError> {
        let embedder = opt.embedder.as_ref().unwrap_or(&self.embedder);
        let qv = embedder.embed_query(query).await?;
        let near_vector_str = serde_json::to_string(&serde_json::json!({ "vector": qv }))?;
        let mut get = GetBuilder::new(&self.class_name, vec!["content", "metadata"])
            .with_limit(limit as u32)
            .with_near_vector(&near_vector_str)
            .with_additional(vec!["distance", "certainty"]);
        if let Some(ref w) = opt.filters {
            if let Some(s) = w.as_str() {
                get = get.with_where(s);
            }
        }
        let query_result = get.build();
        let raw = self
            .client
            .query
            .get(query_result)
            .await
            .map_err(|e| VectorStoreError::Unknown(e.to_string()))?;
        let score_threshold = opt
            .score_threshold
            .map(f64::from)
            .unwrap_or(f64::NEG_INFINITY);
        let docs = parse_get_response(&raw, &self.class_name, score_threshold)?;
        Ok(docs)
    }

    async fn delete(&self, ids: &[String], _opt: &WeaviateOptions) -> Result<(), VectorStoreError> {
        if ids.is_empty() {
            return Ok(());
        }
        for id in ids {
            let uuid = Uuid::parse_str(id).map_err(|e| {
                VectorStoreError::InvalidParameter(format!("invalid uuid {}: {}", id, e))
            })?;
            self.client
                .objects
                .delete(&self.class_name, &uuid, None, None)
                .await
                .map_err(|e| VectorStoreError::Unknown(e.to_string()))?;
        }
        Ok(())
    }
}

fn parse_get_response(
    raw: &Value,
    class_name: &str,
    score_threshold: f64,
) -> Result<Vec<Document>, VectorStoreError> {
    let get = raw
        .get("data")
        .and_then(|d| d.get("Get"))
        .and_then(|g| g.get(class_name))
        .and_then(|c| c.as_array())
        .ok_or_else(|| {
            VectorStoreError::DeserializationError(format!(
                "expected data.Get.{} in Weaviate response",
                class_name
            ))
        })?;
    let mut out = Vec::new();
    for obj in get {
        let content = obj
            .get("content")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();
        let metadata: HashMap<String, Value> = obj
            .get("metadata")
            .and_then(|m| serde_json::from_str(m.as_str().unwrap_or("{}")).ok())
            .unwrap_or_default();
        let score = obj
            .get("_additional")
            .and_then(|a| {
                a.get("certainty")
                    .and_then(|c| c.as_f64())
                    .or_else(|| a.get("distance").and_then(|d| d.as_f64()).map(|d| 1.0 - d))
            })
            .unwrap_or(0.0);
        if score >= score_threshold {
            out.push(Document {
                page_content: content,
                metadata,
                score,
            });
        }
    }
    Ok(out)
}
