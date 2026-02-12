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

#[cfg(feature = "yaml")]
use serde_yaml;

use crate::{
    document_loaders::{process_doc_stream, Loader, LoaderError},
    schemas::Document,
    text_splitter::TextSplitter,
};

/// YAML loader that loads YAML files
/// Supports YAML document streams (multiple documents separated by ---)
#[derive(Debug, Clone)]
pub struct YamlLoader<R> {
    reader: R,
}

impl<R: Read> YamlLoader<R> {
    pub fn new(reader: R) -> Self {
        Self { reader }
    }
}

impl YamlLoader<Cursor<Vec<u8>>> {
    pub fn from_string<S: Into<String>>(input: S) -> Self {
        let input = input.into();
        let reader = Cursor::new(input.into_bytes());
        Self::new(reader)
    }
}

impl YamlLoader<BufReader<File>> {
    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Self, LoaderError> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        Ok(Self::new(reader))
    }
}

#[async_trait]
impl<R: Read + Send + Sync + 'static> Loader for YamlLoader<R> {
    async fn load(
        mut self,
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<Document, LoaderError>> + Send + 'static>>,
        LoaderError,
    > {
        let mut content = String::new();
        self.reader.read_to_string(&mut content)?;

        let stream = stream! {
            #[cfg(feature = "yaml")]
            {
                // Try to parse as YAML document stream (multiple documents separated by ---)
                // For simplicity, parse as single document first
                let value: serde_yaml::Value = serde_yaml::from_str(&content)
                    .map_err(|e| LoaderError::YamlError(e))?;

                // Check if it's a sequence (array) - treat each item as a document
                let documents: Vec<serde_yaml::Value> = match value {
                    serde_yaml::Value::Sequence(seq) => seq,
                    _ => vec![value],
                };

                if documents.is_empty() {
                    // Single document or empty
                    let mut metadata = HashMap::new();
                    metadata.insert("source_type".to_string(), serde_json::Value::from("yaml"));
                    let doc = Document::new(content).with_metadata(metadata);
                    yield Ok(doc);
                } else {
                    // Multiple documents
                    for (idx, value) in documents.into_iter().enumerate() {
                        let content = serde_yaml::to_string(&value)
                            .map_err(|e| LoaderError::YamlError(e))?;

                        let mut metadata = HashMap::new();
                        metadata.insert("source_type".to_string(), serde_json::Value::from("yaml"));
                        metadata.insert("document_index".to_string(), serde_json::Value::from(idx));

                        let doc = Document::new(content).with_metadata(metadata);
                        yield Ok(doc);
                    }
                }
            }
            #[cfg(not(feature = "yaml"))]
            {
                yield Err(LoaderError::OtherError("YAML feature not enabled. Add 'yaml' feature to use YamlLoader.".to_string()));
            }
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
    async fn test_yaml_loader() {
        let input = r#"
name: John
age: 30
city: New York
"#;
        let loader = YamlLoader::from_string(input);

        let documents = loader
            .load()
            .await
            .unwrap()
            .map(|x| x.unwrap())
            .collect::<Vec<_>>()
            .await;

        assert_eq!(documents.len(), 1);
        assert!(documents[0].page_content.contains("John"));
    }

    #[tokio::test]
    async fn test_yaml_loader_multiple_docs() {
        let input = r#"---
name: John
age: 30
---
name: Jane
age: 25
"#;
        let loader = YamlLoader::from_string(input);

        let documents = loader
            .load()
            .await
            .unwrap()
            .map(|x| x.unwrap())
            .collect::<Vec<_>>()
            .await;

        assert_eq!(documents.len(), 2);
    }
}
