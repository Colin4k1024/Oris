use std::sync::Arc;

use mongodb::Collection;

use crate::embedding::embedder_trait::Embedder;

use super::Store;

pub struct StoreBuilder {
    collection: Option<Collection<mongodb::bson::Document>>,
    embedder: Option<Arc<dyn Embedder>>,
    index_name: Option<String>,
    vector_field: String,
    content_field: String,
}

impl StoreBuilder {
    pub fn new() -> Self {
        StoreBuilder {
            collection: None,
            embedder: None,
            index_name: None,
            vector_field: "embedding".to_string(),
            content_field: "page_content".to_string(),
        }
    }

    pub fn collection(mut self, collection: Collection<mongodb::bson::Document>) -> Self {
        self.collection = Some(collection);
        self
    }

    pub fn embedder<E: Embedder + 'static>(mut self, embedder: E) -> Self {
        self.embedder = Some(Arc::new(embedder));
        self
    }

    pub fn index_name(mut self, name: &str) -> Self {
        self.index_name = Some(name.to_string());
        self
    }

    pub fn vector_field(mut self, name: &str) -> Self {
        self.vector_field = name.to_string();
        self
    }

    pub fn content_field(mut self, name: &str) -> Self {
        self.content_field = name.to_string();
        self
    }

    pub fn build(self) -> Result<Store, Box<dyn std::error::Error>> {
        let collection = self.collection.ok_or("collection is required")?;
        let embedder = self.embedder.ok_or("embedder is required")?;
        let index_name = self
            .index_name
            .ok_or("index_name is required (Atlas Vector Search index)")?;
        Ok(Store {
            collection,
            embedder,
            index_name,
            vector_field: self.vector_field,
            content_field: self.content_field,
        })
    }
}

impl Default for StoreBuilder {
    fn default() -> Self {
        Self::new()
    }
}
