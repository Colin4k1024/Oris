use std::sync::Arc;

use pinecone_rs::Client;

use crate::embedding::embedder_trait::Embedder;

use super::Store;

pub struct StoreBuilder {
    client: Option<Client>,
    api_key: Option<String>,
    environment: Option<String>,
    index_name: Option<String>,
    embedder: Option<Arc<dyn Embedder>>,
}

impl StoreBuilder {
    pub fn new() -> Self {
        StoreBuilder {
            client: None,
            api_key: None,
            environment: None,
            index_name: None,
            embedder: None,
        }
    }

    pub fn client(mut self, client: Client) -> Self {
        self.client = Some(client);
        self
    }

    pub fn api_key<S: Into<String>>(mut self, api_key: S) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    pub fn environment<S: Into<String>>(mut self, environment: S) -> Self {
        self.environment = Some(environment.into());
        self
    }

    pub fn index_name<S: Into<String>>(mut self, name: S) -> Self {
        self.index_name = Some(name.into());
        self
    }

    pub fn embedder<E: Embedder + 'static>(mut self, embedder: E) -> Self {
        self.embedder = Some(Arc::new(embedder));
        self
    }

    pub async fn build(self) -> Result<Store, Box<dyn std::error::Error>> {
        let client = match self.client {
            Some(c) => c,
            None => {
                let api_key = self
                    .api_key
                    .ok_or("api_key is required when client is not provided")?;
                let environment = self
                    .environment
                    .ok_or("environment is required when client is not provided")?;
                Client::new(api_key, environment).await?
            }
        };
        let index_name = self.index_name.ok_or("index_name is required")?;
        let embedder = self.embedder.ok_or("embedder is required")?;
        let index = client.index(index_name);
        Ok(Store { index, embedder })
    }
}

impl Default for StoreBuilder {
    fn default() -> Self {
        Self::new()
    }
}
