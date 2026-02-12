use std::sync::Arc;

use weaviate_community::WeaviateClient;

use crate::embedding::embedder_trait::Embedder;

use super::Store;

/// Builder for Weaviate vector store.
///
/// The Weaviate class (schema) must exist with:
/// - `content` (text) – page content
/// - `metadata` (text) – JSON-serialized metadata
/// - `vectorizer` set to `"none"` when providing your own vectors.
pub struct StoreBuilder {
    client: Option<WeaviateClient>,
    base_url: Option<String>,
    auth_secret: Option<String>,
    class_name: Option<String>,
    embedder: Option<Arc<dyn Embedder>>,
}

impl StoreBuilder {
    pub fn new() -> Self {
        StoreBuilder {
            client: None,
            base_url: None,
            auth_secret: None,
            class_name: None,
            embedder: None,
        }
    }

    pub fn client(mut self, client: WeaviateClient) -> Self {
        self.client = Some(client);
        self
    }

    pub fn base_url<S: Into<String>>(mut self, url: S) -> Self {
        self.base_url = Some(url.into());
        self
    }

    pub fn auth_secret<S: Into<String>>(mut self, secret: S) -> Self {
        self.auth_secret = Some(secret.into());
        self
    }

    pub fn class_name<S: Into<String>>(mut self, name: S) -> Self {
        self.class_name = Some(name.into());
        self
    }

    pub fn embedder<E: Embedder + 'static>(mut self, embedder: E) -> Self {
        self.embedder = Some(Arc::new(embedder));
        self
    }

    pub fn build(self) -> Result<Store, Box<dyn std::error::Error>> {
        let client = match self.client {
            Some(c) => c,
            None => {
                let url = self.base_url.ok_or("base_url or client is required")?;
                let mut b = WeaviateClient::builder(&url);
                if let Some(ref secret) = self.auth_secret {
                    b = b.with_auth_secret(secret.as_str());
                }
                b.build()?
            }
        };
        let class_name = self.class_name.ok_or("class_name is required")?;
        let embedder = self.embedder.ok_or("embedder is required")?;
        Ok(Store {
            client,
            class_name,
            embedder,
        })
    }
}

impl Default for StoreBuilder {
    fn default() -> Self {
        Self::new()
    }
}
