use async_trait::async_trait;

use super::Document;
use crate::error::RetrieverError;

#[async_trait]
pub trait Retriever: Sync + Send {
    async fn get_relevant_documents(&self, query: &str) -> Result<Vec<Document>, RetrieverError>;
}

impl<R> From<R> for Box<dyn Retriever>
where
    R: Retriever + 'static,
{
    fn from(retriever: R) -> Self {
        Box::new(retriever)
    }
}
