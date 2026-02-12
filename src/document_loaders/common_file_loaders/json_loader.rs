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
use serde_json::{error::Error as JsonError, Value};

use crate::{
    document_loaders::{process_doc_stream, Loader, LoaderError},
    schemas::Document,
    text_splitter::TextSplitter,
};

/// JSON loader that supports multiple JSON formats:
/// - Single JSON object
/// - Array of JSON objects
/// - JSONL (JSON Lines) - one JSON object per line
#[derive(Debug, Clone)]
pub struct JsonLoader<R> {
    reader: R,
    /// Optional field to extract from each JSON object
    /// If None, the entire JSON is converted to string
    jq_spec: Option<String>,
}

impl<R: Read> JsonLoader<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            jq_spec: None,
        }
    }

    /// Set a jq-like specification to extract specific fields
    /// Example: ".content" to extract the "content" field
    pub fn with_jq_spec<S: Into<String>>(mut self, jq_spec: S) -> Self {
        self.jq_spec = Some(jq_spec.into());
        self
    }
}

impl JsonLoader<Cursor<Vec<u8>>> {
    pub fn from_string<S: Into<String>>(input: S) -> Self {
        let input = input.into();
        let reader = Cursor::new(input.into_bytes());
        Self::new(reader)
    }
}

impl JsonLoader<BufReader<File>> {
    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Self, LoaderError> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        Ok(Self::new(reader))
    }
}

#[async_trait]
impl<R: Read + Send + Sync + 'static> Loader for JsonLoader<R> {
    async fn load(
        mut self,
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<Document, LoaderError>> + Send + 'static>>,
        LoaderError,
    > {
        let mut content = String::new();
        self.reader.read_to_string(&mut content)?;

        let jq_spec = self.jq_spec.clone();

        let stream = stream! {
            // Try to parse as JSONL first (one JSON per line)
            let lines: Vec<&str> = content.lines().collect();
            let mut is_jsonl = false;
            let mut jsonl_docs = Vec::new();

            for (line_num, line) in lines.iter().enumerate() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                match serde_json::from_str::<Value>(trimmed) {
                    Ok(value) => {
                        is_jsonl = true;
                        let doc = create_document_from_json_value(&value, jq_spec.as_deref(), line_num + 1)?;
                        jsonl_docs.push(doc);
                    }
                    Err(_) => {
                        // Not valid JSON on this line, might be regular JSON
                        break;
                    }
                }
            }

            if is_jsonl && !jsonl_docs.is_empty() {
                // It's JSONL format
                for doc in jsonl_docs {
                    yield Ok(doc);
                }
            } else {
                // Try to parse as regular JSON
                match serde_json::from_str::<Value>(&content) {
                    Ok(value) => {
                        match value {
                            Value::Array(arr) => {
                                // Array of objects
                                for (idx, item) in arr.into_iter().enumerate() {
                                    let doc = create_document_from_json_value(&item, jq_spec.as_deref(), idx)?;
                                    yield Ok(doc);
                                }
                            }
                            _ => {
                                // Single object
                                let doc = create_document_from_json_value(&value, jq_spec.as_deref(), 0)?;
                                yield Ok(doc);
                            }
                        }
                    }
                    Err(e) => {
                        yield Err(LoaderError::JsonError(e));
                    }
                }
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

fn create_document_from_json_value(
    value: &Value,
    jq_spec: Option<&str>,
    index: usize,
) -> Result<Document, LoaderError> {
    let content = if let Some(spec) = jq_spec {
        // Simple jq-like spec support (only dot notation for now)
        extract_field(value, spec)
            .unwrap_or_else(|| serde_json::to_string(value).unwrap_or_default())
    } else {
        serde_json::to_string(value).map_err(|e| LoaderError::JsonError(JsonError::from(e)))?
    };

    let mut metadata = HashMap::new();
    metadata.insert("index".to_string(), Value::from(index));
    metadata.insert("source_type".to_string(), Value::from("json"));

    let doc = Document::new(content).with_metadata(metadata);
    Ok(doc)
}

fn extract_field(value: &Value, spec: &str) -> Option<String> {
    let spec = spec.trim_start_matches('.');
    let parts: Vec<&str> = spec.split('.').collect();

    let mut current = value;
    for part in parts {
        match current {
            Value::Object(map) => {
                current = map.get(part)?;
            }
            _ => return None,
        }
    }

    match current {
        Value::String(s) => Some(s.clone()),
        _ => serde_json::to_string(current).ok(),
    }
}

#[cfg(test)]
mod tests {
    use futures_util::StreamExt;

    use super::*;

    #[tokio::test]
    async fn test_json_loader_single_object() {
        let input = r#"{"name": "John", "age": 30, "city": "New York"}"#;
        let loader = JsonLoader::from_string(input);

        let documents = loader
            .load()
            .await
            .unwrap()
            .map(|x| x.unwrap())
            .collect::<Vec<_>>()
            .await;

        assert_eq!(documents.len(), 1);
        assert!(documents[0].page_content.contains("John"));
        assert_eq!(documents[0].metadata.get("index").unwrap(), &Value::from(0));
    }

    #[tokio::test]
    async fn test_json_loader_array() {
        let input = r#"[{"name": "John", "age": 30}, {"name": "Jane", "age": 25}]"#;
        let loader = JsonLoader::from_string(input);

        let documents = loader
            .load()
            .await
            .unwrap()
            .map(|x| x.unwrap())
            .collect::<Vec<_>>()
            .await;

        assert_eq!(documents.len(), 2);
        assert!(documents[0].page_content.contains("John"));
        assert!(documents[1].page_content.contains("Jane"));
    }

    #[tokio::test]
    async fn test_json_loader_jsonl() {
        let input = r#"{"name": "John", "age": 30}
{"name": "Jane", "age": 25}
{"name": "Bob", "age": 35}"#;
        let loader = JsonLoader::from_string(input);

        let documents = loader
            .load()
            .await
            .unwrap()
            .map(|x| x.unwrap())
            .collect::<Vec<_>>()
            .await;

        assert_eq!(documents.len(), 3);
        assert!(documents[0].page_content.contains("John"));
        assert!(documents[1].page_content.contains("Jane"));
        assert!(documents[2].page_content.contains("Bob"));
    }

    #[tokio::test]
    async fn test_json_loader_with_jq_spec() {
        let input = r#"{"content": "Hello world", "metadata": {"author": "John"}}"#;
        let loader = JsonLoader::from_string(input).with_jq_spec(".content");

        let documents = loader
            .load()
            .await
            .unwrap()
            .map(|x| x.unwrap())
            .collect::<Vec<_>>()
            .await;

        assert_eq!(documents.len(), 1);
        assert_eq!(documents[0].page_content, "\"Hello world\"");
    }
}
