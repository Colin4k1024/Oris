use std::sync::Arc;

use chromadb::client::{ChromaClient, ChromaClientOptions};

use crate::embedding::embedder_trait::Embedder;

use super::Store;

pub struct StoreBuilder {
    client: Option<ChromaClient>,
    client_options: Option<ChromaClientOptions>,
    embedder: Option<Arc<dyn Embedder>>,
    collection_name: Option<String>,
}

impl StoreBuilder {
    pub fn new() -> Self {
        StoreBuilder {
            client: None,
            client_options: None,
            embedder: None,
            collection_name: None,
        }
    }

    pub fn client(mut self, client: ChromaClient) -> Self {
        self.client = Some(client);
        self
    }

    pub fn client_options(mut self, opts: ChromaClientOptions) -> Self {
        self.client_options = Some(opts);
        self
    }

    pub fn embedder<E: Embedder + 'static>(mut self, embedder: E) -> Self {
        self.embedder = Some(Arc::new(embedder));
        self
    }

    pub fn collection_name(mut self, name: &str) -> Self {
        self.collection_name = Some(name.to_string());
        self
    }

    pub async fn build(self) -> Result<Store, Box<dyn std::error::Error>> {
        let client = match self.client {
            Some(c) => c,
            None => {
                let opts = self.client_options.unwrap_or_default();
                ChromaClient::new(opts).await?
            }
        };
        let embedder = self.embedder.ok_or("embedder is required")?;
        let collection_name = self.collection_name.ok_or("collection_name is required")?;
        let collection = client
            .get_or_create_collection(&collection_name, None)
            .await?;
        Ok(Store {
            collection,
            embedder,
        })
    }
}

impl Default for StoreBuilder {
    fn default() -> Self {
        Self::new()
    }
}
