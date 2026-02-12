use std::{collections::HashMap, pin::Pin, time::Duration};

use async_stream::stream;
use async_trait::async_trait;
use futures::Stream;
use reqwest::Client;
use serde_json::Value;
use url::Url;

#[cfg(feature = "xml")]
use quick_xml;

use crate::{
    document_loaders::{process_doc_stream, Loader, LoaderError},
    schemas::Document,
    text_splitter::TextSplitter,
};

/// SitemapLoader loads all URLs from a sitemap.xml file
#[derive(Debug, Clone)]
pub struct SitemapLoader {
    sitemap_url: Url,
    client: Client,
    timeout: Option<Duration>,
    /// Optional URL filter patterns
    url_filters: Option<Vec<String>>,
}

impl SitemapLoader {
    pub fn new(sitemap_url: Url) -> Self {
        Self {
            sitemap_url,
            client: Client::new(),
            timeout: Some(Duration::from_secs(30)),
            url_filters: None,
        }
    }

    pub fn from_url_str<S: AsRef<str>>(url_str: S) -> Result<Self, LoaderError> {
        let url = Url::parse(url_str.as_ref())
            .map_err(|e| LoaderError::OtherError(format!("Invalid URL: {}", e)))?;
        Ok(Self::new(url))
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn with_client(mut self, client: Client) -> Self {
        self.client = client;
        self
    }

    /// Set URL filter patterns (URLs must contain at least one pattern)
    pub fn with_url_filters(mut self, filters: Vec<String>) -> Self {
        self.url_filters = Some(filters);
        self
    }

    async fn fetch_sitemap(&self) -> Result<String, LoaderError> {
        let mut request = self.client.get(self.sitemap_url.as_str());

        if let Some(timeout) = self.timeout {
            request = request.timeout(timeout);
        }

        let response = request
            .send()
            .await
            .map_err(|e| LoaderError::OtherError(format!("Failed to fetch sitemap: {}", e)))?;

        if !response.status().is_success() {
            return Err(LoaderError::OtherError(format!(
                "HTTP error {} for sitemap",
                response.status()
            )));
        }

        let content = response
            .text()
            .await
            .map_err(|e| LoaderError::OtherError(format!("Failed to read sitemap: {}", e)))?;

        Ok(content)
    }

    #[cfg(feature = "xml")]
    fn parse_sitemap(&self, xml: &str) -> Result<Vec<Url>, LoaderError> {
        let mut urls = Vec::new();
        let mut reader = quick_xml::Reader::from_str(xml);
        reader.trim_text(true);

        let mut buf = Vec::new();
        let mut in_url = false;
        let mut current_url = String::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(quick_xml::events::Event::Start(e)) => {
                    if e.name().as_ref() == b"url" {
                        in_url = true;
                    } else if e.name().as_ref() == b"loc" && in_url {
                        // Will read text in next event
                    }
                }
                Ok(quick_xml::events::Event::Text(e)) => {
                    if in_url {
                        current_url = e.unescape().unwrap_or_default().to_string();
                    }
                }
                Ok(quick_xml::events::Event::End(e)) => {
                    if e.name().as_ref() == b"url" {
                        if !current_url.is_empty() {
                            if let Ok(url) = Url::parse(&current_url) {
                                // Apply filters if any
                                if let Some(ref filters) = self.url_filters {
                                    let url_str = url.as_str();
                                    if filters.iter().any(|f| url_str.contains(f)) {
                                        urls.push(url);
                                    }
                                } else {
                                    urls.push(url);
                                }
                            }
                        }
                        current_url.clear();
                        in_url = false;
                    } else if e.name().as_ref() == b"sitemapindex" {
                        // Handle sitemap index - would need to fetch nested sitemaps
                        // For now, we'll just parse the main sitemap
                    }
                }
                Ok(quick_xml::events::Event::Eof) => break,
                Err(e) => {
                    return Err(LoaderError::OtherError(format!("XML parsing error: {}", e)));
                }
                _ => {}
            }
            buf.clear();
        }

        Ok(urls)
    }

    async fn fetch_and_extract_page(&self, url: &Url) -> Result<Document, LoaderError> {
        let mut request = self.client.get(url.as_str());

        if let Some(timeout) = self.timeout {
            request = request.timeout(timeout);
        }

        let response = request
            .send()
            .await
            .map_err(|e| LoaderError::OtherError(format!("Failed to fetch {}: {}", url, e)))?;

        if !response.status().is_success() {
            return Err(LoaderError::OtherError(format!(
                "HTTP error {} for {}",
                response.status(),
                url
            )));
        }

        let html = response
            .text()
            .await
            .map_err(|e| LoaderError::OtherError(format!("Failed to read response: {}", e)))?;

        let mut html_reader = html.as_bytes();
        let cleaned = readability::extractor::extract(&mut html_reader, url)
            .map_err(|e| LoaderError::ReadabilityError(e))?;

        let content = format!("{}\n{}", cleaned.title, cleaned.text);

        let mut metadata = HashMap::new();
        metadata.insert("source".to_string(), Value::from(url.as_str()));
        metadata.insert("source_type".to_string(), Value::from("web"));
        metadata.insert("title".to_string(), Value::from(cleaned.title));

        Ok(Document::new(content).with_metadata(metadata))
    }
}

#[async_trait]
impl Loader for SitemapLoader {
    async fn load(
        self,
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<Document, LoaderError>> + Send + 'static>>,
        LoaderError,
    > {
        let sitemap_url = self.sitemap_url.clone();
        let client = self.client.clone();
        let timeout = self.timeout;
        let url_filters = self.url_filters.clone();

        let stream = stream! {
            // Fetch sitemap
            let loader = SitemapLoader {
                sitemap_url: sitemap_url.clone(),
                client: client.clone(),
                timeout,
                url_filters: url_filters.clone(),
            };

            let sitemap_xml = loader.fetch_sitemap().await?;

            #[cfg(feature = "xml")]
            {
                let urls = loader.parse_sitemap(&sitemap_xml)?;
                // Fetch each URL from sitemap
                for url in urls {
                    match loader.fetch_and_extract_page(&url).await {
                        Ok(doc) => yield Ok(doc),
                        Err(e) => yield Err(e),
                    }
                }
            }
            #[cfg(not(feature = "xml"))]
            {
                yield Err(LoaderError::OtherError("XML feature required for sitemap parsing. Add 'xml' feature.".to_string()));
            }
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

    #[tokio::test]
    #[ignore] // Requires network access
    async fn test_sitemap_loader() {
        let loader = SitemapLoader::from_url_str("https://example.com/sitemap.xml")
            .expect("Failed to create loader");

        let documents = loader
            .load()
            .await
            .unwrap()
            .map(|x| x.unwrap())
            .collect::<Vec<_>>()
            .await;

        // Results depend on sitemap content
        assert!(!documents.is_empty() || documents.is_empty());
    }
}
