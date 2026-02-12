use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use mongodb::bson::{doc, Bson, Document};
use serde_json::Value;

use crate::{
    embedding::embedder_trait::Embedder,
    schemas::Document as LangchainDocument,
    vectorstore::{VecStoreOptions, VectorStore, VectorStoreError},
};

pub struct Store {
    pub collection: mongodb::Collection<Document>,
    pub embedder: Arc<dyn Embedder>,
    pub index_name: String,
    pub vector_field: String,
    pub content_field: String,
}

pub type MongoOptions = VecStoreOptions<Value>;

fn bson_to_value(b: &Bson) -> Value {
    match b {
        Bson::Null => Value::Null,
        Bson::Boolean(x) => Value::Bool(*x),
        Bson::Int32(x) => Value::Number(serde_json::Number::from(*x)),
        Bson::Int64(x) => Value::Number(serde_json::Number::from(*x)),
        Bson::Double(x) => serde_json::Number::from_f64(*x)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        Bson::String(s) => Value::String(s.clone()),
        Bson::Array(a) => Value::Array(a.iter().map(bson_to_value).collect()),
        Bson::Document(d) => Value::Object(
            d.iter()
                .map(|(k, v)| (k.clone(), bson_to_value(v)))
                .collect(),
        ),
        _ => Value::Null,
    }
}

fn value_to_bson(v: &Value) -> Bson {
    match v {
        Value::Null => Bson::Null,
        Value::Bool(b) => Bson::Boolean(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Bson::Int64(i)
            } else if let Some(f) = n.as_f64() {
                Bson::Double(f)
            } else {
                Bson::Null
            }
        }
        Value::String(s) => Bson::String(s.clone()),
        Value::Array(arr) => Bson::Array(arr.iter().map(value_to_bson).collect()),
        Value::Object(obj) => Bson::Document(
            obj.iter()
                .map(|(k, v)| (k.clone(), value_to_bson(v)))
                .collect(),
        ),
    }
}

#[async_trait]
impl VectorStore for Store {
    type Options = MongoOptions;

    async fn add_documents(
        &self,
        docs: &[LangchainDocument],
        opt: &MongoOptions,
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
        for (doc, vector) in docs.iter().zip(vectors.iter()) {
            let id = uuid::Uuid::new_v4();
            let mut bson_doc = doc! {
                "_id": id.to_string(),
                &self.content_field: doc.page_content.clone(),
                &self.vector_field: vector.iter().map(|x| Bson::Double(*x)).collect::<Vec<_>>(),
                "metadata": doc.metadata.iter().map(|(k,v)| (k.clone(), value_to_bson(v))).collect::<Document>()
            };
            if let Some(ref ns) = opt.name_space {
                bson_doc.insert("namespace", ns.as_str());
            }
            self.collection
                .insert_one(bson_doc, None)
                .await
                .map_err(|e| VectorStoreError::Unknown(e.to_string()))?;
            ids.push(id.to_string());
        }
        Ok(ids)
    }

    async fn similarity_search(
        &self,
        query: &str,
        limit: usize,
        opt: &MongoOptions,
    ) -> Result<Vec<LangchainDocument>, VectorStoreError> {
        let embedder = opt.embedder.as_ref().unwrap_or(&self.embedder);
        let qv = embedder.embed_query(query).await?;
        let qv_bson: Vec<Bson> = qv.iter().map(|x| Bson::Double(*x)).collect();

        let mut filter_doc = Document::new();
        if let Some(ref f) = opt.filters {
            if let Some(obj) = f.as_object() {
                for (k, v) in obj {
                    filter_doc.insert(k.clone(), value_to_bson(v));
                }
            }
        }
        if let Some(ref ns) = opt.name_space {
            filter_doc.insert("namespace", ns.as_str());
        }

        let mut vs_doc = doc! {
            "index": &self.index_name,
            "path": &self.vector_field,
            "queryVector": qv_bson,
                "numCandidates": (limit * 20).min(10000) as i32,
                "limit": limit as i32
        };
        if !filter_doc.is_empty() {
            vs_doc.insert("filter", filter_doc);
        }
        let vector_search_stage = doc! { "$vectorSearch": vs_doc };

        let mut project_doc = Document::new();
        project_doc.insert("score", doc! { "$meta": "vectorSearchScore" });
        project_doc.insert(
            "page_content",
            Bson::String(format!("${}", self.content_field)),
        );
        project_doc.insert("metadata", doc! { "$ifNull": [ "$metadata", {} ] });

        let pipeline = vec![vector_search_stage, doc! { "$project": project_doc }];

        let mut cursor = self
            .collection
            .aggregate(pipeline, None)
            .await
            .map_err(|e| VectorStoreError::Unknown(e.to_string()))?;
        let mut result = Vec::new();
        while cursor
            .advance()
            .await
            .map_err(|e| VectorStoreError::Unknown(e.to_string()))?
        {
            let d = cursor
                .deserialize_current()
                .map_err(|e| VectorStoreError::Unknown(e.to_string()))?;
            let page_content = d.get_str("page_content").unwrap_or("").to_string();
            let metadata: HashMap<String, Value> = d
                .get_document("metadata")
                .map(|m| {
                    m.iter()
                        .map(|(k, v)| (k.clone(), bson_to_value(v)))
                        .collect()
                })
                .unwrap_or_default();
            let score = d.get_f64("score").unwrap_or(0.0);
            result.push(LangchainDocument {
                page_content,
                metadata,
                score,
            });
        }
        Ok(result)
    }

    async fn delete(&self, ids: &[String], _opt: &MongoOptions) -> Result<(), VectorStoreError> {
        if ids.is_empty() {
            return Ok(());
        }
        self.collection
            .delete_many(doc! { "_id": { "$in": ids } }, None)
            .await
            .map_err(|e| VectorStoreError::Unknown(e.to_string()))?;
        Ok(())
    }
}
