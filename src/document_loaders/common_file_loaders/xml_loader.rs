use std::{
    collections::HashMap,
    fs::File,
    io::{BufReader, Cursor, Read},
    path::Path,
    pin::Pin,
};

use async_stream::stream;
use async_trait::async_trait;
use futures::Stream;

#[cfg(feature = "xml")]
use quick_xml;

use crate::{
    document_loaders::{process_doc_stream, Loader, LoaderError},
    schemas::Document,
    text_splitter::TextSplitter,
};

/// XML loader that loads and parses XML files
#[derive(Debug, Clone)]
pub struct XmlLoader<R> {
    reader: R,
    /// Optional XPath-like selector to extract specific elements
    /// If None, extracts all text content
    xpath_selector: Option<String>,
}

impl<R: Read> XmlLoader<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            xpath_selector: None,
        }
    }

    /// Set an XPath-like selector to extract specific elements
    /// Simple tag name support for now (e.g., "item", "article")
    pub fn with_selector<S: Into<String>>(mut self, selector: S) -> Self {
        self.xpath_selector = Some(selector.into());
        self
    }
}

impl XmlLoader<Cursor<Vec<u8>>> {
    pub fn from_string<S: Into<String>>(input: S) -> Self {
        let input = input.into();
        let reader = Cursor::new(input.into_bytes());
        Self::new(reader)
    }
}

impl XmlLoader<BufReader<File>> {
    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Self, LoaderError> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        Ok(Self::new(reader))
    }
}

#[async_trait]
impl<R: Read + Send + Sync + 'static> Loader for XmlLoader<R> {
    async fn load(
        mut self,
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<Document, LoaderError>> + Send + 'static>>,
        LoaderError,
    > {
        let mut content = String::new();
        self.reader.read_to_string(&mut content)?;

        let selector = self.xpath_selector.clone();

        let stream = stream! {
            // Parse XML using quick-xml
            #[cfg(feature = "xml")]
            let mut reader = quick_xml::Reader::from_str(&content);
            #[cfg(not(feature = "xml"))]
            {
                yield Err(LoaderError::OtherError("XML feature not enabled".to_string()));
                return;
            }

            #[cfg(feature = "xml")]
            {
            reader.trim_text(true);

            let mut buf = Vec::new();
            let mut current_text = String::new();
            let mut in_target_element = false;
            let mut _depth = 0;

            if let Some(ref sel) = selector {
                // Extract specific elements
                loop {
                    match reader.read_event_into(&mut buf) {
                        Ok(quick_xml::events::Event::Start(e)) => {
                            _depth += 1;
                            if e.name().as_ref() == sel.as_bytes() {
                                in_target_element = true;
                                current_text.clear();
                            }
                        }
                        Ok(quick_xml::events::Event::Text(e)) => {
                            if in_target_element {
                                current_text.push_str(&e.unescape().unwrap_or_default());
                                current_text.push(' ');
                            }
                        }
                        Ok(quick_xml::events::Event::End(e)) => {
                            if in_target_element && e.name().as_ref() == sel.as_bytes() {
                                // Found complete element
                                let mut metadata = HashMap::new();
                                metadata.insert("source_type".to_string(), serde_json::Value::from("xml"));
                                metadata.insert("element".to_string(), serde_json::Value::from(sel.clone()));

                                let doc = Document::new(current_text.trim().to_string()).with_metadata(metadata);
                                yield Ok(doc);

                                in_target_element = false;
                                current_text.clear();
                            }
                            _depth -= 1;
                        }
                        Ok(quick_xml::events::Event::Eof) => break,
                        Err(e) => {
                            yield Err(LoaderError::OtherError(format!("XML parsing error: {}", e)));
                            break;
                        }
                        _ => {}
                    }
                    buf.clear();
                }
            } else {
                // Extract all text content
                loop {
                    match reader.read_event_into(&mut buf) {
                        Ok(quick_xml::events::Event::Text(e)) => {
                            current_text.push_str(&e.unescape().unwrap_or_default());
                            current_text.push(' ');
                        }
                        Ok(quick_xml::events::Event::Eof) => break,
                        Err(e) => {
                            yield Err(LoaderError::OtherError(format!("XML parsing error: {}", e)));
                            break;
                        }
                        _ => {}
                    }
                    buf.clear();
                }

                if !current_text.trim().is_empty() {
                    let mut metadata = HashMap::new();
                    metadata.insert("source_type".to_string(), serde_json::Value::from("xml"));

                    let doc = Document::new(current_text.trim().to_string()).with_metadata(metadata);
                    yield Ok(doc);
                }
            }
            } // End of #[cfg(feature = "xml")]
        };

        Ok(Box::pin(stream))
    }

    async fn load_and_split<TS: TextSplitter + 'static>(
        mut self,
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
    async fn test_xml_loader() {
        let input = r#"<root><item>Hello</item><item>World</item></root>"#;
        let loader = XmlLoader::from_string(input);

        let documents = loader
            .load()
            .await
            .unwrap()
            .map(|x| x.unwrap())
            .collect::<Vec<_>>()
            .await;

        assert_eq!(documents.len(), 1);
        assert!(documents[0].page_content.contains("Hello"));
    }

    #[tokio::test]
    async fn test_xml_loader_with_selector() {
        let input = r#"<root><item>Hello</item><item>World</item></root>"#;
        let loader = XmlLoader::from_string(input).with_selector("item");

        let documents = loader
            .load()
            .await
            .unwrap()
            .map(|x| x.unwrap())
            .collect::<Vec<_>>()
            .await;

        assert_eq!(documents.len(), 2);
        assert!(documents[0].page_content.contains("Hello"));
        assert!(documents[1].page_content.contains("World"));
    }
}
