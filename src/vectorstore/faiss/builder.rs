use std::sync::Arc;

use hnsw_rs::hnsw::Hnsw;
use hnsw_rs::prelude::DistL2;

use crate::embedding::embedder_trait::Embedder;

use super::Store;

pub struct StoreBuilder {
    embedder: Option<Arc<dyn Embedder>>,
    dim: Option<usize>,
    max_elements: usize,
    ef_construction: usize,
    max_nb_connection: usize,
}

impl StoreBuilder {
    pub fn new() -> Self {
        StoreBuilder {
            embedder: None,
            dim: None,
            max_elements: 10_000,
            ef_construction: 200,
            max_nb_connection: 16,
        }
    }

    pub fn embedder<E: Embedder + 'static>(mut self, embedder: E) -> Self {
        self.embedder = Some(Arc::new(embedder));
        self
    }

    pub fn dim(mut self, dim: usize) -> Self {
        self.dim = Some(dim);
        self
    }

    pub fn max_elements(mut self, n: usize) -> Self {
        self.max_elements = n;
        self
    }

    pub async fn build(self) -> Result<Store, Box<dyn std::error::Error>> {
        let embedder = self.embedder.ok_or("embedder is required")?;
        let dim = match self.dim {
            Some(d) => d,
            None => {
                let v = embedder.embed_query("x").await?;
                v.len()
            }
        };
        let hnsw = Hnsw::<f32, DistL2>::new(
            self.max_nb_connection,
            self.max_elements,
            16,
            self.ef_construction,
            DistL2,
        );
        Ok(Store {
            hnsw: std::sync::RwLock::new(hnsw),
            docstore: std::sync::RwLock::new(Vec::new()),
            ids: std::sync::RwLock::new(Vec::new()),
            embedder,
            dim,
        })
    }
}

impl Default for StoreBuilder {
    fn default() -> Self {
        Self::new()
    }
}
