use std::{collections::HashMap, pin::Pin, time::Duration};

use async_stream::stream;
use async_trait::async_trait;
use futures::Stream;
use reqwest::{header::HeaderMap, Client, ClientBuilder};
use serde_json::Value;
use url::Url;

use crate::{
    document_loaders::{process_doc_stream, Loader, LoaderError},
    schemas::Document,
    text_splitter::TextSplitter,
};

/// WebBaseLoader loads content from a URL
/// Uses reqwest to fetch the page and readability to extract main content
#[derive(Debug, Clone)]
pub struct WebBaseLoader {
    url: Url,
    client: Client,
    headers: Option<HeaderMap>,
    timeout: Option<Duration>,
}

impl WebBaseLoader {
    pub fn new(url: Url) -> Self {
        Self {
            url,
            client: Client::new(),
            headers: None,
            timeout: None,
        }
    }

    pub fn from_url_str<S: AsRef<str>>(url_str: S) -> Result<Self, LoaderError> {
        let url = Url::parse(url_str.as_ref())
            .map_err(|e| LoaderError::OtherError(format!("Invalid URL: {}", e)))?;
        Ok(Self::new(url))
    }

    /// Set custom headers
    pub fn with_headers(mut self, headers: HeaderMap) -> Self {
        self.headers = Some(headers);
        self
    }

    /// Set timeout for the request
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Set a custom HTTP client
    pub fn with_client(mut self, client: Client) -> Self {
        self.client = client;
        self
    }

    fn build_client(&self) -> Client {
        let mut builder = ClientBuilder::new();

        if let Some(ref headers) = self.headers {
            builder = builder.default_headers(headers.clone());
        }

        if let Some(timeout) = self.timeout {
            builder = builder.timeout(timeout);
        }

        builder.build().unwrap_or_else(|_| Client::new())
    }
}

#[async_trait]
impl Loader for WebBaseLoader {
    async fn load(
        self,
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<Document, LoaderError>> + Send + 'static>>,
        LoaderError,
    > {
        let url = self.url.clone();
        let client = self.build_client();

        let stream = stream! {
            // Fetch the page
            let response = client
                .get(url.as_str())
                .send()
                .await
                .map_err(|e| LoaderError::OtherError(format!("Failed to fetch URL: {}", e)))?;

            let status = response.status();
            if !status.is_success() {
                yield Err(LoaderError::OtherError(format!(
                    "HTTP error: {}",
                    status
                )));
                return;
            }

            let html = response
                .text()
                .await
                .map_err(|e| LoaderError::OtherError(format!("Failed to read response: {}", e)))?;

            // Extract main content using readability
            let mut html_reader = html.as_bytes();
            let cleaned = readability::extractor::extract(&mut html_reader, &url)
                .map_err(|e| LoaderError::ReadabilityError(e))?;

            let content = format!("{}\n{}", cleaned.title, cleaned.text);

            let mut metadata = HashMap::new();
            metadata.insert("source".to_string(), Value::from(url.as_str()));
            metadata.insert("source_type".to_string(), Value::from("web"));
            metadata.insert("title".to_string(), Value::from(cleaned.title));

            let doc = Document::new(content).with_metadata(metadata);
            yield Ok(doc);
        };

        Ok(Box::pin(stream))
    }

    async fn load_and_split<TS: TextSplitter + 'static>(
        self,
        splitter: TS,
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<Document, LoaderError>> + Send + 'static>>,
        LoaderError,
    > {
        let doc_stream = self.load().await?;
        let stream = process_doc_stream(doc_stream, splitter).await;
        Ok(Box::pin(stream))
    }
}

#[cfg(test)]
mod tests {
    use futures_util::StreamExt;

    use super::*;

    /// Integration test requiring network access
    ///
    /// Run with: cargo test test_web_base_loader -- --ignored
    #[tokio::test]
    #[ignore = "Requires network access and may timeout - run with: cargo test test_web_base_loader -- --ignored"]
    async fn test_web_base_loader() {
        let loader =
            WebBaseLoader::from_url_str("https://example.com").expect("Failed to create loader");

        let documents = loader
            .load()
            .await
            .unwrap()
            .map(|x| x.unwrap())
            .collect::<Vec<_>>()
            .await;

        assert_eq!(documents.len(), 1);
        assert!(documents[0].page_content.contains("Example"));
    }
}
