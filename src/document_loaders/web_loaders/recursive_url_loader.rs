use std::{
    collections::{HashMap, HashSet},
    pin::Pin,
    time::Duration,
};

use async_stream::stream;
use async_trait::async_trait;
use futures::Stream;
use reqwest::Client;
use scraper::{Html, Selector};
use serde_json::Value;
use url::Url;

use crate::{
    document_loaders::{process_doc_stream, Loader, LoaderError},
    schemas::Document,
    text_splitter::TextSplitter,
};

/// Configuration for RecursiveURLLoader
#[derive(Debug, Clone)]
pub struct RecursiveUrlConfig {
    /// Maximum depth to crawl (0 = only root URL)
    pub max_depth: usize,
    /// Maximum number of pages to load
    pub max_pages: Option<usize>,
    /// URL patterns to include (if None, all URLs are included)
    pub include_patterns: Option<Vec<String>>,
    /// URL patterns to exclude
    pub exclude_patterns: Option<Vec<String>>,
    /// Timeout for each request
    pub timeout: Option<Duration>,
}

impl Default for RecursiveUrlConfig {
    fn default() -> Self {
        Self {
            max_depth: 2,
            max_pages: Some(10),
            include_patterns: None,
            exclude_patterns: None,
            timeout: Some(Duration::from_secs(30)),
        }
    }
}

/// RecursiveURLLoader recursively crawls a website starting from a root URL
#[derive(Debug, Clone)]
pub struct RecursiveUrlLoader {
    root_url: Url,
    config: RecursiveUrlConfig,
    client: Client,
}

impl RecursiveUrlLoader {
    pub fn new(root_url: Url) -> Self {
        Self {
            root_url,
            config: RecursiveUrlConfig::default(),
            client: Client::new(),
        }
    }

    pub fn from_url_str<S: AsRef<str>>(url_str: S) -> Result<Self, LoaderError> {
        let url = Url::parse(url_str.as_ref())
            .map_err(|e| LoaderError::OtherError(format!("Invalid URL: {}", e)))?;
        Ok(Self::new(url))
    }

    pub fn with_config(mut self, config: RecursiveUrlConfig) -> Self {
        self.config = config;
        self
    }

    pub fn with_client(mut self, client: Client) -> Self {
        self.client = client;
        self
    }

    fn should_include_url(&self, url: &Url) -> bool {
        let url_str = url.as_str();

        // Check exclude patterns
        if let Some(ref patterns) = self.config.exclude_patterns {
            for pattern in patterns {
                if url_str.contains(pattern) {
                    return false;
                }
            }
        }

        // Check include patterns
        if let Some(ref patterns) = self.config.include_patterns {
            for pattern in patterns {
                if url_str.contains(pattern) {
                    return true;
                }
            }
            return false; // If include patterns exist but none match, exclude
        }

        true
    }

    fn extract_links(&self, html: &str, base_url: &Url) -> Vec<Url> {
        let document = Html::parse_document(html);
        let selector = Selector::parse("a[href]").unwrap();
        let mut links = Vec::new();

        for element in document.select(&selector) {
            if let Some(href) = element.value().attr("href") {
                if let Ok(url) = base_url.join(href) {
                    // Only include same-domain URLs
                    if url.host_str() == base_url.host_str() {
                        links.push(url);
                    }
                }
            }
        }

        links
    }

    async fn fetch_page(&self, url: &Url) -> Result<String, LoaderError> {
        let mut request = self.client.get(url.as_str());

        if let Some(timeout) = self.config.timeout {
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

        Ok(html)
    }
}

#[async_trait]
impl Loader for RecursiveUrlLoader {
    async fn load(
        self,
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<Document, LoaderError>> + Send + 'static>>,
        LoaderError,
    > {
        let root_url = self.root_url.clone();
        let config = self.config.clone();
        let _client = self.client.clone();

        let stream = stream! {
            let mut visited = HashSet::new();
            let mut to_visit = vec![(root_url.clone(), 0)]; // (url, depth)
            let mut pages_loaded = 0;

            while let Some((url, depth)) = to_visit.pop() {
                // Check limits
                if depth > config.max_depth {
                    continue;
                }

                if let Some(max_pages) = config.max_pages {
                    if pages_loaded >= max_pages {
                        break;
                    }
                }

                // Skip if already visited
                if visited.contains(&url) {
                    continue;
                }

                // Check if URL should be included
                if !self.should_include_url(&url) {
                    continue;
                }

                visited.insert(url.clone());
                pages_loaded += 1;

                // Fetch the page
                match self.fetch_page(&url).await {
                    Ok(html) => {
                        // Extract content
                        let mut html_reader = html.as_bytes();
                        match readability::extractor::extract(&mut html_reader, &url) {
                            Ok(cleaned) => {
                                let content = format!("{}\n{}", cleaned.title, cleaned.text);

                                let mut metadata = HashMap::new();
                                metadata.insert("source".to_string(), Value::from(url.as_str()));
                                metadata.insert("source_type".to_string(), Value::from("web"));
                                metadata.insert("title".to_string(), Value::from(cleaned.title.clone()));
                                metadata.insert("depth".to_string(), Value::from(depth));

                                let doc = Document::new(content).with_metadata(metadata);
                                yield Ok(doc);

                                // Extract links for next level
                                if depth < config.max_depth {
                                    let links = self.extract_links(&html, &url);
                                    for link in links {
                                        if !visited.contains(&link) {
                                            to_visit.push((link, depth + 1));
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                yield Err(LoaderError::ReadabilityError(e));
                            }
                        }
                    }
                    Err(e) => {
                        yield Err(e);
                    }
                }
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
    async fn test_recursive_url_loader() {
        let config = RecursiveUrlConfig {
            max_depth: 1,
            max_pages: Some(3),
            ..Default::default()
        };

        let loader = RecursiveUrlLoader::from_url_str("https://example.com")
            .expect("Failed to create loader")
            .with_config(config);

        let documents = loader
            .load()
            .await
            .unwrap()
            .map(|x| x.unwrap())
            .collect::<Vec<_>>()
            .await;

        assert!(!documents.is_empty());
    }
}
