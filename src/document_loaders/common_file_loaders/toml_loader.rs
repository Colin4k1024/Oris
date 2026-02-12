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

#[cfg(feature = "toml")]
use toml;

use crate::{
    document_loaders::{process_doc_stream, Loader, LoaderError},
    schemas::Document,
    text_splitter::TextSplitter,
};

/// TOML loader that loads TOML configuration files
#[derive(Debug, Clone)]
pub struct TomlLoader<R> {
    reader: R,
}

impl<R: Read> TomlLoader<R> {
    pub fn new(reader: R) -> Self {
        Self { reader }
    }
}

impl TomlLoader<Cursor<Vec<u8>>> {
    pub fn from_string<S: Into<String>>(input: S) -> Self {
        let input = input.into();
        let reader = Cursor::new(input.into_bytes());
        Self::new(reader)
    }
}

impl TomlLoader<BufReader<File>> {
    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Self, LoaderError> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        Ok(Self::new(reader))
    }
}

#[async_trait]
impl<R: Read + Send + Sync + 'static> Loader for TomlLoader<R> {
    async fn load(
        mut self,
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<Document, LoaderError>> + Send + 'static>>,
        LoaderError,
    > {
        let mut content = String::new();
        self.reader.read_to_string(&mut content)?;

        let stream = stream! {
            #[cfg(feature = "toml")]
            {
                // Parse TOML to validate it
                let _value: toml::Value = toml::from_str(&content)
                    .map_err(|e| LoaderError::TomlError(e))?;

                let mut metadata = HashMap::new();
                metadata.insert("source_type".to_string(), serde_json::Value::from("toml"));

                let doc = Document::new(content).with_metadata(metadata);
                yield Ok(doc);
            }
            #[cfg(not(feature = "toml"))]
            {
                yield Err(LoaderError::OtherError("TOML feature not enabled. Add 'toml' feature to use TomlLoader.".to_string()));
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
    async fn test_toml_loader() {
        let input = r#"
[package]
name = "test"
version = "1.0.0"

[dependencies]
serde = "1.0"
"#;
        let loader = TomlLoader::from_string(input);

        let documents = loader
            .load()
            .await
            .unwrap()
            .map(|x| x.unwrap())
            .collect::<Vec<_>>()
            .await;

        assert_eq!(documents.len(), 1);
        assert!(documents[0].page_content.contains("name = \"test\""));
    }
}
